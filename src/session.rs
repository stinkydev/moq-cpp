use anyhow::{Context, Result};
use bytes::Bytes;
use rand::Rng;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, watch, RwLock};
use tokio::time::{sleep, Instant};
use tracing::{debug, error, info, warn, Level};

use moq_lite::{
    Broadcast, BroadcastConsumer, BroadcastProducer, GroupProducer, Origin, OriginConsumer,
    OriginProducer, Session, Track, TrackConsumer, TrackProducer,
};
use moq_native::Client;

use crate::catalog::{Catalog, CatalogType, TrackDefinition};
use crate::config::{SessionConfig, WrapperError};
use crate::subscription::SubscriptionManager;

/// Log callback function type for session-specific logging
pub type SessionLogCallback = Box<dyn Fn(&str, Level, &str) + Send + Sync>;

/// Macro for session-aware logging that sends to both tracing and session callback
macro_rules! session_log {
    ($session:expr, info, $($arg:tt)*) => {
        {
            let message = format!($($arg)*);
            let target = module_path!();
            tracing::info!("{}", message);

            let session = $session.clone();
            tokio::spawn(async move {
                session.session_log(Level::INFO, target, &message).await;
            });
        }
    };
    ($session:expr, debug, $($arg:tt)*) => {
        {
            let message = format!($($arg)*);
            let target = module_path!();
            tracing::debug!("{}", message);

            let session = $session.clone();
            tokio::spawn(async move {
                session.session_log(Level::DEBUG, target, &message).await;
            });
        }
    };
    ($session:expr, warn, $($arg:tt)*) => {
        {
            let message = format!($($arg)*);
            let target = module_path!();
            tracing::warn!("{}", message);

            let session = $session.clone();
            tokio::spawn(async move {
                session.session_log(Level::WARN, target, &message).await;
            });
        }
    };
    ($session:expr, error, $($arg:tt)*) => {
        {
            let message = format!($($arg)*);
            let target = module_path!();
            tracing::error!("{}", message);

            let session = $session.clone();
            tokio::spawn(async move {
                session.session_log(Level::ERROR, target, &message).await;
            });
        }
    };
}

#[derive(Clone, Debug)]
pub enum SessionType {
    Publisher,
    Subscriber,
}

#[derive(Clone, Debug)]
pub enum SessionEvent {
    Connected,
    Disconnected { reason: String },
    BroadcastAnnounced { path: String },
    BroadcastUnannounced { path: String },
    TrackRequested { name: String },
    Error { error: String },
}

/// A high-level wrapper around moq-native that provides:
/// - Automatic reconnection for both publish and subscribe sessions
/// - Session lifecycle management
/// - Event notifications
/// - Easy-to-use API for publishing and subscribing with direct frame operations
#[derive(Clone)]
pub struct MoqSession {
    config: SessionConfig,
    session_type: SessionType,
    client: Client,
    broadcast_name: String, // Store the broadcast name for publishers

    // Internal state
    state: Arc<RwLock<SessionState>>,

    // Track management for publishers
    tracks: Arc<RwLock<HashMap<String, TrackHandle>>>,
    current_groups: Arc<RwLock<HashMap<String, GroupProducer>>>,
    sequence_numbers: Arc<RwLock<HashMap<String, u64>>>,

    // Catalog management
    catalog: Arc<RwLock<Option<Catalog>>>,
    catalog_type: Arc<RwLock<CatalogType>>,
    catalog_published: Arc<RwLock<bool>>,
    requested_tracks: Arc<RwLock<Vec<TrackDefinition>>>,

    // Event notification
    event_tx: mpsc::UnboundedSender<SessionEvent>,
    event_rx: Arc<RwLock<Option<mpsc::UnboundedReceiver<SessionEvent>>>>,

    // Internal broadcast channel for announcement events (for ResilientTrackConsumer)
    announcement_tx: broadcast::Sender<String>,

    // Subscription management
    subscription_manager: Arc<RwLock<Option<SubscriptionManager>>>,

    // Shutdown signal
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,

    // Session logging
    log_callback: Arc<RwLock<Option<SessionLogCallback>>>,
}

#[derive(Clone)]
struct SessionState {
    connected: bool,
    connection_attempts: usize,
    last_connection_time: Option<Instant>,
    current_session: Option<SessionHandle>,
    broadcast: Option<BroadcastHandle>,
}

#[derive(Clone)]
#[allow(dead_code)]
struct TrackHandle {
    producer: Option<TrackProducer>,
    consumer: Option<TrackConsumer>,
    track_info: Track,
    track_definition: Option<TrackDefinition>,
}

#[derive(Clone)]
#[allow(dead_code)]
struct BroadcastHandle {
    producer: Option<BroadcastProducer>,
    consumer: Option<BroadcastConsumer>,
}

#[derive(Clone)]
struct SessionHandle {
    session: Arc<Session<web_transport_quinn::Session>>,
    origin_producer: Option<OriginProducer>,
    origin_consumer: Option<OriginConsumer>,
}

impl MoqSession {
    /// Create a new publisher session
    pub async fn publisher(config: SessionConfig, broadcast_name: String) -> Result<Self> {
        Self::new(config, SessionType::Publisher, broadcast_name).await
    }

    /// Create a new subscriber session
    pub async fn subscriber(config: SessionConfig, broadcast_name: String) -> Result<Self> {
        Self::new(config, SessionType::Subscriber, broadcast_name).await
    }

    async fn new(
        config: SessionConfig,
        session_type: SessionType,
        broadcast_name: String,
    ) -> Result<Self> {
        let mut client_config = config.connection.client_config.clone();

        // Force IPv4 binding on Windows to avoid IPv6 issues
        #[cfg(windows)]
        {
            client_config.bind = "0.0.0.0:0".parse().expect("Valid IPv4 bind address");
        }

        // Also respect the explicit ipv4_only flag
        if config.connection.ipv4_only {
            client_config.bind = "0.0.0.0:0".parse().expect("Valid IPv4 bind address");
        }

        let client = client_config
            .init()
            .context("Failed to initialize MoQ client")?;

        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (announcement_tx, _) = broadcast::channel(100); // Buffer up to 100 announcements

        let state = Arc::new(RwLock::new(SessionState {
            connected: false,
            connection_attempts: 0,
            last_connection_time: None,
            current_session: None,
            broadcast: None,
        }));

        let session = Self {
            config,
            session_type: session_type.clone(),
            client,
            broadcast_name,
            state,
            tracks: Arc::new(RwLock::new(HashMap::new())),
            current_groups: Arc::new(RwLock::new(HashMap::new())),
            sequence_numbers: Arc::new(RwLock::new(HashMap::new())),
            catalog: Arc::new(RwLock::new(None)),
            catalog_type: Arc::new(RwLock::new(CatalogType::None)),
            catalog_published: Arc::new(RwLock::new(false)),
            requested_tracks: Arc::new(RwLock::new(Vec::new())),
            event_tx,
            event_rx: Arc::new(RwLock::new(Some(event_rx))),
            announcement_tx,
            subscription_manager: Arc::new(RwLock::new(None)),
            shutdown_tx,
            shutdown_rx,
            log_callback: Arc::new(RwLock::new(None)),
        };

        // Initialize subscription manager after session creation
        *session.subscription_manager.write().await =
            Some(SubscriptionManager::new(session.clone()));

        Ok(session)
    }

    /// Start the session and handle connections/reconnections
    pub async fn start(&self) -> Result<()> {
        session_log!(self, info, "Starting MoQ session: {:?}", self.session_type);

        let state = self.state.clone();
        let config = self.config.clone();
        let client = self.client.clone();
        let event_tx = self.event_tx.clone();
        let session_type = self.session_type.clone();
        let broadcast_name = self.broadcast_name.clone();
        let mut shutdown_rx = self.shutdown_rx.clone();
        let announcement_tx = self.announcement_tx.clone();
        let session_clone = self.clone();

        tokio::spawn(async move {
            let mut reconnect_delay = config.connection.reconnect_delay;

            loop {
                // Check for shutdown signal
                if *shutdown_rx.borrow() {
                    info!("Shutdown signal received, stopping session");
                    break;
                }

                let result = Self::establish_connection(
                    &config,
                    &client,
                    &session_type,
                    &broadcast_name,
                    state.clone(),
                    event_tx.clone(),
                    announcement_tx.clone(),
                )
                .await;

                match result {
                    Ok(session_handle) => {
                        info!("Successfully established MoQ connection");

                        // Reset reconnection state on successful connection
                        {
                            let mut state_guard = state.write().await;
                            state_guard.connected = true;
                            state_guard.connection_attempts = 0;
                            state_guard.last_connection_time = Some(Instant::now());
                            state_guard.current_session = Some(session_handle.clone());
                        }

                        reconnect_delay = config.connection.reconnect_delay;

                        // Create track producers for publisher sessions on every connection
                        if matches!(session_type, SessionType::Publisher) {
                            let session_for_tracks = session_clone.clone();
                            // Create track producers before sending Connected event
                            if let Err(e) = session_for_tracks.create_track_producers().await {
                                warn!("Failed to create track producers: {}", e);
                                let _ = event_tx.send(SessionEvent::Error {
                                    error: format!("Failed to create track producers: {}", e),
                                });
                            } else {
                                info!("Successfully created track producers");
                                // Only send Connected event after track producers are ready
                                let _ = event_tx.send(SessionEvent::Connected);
                            }
                        } else {
                            // For subscribers, send Connected immediately
                            let _ = event_tx.send(SessionEvent::Connected);
                        }

                        // Auto-subscribe to requested tracks for subscriber sessions
                        if matches!(session_type, SessionType::Subscriber) {
                            let session_for_auto_sub = session_clone.clone();
                            tokio::spawn(async move {
                                if let Err(e) =
                                    session_for_auto_sub.auto_subscribe_to_tracks().await
                                {
                                    warn!("Failed to auto-subscribe to tracks: {}", e);
                                }
                            });
                        }

                        // Wait for session to close or shutdown signal
                        tokio::select! {
                            result = session_handle.session.closed() => {
                                match result {
                                    Ok(()) => {
                                        info!("Session closed normally");
                                        let _ = event_tx.send(SessionEvent::Disconnected {
                                            reason: "Session closed normally".to_string()
                                        });
                                    }
                                    Err(e) => {
                                        error!("Session closed with error: {}", e);
                                        let _ = event_tx.send(SessionEvent::Disconnected {
                                            reason: format!("Session error: {}", e)
                                        });
                                    }
                                }
                            }
                            _ = shutdown_rx.changed() => {
                                if *shutdown_rx.borrow() {
                                    info!("Shutdown requested, closing session");
                                    break;
                                }
                            }
                        }

                        // Mark as disconnected and clean up session state
                        {
                            let mut state_guard = state.write().await;
                            state_guard.connected = false;
                            state_guard.current_session = None;
                            state_guard.broadcast = None; // Clear broadcast so it gets recreated on reconnection
                        }

                        // Clear current groups and reset catalog published flag for reconnection
                        {
                            let session_for_cleanup = session_clone.clone();
                            tokio::spawn(async move {
                                session_for_cleanup.current_groups.write().await.clear();
                                *session_for_cleanup.catalog_published.write().await = false;
                                info!("Cleared session state for reconnection");
                            });
                        }
                    }
                    Err(e) => {
                        let mut state_guard = state.write().await;
                        state_guard.connected = false;
                        state_guard.connection_attempts += 1;
                        state_guard.current_session = None;

                        error!(
                            "Failed to establish connection (attempt {}): {}",
                            state_guard.connection_attempts, e
                        );

                        let _ = event_tx.send(SessionEvent::Error {
                            error: format!("Connection failed: {}", e),
                        });

                        // Always continue reconnecting if auto_reconnect is enabled
                        // We never give up as long as auto_reconnect is true
                        if !config.auto_reconnect {
                            error!("Auto-reconnect disabled, giving up");
                            let _ = event_tx.send(SessionEvent::Error {
                                error: "Reconnection disabled".to_string(),
                            });
                            break;
                        }

                        drop(state_guard);
                    }
                }

                // If auto-reconnect is disabled and we're not connected, break
                if !config.auto_reconnect {
                    break;
                }

                // Wait before attempting to reconnect
                debug!("Waiting {:?} before reconnection attempt", reconnect_delay);
                info!(
                    "Attempting to reconnect... (attempt {} - will continue indefinitely)",
                    state.read().await.connection_attempts + 1
                );
                sleep(reconnect_delay).await;

                // Exponential backoff with maximum delay
                reconnect_delay =
                    std::cmp::min(reconnect_delay * 2, config.connection.max_reconnect_delay);
            }

            info!("Session management task terminated");
        });

        Ok(())
    }

    async fn establish_connection(
        config: &SessionConfig,
        client: &Client,
        session_type: &SessionType,
        broadcast_name: &str,
        state: Arc<RwLock<SessionState>>,
        event_tx: mpsc::UnboundedSender<SessionEvent>,
        announcement_tx: broadcast::Sender<String>,
    ) -> Result<SessionHandle> {
        debug!("Establishing connection to: {}", config.connection.url);

        // Establish WebTransport/QUIC connection
        let connection = client
            .connect(config.connection.url.clone())
            .await
            .context("Failed to connect to relay")?;

        // Set up origin for publish/subscribe operations
        let origin = Origin::produce();

        let (origin_consumer, origin_producer, broadcast_handle) = match session_type {
            SessionType::Publisher => {
                // For publishing, create broadcast first, then publish to origin before connecting
                let broadcast_produce = Broadcast::produce();

                // Publish the broadcast to origin with the specified name
                origin
                    .producer
                    .publish_broadcast(broadcast_name, broadcast_produce.consumer);

                // Store broadcast handle for later track creation
                let broadcast_handle = Some(BroadcastHandle {
                    producer: Some(broadcast_produce.producer),
                    consumer: None,
                });

                // For session connection, we provide the consumer and no producer
                (Some(origin.consumer), None, broadcast_handle)
            }
            SessionType::Subscriber => {
                // For subscribing, we provide the producer to the session
                // and keep the consumer to receive announcements
                (None, Some(origin.producer), None)
            }
        };

        // Perform MoQ handshake
        let session = Session::connect(connection, origin_consumer, origin_producer.clone())
            .await
            .context("Failed to perform MoQ handshake")?;

        let session_handle = SessionHandle {
            session: Arc::new(session),
            origin_producer: origin_producer.clone(),
            origin_consumer: origin_producer.map(|p| p.consume()),
        };

        // Store broadcast handle in state if we're a publisher
        if let Some(broadcast_handle) = broadcast_handle {
            let mut state_guard = state.write().await;
            state_guard.broadcast = Some(broadcast_handle);
        }

        // For subscribers, start monitoring announcements
        if let SessionType::Subscriber = session_type {
            if let Some(origin_consumer) = &session_handle.origin_consumer {
                Self::monitor_announcements(
                    origin_consumer.clone(),
                    event_tx.clone(),
                    announcement_tx.clone(),
                )
                .await;
            }
        }

        Ok(session_handle)
    }

    #[allow(dead_code)]
    async fn setup_publisher_broadcast(_state: Arc<RwLock<SessionState>>) -> Result<()> {
        // This method would be called to setup the broadcast after connection
        // For now, we'll set it up when tracks are actually used
        Ok(())
    }

    /// Set up broadcast for a publisher session
    async fn monitor_announcements(
        mut origin_consumer: OriginConsumer,
        event_tx: mpsc::UnboundedSender<SessionEvent>,
        announcement_tx: broadcast::Sender<String>,
    ) {
        tokio::spawn(async move {
            while let Some((path, broadcast)) = origin_consumer.announced().await {
                match broadcast {
                    Some(_) => {
                        debug!("Broadcast announced: {}", path);
                        let _ = event_tx.send(SessionEvent::BroadcastAnnounced {
                            path: path.to_string(),
                        });
                        // Also send to internal broadcast channel for ResilientTrackConsumer
                        let _ = announcement_tx.send(path.to_string());
                    }
                    None => {
                        debug!("Broadcast unannounced: {}", path);
                        let _ = event_tx.send(SessionEvent::BroadcastUnannounced {
                            path: path.to_string(),
                        });
                    }
                }
            }
        });
    }

    /// Get the next session event
    pub async fn next_event(&self) -> Option<SessionEvent> {
        let mut guard = self.event_rx.write().await;
        if let Some(rx) = guard.as_mut() {
            rx.recv().await
        } else {
            None
        }
    }

    /// Subscribe to broadcast announcement events (internal use)
    pub fn subscribe_announcements(&self) -> broadcast::Receiver<String> {
        self.announcement_tx.subscribe()
    }

    /// Check if the session is currently connected
    pub async fn is_connected(&self) -> bool {
        self.state.read().await.connected
    }

    /// Get connection statistics
    pub async fn connection_info(&self) -> ConnectionInfo {
        let state = self.state.read().await;
        ConnectionInfo {
            connected: state.connected,
            connection_attempts: state.connection_attempts,
            last_connection_time: state.last_connection_time,
        }
    }

    /// Stop the session and close all connections
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping MoQ session");

        // Send shutdown signal
        let _ = self.shutdown_tx.send(true);

        // Close current session if connected
        let state = self.state.read().await;
        if let Some(_session_handle) = &state.current_session {
            // Session handle will be dropped, which should close the connection
            // (*session_handle.session).clone().close(moq_lite::Error::App(1)); // App code 1 for session closed
        }

        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct ConnectionInfo {
    pub connected: bool,
    pub connection_attempts: usize,
    pub last_connection_time: Option<Instant>,
}

/// Publisher-specific functionality
impl MoqSession {
    /// Add a track definition to the session
    pub fn add_track_definition(&mut self, track_def: TrackDefinition) -> Result<()> {
        let track = Track {
            name: track_def.name.clone(),
            priority: track_def.priority.try_into().unwrap_or(0),
        };

        let track_handle = TrackHandle {
            producer: None, // Will be created when session connects
            consumer: None,
            track_info: track,
            track_definition: Some(track_def.clone()),
        };

        // Store track for later creation when session connects
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                self.tracks
                    .write()
                    .await
                    .insert(track_def.name.clone(), track_handle);

                // Generate random starting group sequence number for this track
                let mut rng = rand::thread_rng();
                let random_start: u64 = rng.gen_range(1..=10000);

                self.sequence_numbers
                    .write()
                    .await
                    .insert(track_def.name.clone(), random_start);

                info!(
                    "Track '{}' initialized with random starting group sequence: {}",
                    track_def.name, random_start
                );

                // Add to requested tracks if subscriber
                if matches!(self.session_type, SessionType::Subscriber) {
                    self.requested_tracks.write().await.push(track_def.clone());
                }
            })
        });

        info!(
            "Added track definition: {} ({})",
            track_def.name, track_def.track_type
        );
        Ok(())
    }

    /// Set catalog for publisher
    pub fn set_catalog(&mut self, catalog: Catalog) -> Result<()> {
        if !matches!(self.session_type, SessionType::Publisher) {
            return Err(
                WrapperError::Session("Only publishers can set catalog".to_string()).into(),
            );
        }

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                *self.catalog.write().await = Some(catalog);
            })
        });

        // Add catalog.json track
        let catalog_track = TrackDefinition::data("catalog.json", u32::MAX); // Highest priority
        self.add_track_definition(catalog_track)?;

        info!("Set catalog for publisher");
        Ok(())
    }

    /// Set catalog type for subscriber
    pub fn set_catalog_type(&mut self, catalog_type: CatalogType) -> Result<()> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                *self.catalog_type.write().await = catalog_type.clone();
            })
        });

        info!("Set catalog type: {:?}", catalog_type);
        Ok(())
    }

    /// Set a log callback to receive session-specific log messages
    ///
    /// # Arguments
    /// * `callback` - Optional callback function that receives (target, level, message)
    ///
    /// # Example
    /// ```rust
    /// use tracing::Level;
    ///
    /// session.set_log_callback(Some(Box::new(|target, level, message| {
    ///     println!("[SESSION][{}] {}: {}", level, target, message);
    /// })));
    /// ```
    pub async fn set_log_callback(&self, callback: Option<SessionLogCallback>) {
        *self.log_callback.write().await = callback;
    }

    /// Internal method to log session-specific messages
    /// Only logs messages with targets related to this session
    async fn session_log(&self, level: Level, target: &str, message: &str) {
        // Filter to only session-related log messages
        if target.starts_with("moq_wrapper::session")
            || target.starts_with("moq_ffi")
            || target.starts_with("session")
        {
            if let Some(callback) = self.log_callback.read().await.as_ref() {
                callback(target, level, message);
            }
        }
    }

    /// Publish catalog data to catalog.json track (internal method called during setup)
    async fn publish_catalog(&self) -> Result<()> {
        let catalog_guard = self.catalog.read().await;
        if let Some(catalog) = catalog_guard.as_ref() {
            let catalog_json = catalog.to_json().map_err(|e| {
                WrapperError::Session(format!("Failed to serialize catalog: {}", e))
            })?;

            // Get the catalog track producer directly to avoid recursion
            let tracks = self.tracks.read().await;
            if let Some(catalog_handle) = tracks.get("catalog.json") {
                if let Some(track_producer) = &catalog_handle.producer {
                    let mut track_producer = track_producer.clone();
                    track_producer.write_frame(Bytes::from(catalog_json));
                    info!("Published catalog data");
                }
            }
        }
        Ok(())
    }
    /// Add a track to the session (legacy method for backward compatibility)
    pub fn add_track(&mut self, track: Track) -> Result<()> {
        let track_def = TrackDefinition {
            name: track.name.clone(),
            priority: track.priority.into(),
            track_type: crate::catalog::TrackType::Data, // Default to data
        };
        self.add_track_definition(track_def)
    }

    /// Start a new group for the specified track
    pub async fn start_group(&self, track_name: &str) -> Result<()> {
        if !matches!(self.session_type, SessionType::Publisher) {
            return Err(WrapperError::Session("Not a publisher session".to_string()).into());
        }

        let state = self.state.read().await;
        if !state.connected {
            return Err(WrapperError::Session("Not connected".to_string()).into());
        }

        if state.broadcast.is_none() {
            drop(state);
            return Err(WrapperError::Session(
                "No broadcast available - session not properly initialized".to_string(),
            )
            .into());
        }
        drop(state); // Release the read lock

        // Close any existing group for this track
        self.close_group(track_name).await?;

        // Get track producer
        let mut track_producer = {
            let tracks = self.tracks.read().await;
            let track_handle = tracks
                .get(track_name)
                .ok_or_else(|| WrapperError::TrackNotFound(track_name.to_string()))?;
            track_handle
                .producer
                .as_ref()
                .ok_or_else(|| WrapperError::Session("Track producer not available".to_string()))?
                .clone()
        };

        // Get and increment sequence number
        let sequence = {
            let mut sequences = self.sequence_numbers.write().await;
            let seq = sequences
                .get_mut(track_name)
                .ok_or_else(|| WrapperError::TrackNotFound(track_name.to_string()))?;
            let current = *seq;
            *seq += 1;
            current
        };

        // Create new group
        let group = track_producer
            .create_group(sequence.into())
            .ok_or_else(|| WrapperError::Session("Failed to create group".to_string()))?;

        // Store the group
        self.current_groups
            .write()
            .await
            .insert(track_name.to_string(), group);

        debug!("Started group {} for track {}", sequence, track_name);
        Ok(())
    }

    /// Write a frame to the current group of the specified track
    pub async fn write_frame(&self, track_name: &str, data: Bytes) -> Result<()> {
        if !matches!(self.session_type, SessionType::Publisher) {
            return Err(WrapperError::Session("Not a publisher session".to_string()).into());
        }

        // Check connection status
        if !self.is_connected().await {
            return Err(WrapperError::Session(
                "Session not connected - reconnection in progress".to_string(),
            )
            .into());
        }

        // Check if we have an active group, if not, create one
        {
            let groups = self.current_groups.read().await;
            if !groups.contains_key(track_name) {
                drop(groups); // Release read lock before calling start_group
                              // Automatically start a group if none exists
                self.start_group(track_name).await?;
            }
        }

        let mut groups = self.current_groups.write().await;
        let group = groups.get_mut(track_name).ok_or_else(|| {
            WrapperError::Session(format!(
                "Failed to get group for track {} - session may be reconnecting",
                track_name
            ))
        })?;

        group.write_frame(data);
        Ok(())
    }

    /// Write a string frame (convenience method)
    pub async fn write_string(&self, track_name: &str, data: &str) -> Result<()> {
        self.write_frame(track_name, Bytes::from(data.to_string()))
            .await
    }

    /// Write a single frame and automatically manage the group
    pub async fn write_single_frame(&self, track_name: &str, data: Bytes) -> Result<()> {
        if !matches!(self.session_type, SessionType::Publisher) {
            return Err(WrapperError::Session("Not a publisher session".to_string()).into());
        }

        // Check connection status
        if !self.is_connected().await {
            return Err(WrapperError::Session(
                "Session not connected - reconnection in progress".to_string(),
            )
            .into());
        }

        self.start_group(track_name).await?;
        self.write_frame(track_name, data).await?;
        self.close_group(track_name).await?;
        Ok(())
    }

    /// Close the current group for the specified track
    pub async fn close_group(&self, track_name: &str) -> Result<()> {
        if !matches!(self.session_type, SessionType::Publisher) {
            return Err(WrapperError::Session("Not a publisher session".to_string()).into());
        }

        let mut groups = self.current_groups.write().await;
        if let Some(group) = groups.remove(track_name) {
            group.close();
            debug!("Closed group for track {}", track_name);
        }
        Ok(())
    }

    /// Get list of configured tracks
    pub async fn list_tracks(&self) -> Vec<String> {
        self.tracks.read().await.keys().cloned().collect()
    }

    /// Simplified publish data function that handles group creation internally  
    pub async fn publish_data(&self, track_name: &str, data: Vec<u8>) -> Result<(), WrapperError> {
        if !matches!(self.session_type, SessionType::Publisher) {
            return Err(WrapperError::Session("Not a publisher session".to_string()));
        }

        // Use the existing write_single_frame method which handles group creation
        self.write_single_frame(track_name, Bytes::from(data))
            .await
            .map_err(|e| WrapperError::Session(format!("Failed to publish data: {}", e)))
    }

    /// Publish a broadcast (only available for publisher sessions)
    pub async fn publish_broadcast(&self, broadcast: BroadcastConsumer) -> Result<()> {
        let state = self.state.read().await;

        let session_handle = state
            .current_session
            .as_ref()
            .ok_or_else(|| WrapperError::Session("Not connected".to_string()))?;

        if let SessionType::Publisher = self.session_type {
            if let Some(origin_producer) = &session_handle.origin_producer {
                let success =
                    origin_producer.publish_broadcast(&self.config.broadcast_name, broadcast);
                if success {
                    info!(
                        "Successfully published broadcast: {}",
                        self.config.broadcast_name
                    );
                    Ok(())
                } else {
                    Err(WrapperError::Session("Failed to publish broadcast".to_string()).into())
                }
            } else {
                Err(WrapperError::Session("No origin producer available".to_string()).into())
            }
        } else {
            Err(WrapperError::Session("Not a publisher session".to_string()).into())
        }
    }

    /// Create track producers from the existing broadcast (internal method, called automatically)
    pub async fn create_track_producers(&self) -> Result<()> {
        if !matches!(self.session_type, SessionType::Publisher) {
            return Ok(());
        }

        // Get the existing broadcast producer
        let broadcast_producer = {
            let state = self.state.read().await;
            if let Some(broadcast_handle) = &state.broadcast {
                broadcast_handle.producer.clone()
            } else {
                return Err(WrapperError::Session(
                    "No broadcast available for track creation".to_string(),
                )
                .into());
            }
        };

        if let Some(mut broadcast_producer) = broadcast_producer {
            // Create track producers for all configured tracks
            let mut tracks = self.tracks.write().await;
            for (name, handle) in tracks.iter_mut() {
                if handle.producer.is_none() {
                    let track_producer = broadcast_producer.create_track(handle.track_info.clone());
                    handle.producer = Some(track_producer);
                    info!("Created track producer for: {}", name);
                }
            }
            drop(tracks); // Release lock before catalog publishing
        }

        // Publish catalog after all tracks are set up (only once)
        let should_publish_catalog = {
            let mut catalog_published = self.catalog_published.write().await;
            if !*catalog_published && self.catalog.read().await.is_some() {
                *catalog_published = true;
                true
            } else {
                false
            }
        };

        if should_publish_catalog {
            if let Err(e) = self.publish_catalog().await {
                warn!("Failed to publish catalog: {}", e);
            } else {
                info!("Published catalog data after track setup");
            }
        }

        Ok(())
    }

    /// Set a data callback for receiving track data automatically
    pub async fn set_data_callback<F>(&self, callback: F)
    where
        F: Fn(String, Vec<u8>) + Send + Sync + 'static,
    {
        let subscription_manager = self.subscription_manager.read().await;
        if let Some(manager) = subscription_manager.as_ref() {
            manager.set_data_callback(Arc::new(callback)).await;
        }
    }

    /// Subscribe to a track with automatic data callback handling
    pub async fn subscribe_track_with_callback(
        &self,
        broadcast_name: &str,
        track_name: &str,
    ) -> Result<()> {
        if !matches!(self.session_type, SessionType::Subscriber) {
            return Err(WrapperError::Session("Not a subscriber session".to_string()).into());
        }

        let subscription_manager = self.subscription_manager.read().await;
        if let Some(manager) = subscription_manager.as_ref() {
            manager
                .subscribe_track_with_callback(broadcast_name, track_name)
                .await?;
        }
        Ok(())
    }

    /// Close the session and stop all operations
    pub async fn close_session(&self) -> Result<()> {
        info!("🔒 Closing MoQ session");

        // Send shutdown signal
        if let Err(e) = self.shutdown_tx.send(true) {
            warn!("Failed to send shutdown signal: {}", e);
        }

        // Clear session state
        {
            let mut state = self.state.write().await;
            state.connected = false;
            state.current_session = None;
            state.broadcast = None;
        }

        // Clear tracks and groups
        {
            self.tracks.write().await.clear();
            self.current_groups.write().await.clear();
            self.sequence_numbers.write().await.clear();
        }

        // Shutdown subscription manager
        {
            let mut subscription_manager = self.subscription_manager.write().await;
            if let Some(manager) = subscription_manager.as_ref() {
                if let Err(e) = manager.shutdown().await {
                    warn!("Failed to shutdown subscription manager: {}", e);
                }
            }
            *subscription_manager = None;
        }

        info!("Session closed successfully");
        Ok(())
    }

    /// Auto-subscribe to all requested tracks for subscriber sessions
    async fn auto_subscribe_to_tracks(&self) -> Result<()> {
        // Only auto-subscribe for subscriber sessions
        if !matches!(self.session_type, SessionType::Subscriber) {
            return Ok(());
        }

        // Get the list of requested tracks
        let requested_tracks = self.requested_tracks.read().await.clone();

        if requested_tracks.is_empty() {
            debug!("No tracks requested for auto-subscription");
            return Ok(());
        }

        info!(
            "Auto-subscribing to {} requested tracks",
            requested_tracks.len()
        );

        // Subscribe to each requested track with callback support
        for track_def in requested_tracks {
            match self
                .subscribe_track_with_callback(&self.broadcast_name, &track_def.name)
                .await
            {
                Ok(()) => {
                    info!("Auto-subscribed to track: {}", track_def.name);
                }
                Err(e) => {
                    warn!(
                        "⚠️ Failed to auto-subscribe to track {}: {}",
                        track_def.name, e
                    );
                }
            }
        }

        Ok(())
    }
}

/// Subscriber-specific functionality  
impl MoqSession {
    /// Subscribe to a broadcast (only available for subscriber sessions)
    pub async fn subscribe_broadcast(&self, broadcast_name: &str) -> Result<BroadcastConsumer> {
        debug!(
            "📻 [MoqSession] subscribe_broadcast called for: {}",
            broadcast_name
        );

        let state = self.state.read().await;

        let session_handle = state.current_session.as_ref().ok_or_else(|| {
            let err = WrapperError::Session("Not connected".to_string());
            error!(
                "❌ [MoqSession] subscribe_broadcast failed - session not connected: {}",
                err
            );
            err
        })?;

        if let SessionType::Subscriber = self.session_type {
            if let Some(origin_consumer) = &session_handle.origin_consumer {
                debug!(
                    "[MoqSession] Calling consume_broadcast on OriginConsumer for: {}",
                    broadcast_name
                );
                match origin_consumer.consume_broadcast(broadcast_name) {
                    Some(broadcast_consumer) => {
                        debug!(
                            "[MoqSession] Successfully consumed broadcast: {}",
                            broadcast_name
                        );
                        Ok(broadcast_consumer)
                    }
                    None => {
                        let err = WrapperError::BroadcastNotFound(broadcast_name.to_string());
                        debug!("⚠️ [MoqSession] Broadcast not found: {}", err);
                        Err(err.into())
                    }
                }
            } else {
                let err = WrapperError::Session("No origin consumer available".to_string());
                error!(
                    "❌ [MoqSession] subscribe_broadcast failed - no origin consumer: {}",
                    err
                );
                Err(err.into())
            }
        } else {
            let err = WrapperError::Session("Not a subscriber session".to_string());
            error!(
                "❌ [MoqSession] subscribe_broadcast failed - not a subscriber: {}",
                err
            );
            Err(err.into())
        }
    }

    /// Subscribe to a track with catalog validation and automatic reconnection
    pub async fn subscribe_track(
        &self,
        broadcast_name: &str,
        track_name: &str,
    ) -> Result<ResilientTrackConsumer> {
        if !matches!(self.session_type, SessionType::Subscriber) {
            return Err(WrapperError::Session("Not a subscriber session".to_string()).into());
        }

        let resilient_consumer = ResilientTrackConsumer::new(
            self.clone(),
            broadcast_name.to_string(),
            track_name.to_string(),
        )
        .await?;

        Ok(resilient_consumer)
    }

    /// Internal method to subscribe to a track without the resilient wrapper
    pub async fn subscribe_track_internal(
        &self,
        broadcast_name: &str,
        track_name: &str,
    ) -> Result<TrackConsumer> {
        debug!(
            "[MoqSession] subscribe_track_internal called for track: {} in broadcast: {}",
            track_name, broadcast_name
        );

        // Check if catalog validation is needed
        let catalog_type = self.catalog_type.read().await.clone();
        if catalog_type != CatalogType::None {
            debug!(
                "📋 [MoqSession] Catalog validation needed for track: {}",
                track_name
            );
            // First, validate against catalog
            self.validate_track_against_catalog(track_name).await?;
            debug!(
                "[MoqSession] Catalog validation passed for track: {}",
                track_name
            );
        }

        debug!(
            "[MoqSession] Calling subscribe_broadcast for: {}",
            broadcast_name
        );
        let broadcast = self.subscribe_broadcast(broadcast_name).await?;
        debug!(
            "[MoqSession] Successfully got broadcast: {}",
            broadcast_name
        );

        let track = Track {
            name: track_name.to_string(),
            priority: 0, // Priority doesn't matter for subscription
        };

        debug!(
            "🎵 [MoqSession] Calling subscribe_track on broadcast for track: {}",
            track_name
        );
        let track_consumer = broadcast.subscribe_track(&track);

        info!(
            "[MoqSession] Subscribed to track: {} in broadcast: {}",
            track_name, broadcast_name
        );
        Ok(track_consumer)
    }

    /// Validate that a track exists in the published catalog
    async fn validate_track_against_catalog(&self, track_name: &str) -> Result<()> {
        // If it's catalog.json itself, allow it
        if track_name == "catalog.json" {
            return Ok(());
        }

        let catalog_type = self.catalog_type.read().await.clone();
        if catalog_type == CatalogType::None {
            return Ok(()); // No validation needed
        }

        // Subscribe to catalog.json and parse it
        let catalog_json = self.fetch_catalog().await?;

        match catalog_type {
            CatalogType::Sesame => {
                let catalog = Catalog::parse_sesame(&catalog_json).map_err(|e| {
                    WrapperError::Session(format!("Failed to parse Sesame catalog: {}", e))
                })?;

                if catalog.find_track(track_name).is_none() {
                    return Err(WrapperError::TrackNotFound(format!(
                        "Track '{}' not found in published catalog",
                        track_name
                    ))
                    .into());
                }
            }
            CatalogType::Hang => {
                // TODO: Implement Hang catalog parsing when available
                warn!(
                    "Hang catalog validation not yet implemented, allowing track: {}",
                    track_name
                );
            }
            CatalogType::None => unreachable!(),
        }

        Ok(())
    }

    /// Fetch catalog.json from the broadcast
    async fn fetch_catalog(&self) -> Result<String> {
        // Try to subscribe to catalog.json track
        let broadcast = self
            .subscribe_broadcast(&self.config.broadcast_name)
            .await?;
        let catalog_track = Track {
            name: "catalog.json".to_string(),
            priority: 0,
        };

        let mut track_consumer = broadcast.subscribe_track(&catalog_track);

        // Read the first group/frame which should contain the catalog
        if let Some(mut group) = track_consumer.next_group().await? {
            if let Some(frame) = group.read_frame().await? {
                let catalog_json = String::from_utf8_lossy(&frame).to_string();
                info!("Fetched catalog: {} bytes", catalog_json.len());
                return Ok(catalog_json);
            }
        }

        Err(WrapperError::Session("No catalog data found".to_string()).into())
    }
}

/// A resilient wrapper around TrackConsumer that handles reconnections automatically
/// Simple event-driven approach: Connect -> Wait for announce -> Subscribe -> Unannounce/Error -> Close -> Repeat
pub struct ResilientTrackConsumer {
    session: MoqSession,
    broadcast_name: String,
    track_name: String,
    current_consumer: Arc<RwLock<Option<TrackConsumer>>>,
}

impl ResilientTrackConsumer {
    async fn new(session: MoqSession, broadcast_name: String, track_name: String) -> Result<Self> {
        let resilient = Self {
            session: session.clone(),
            broadcast_name: broadcast_name.clone(),
            track_name: track_name.clone(),
            current_consumer: Arc::new(RwLock::new(None)),
        };

        // Start the simple subscription manager
        resilient.start_subscription_manager().await;

        Ok(resilient)
    }

    /// Start the simple subscription manager that follows: Connect -> Announce -> Subscribe -> Unannounce/Error -> Close -> Repeat
    /// Also listens for BroadcastAnnounced events to immediately reset on broadcaster reconnections
    async fn start_subscription_manager(&self) {
        let session = self.session.clone();
        let broadcast_name = self.broadcast_name.clone();
        let track_name = self.track_name.clone();
        let current_consumer = self.current_consumer.clone();

        // Start the subscription management task
        let subscription_task = {
            let session = session.clone();
            let broadcast_name = broadcast_name.clone();
            let track_name = track_name.clone();
            let current_consumer = current_consumer.clone();

            tokio::spawn(async move {
                info!(
                    "[ResilientTrackConsumer] Starting subscription manager for broadcast: {}",
                    broadcast_name
                );

                loop {
                    // Step 1: Wait for session to be connected
                    while !session.is_connected().await {
                        debug!("[ResilientTrackConsumer] Waiting for session connection...");
                        sleep(Duration::from_millis(100)).await;
                    }

                    // Step 2: Check if we have a consumer
                    let has_consumer = {
                        let consumer_guard = current_consumer.read().await;
                        consumer_guard.is_some()
                    };

                    if !has_consumer {
                        // Step 3: Try to subscribe when we don't have a consumer
                        match session
                            .subscribe_track_internal(&broadcast_name, &track_name)
                            .await
                        {
                            Ok(consumer) => {
                                info!(
                                    "[ResilientTrackConsumer] Successfully subscribed to track: {}",
                                    track_name
                                );
                                *current_consumer.write().await = Some(consumer);
                            }
                            Err(e) => {
                                debug!(
                                    "[ResilientTrackConsumer] Subscription failed (will retry): {}",
                                    e
                                );
                            }
                        }
                    }

                    // Step 4: Sleep before checking again
                    sleep(Duration::from_millis(1000)).await;
                }
            })
        };

        // Start the broadcast announcement listener task
        let announcement_task = {
            let session = session.clone();
            let broadcast_name = broadcast_name.clone();
            let current_consumer = current_consumer.clone();

            tokio::spawn(async move {
                info!(
                    "[ResilientTrackConsumer] Starting broadcast announcement listener for: {}",
                    broadcast_name
                );

                let mut announcement_rx = session.subscribe_announcements();

                loop {
                    match announcement_rx.recv().await {
                        Ok(announced_broadcast) => {
                            if announced_broadcast == broadcast_name {
                                info!("[ResilientTrackConsumer] Broadcast '{}' announced - resetting consumer for fresh connection", broadcast_name);

                                // Immediately clear the current consumer to force a new subscription
                                *current_consumer.write().await = None;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(skipped)) => {
                            warn!("[ResilientTrackConsumer] Announcement listener lagged, skipped {} messages", skipped);
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            debug!("[ResilientTrackConsumer] Announcement channel closed, exiting listener");
                            break;
                        }
                    }
                }
            })
        };

        // Don't wait for tasks to complete - they run indefinitely
        // Just spawn them and return
        std::mem::drop(subscription_task);
        std::mem::drop(announcement_task);
    }

    /// Get the next group, with automatic error handling that clears the consumer on errors
    pub async fn next_group(&self) -> Result<Option<moq_lite::GroupConsumer>> {
        loop {
            // Check if we have a consumer and call next_group directly without cloning
            let mut consumer_guard = self.current_consumer.write().await;

            if let Some(ref mut consumer) = *consumer_guard {
                match consumer.next_group().await {
                    Ok(Some(group)) => {
                        // Successfully got a group - consumer has advanced internally
                        return Ok(Some(group));
                    }
                    Ok(None) => {
                        // Stream ended normally - clear consumer and wait for reconnection
                        warn!("[ResilientTrackConsumer] Track stream ended (Ok(None)), clearing consumer");
                        *consumer_guard = None;
                        drop(consumer_guard); // Release lock before sleeping
                                              // Sleep briefly before retrying
                        sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                    Err(e) => {
                        // Error occurred - clear consumer and wait for reconnection
                        warn!(
                            "[ResilientTrackConsumer] Track stream error: {}, clearing consumer",
                            e
                        );
                        *consumer_guard = None;
                        drop(consumer_guard); // Release lock before sleeping
                                              // Sleep briefly before retrying
                        sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                }
            } else {
                drop(consumer_guard); // Release lock before sleeping
                                      // No consumer available, wait for the subscription manager to create one
                sleep(Duration::from_millis(100)).await;
                continue;
            }
        }
    }
}
