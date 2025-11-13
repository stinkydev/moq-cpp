use anyhow::{Context, Result};
use bytes::Bytes;
use rand::Rng;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, watch, RwLock};
use tokio::time::Instant;
use tracing::{debug, error, info, warn, Level};

use moq_lite::{
    Broadcast, BroadcastConsumer, BroadcastProducer, GroupProducer, Origin, OriginConsumer,
    OriginProducer, Session, Track, TrackConsumer, TrackProducer,
};
use moq_native::Client;

use crate::catalog::{Catalog, CatalogType, TrackDefinition};
use crate::config::{SessionConfig, WrapperError};

/// Log callback function type for session-specific logging
pub type SessionLogCallback = Box<dyn Fn(&str, Level, &str) + Send + Sync>;

/// Type alias for data callback function
pub type DataCallback = Arc<dyn Fn(String, Vec<u8>) + Send + Sync>;

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

/// Callback function types for session events
pub type BroadcastAnnouncedCallback = Box<dyn Fn(&str) + Send + Sync>;
pub type BroadcastCancelledCallback = Box<dyn Fn(&str) + Send + Sync>;
pub type ConnectionClosedCallback = Box<dyn Fn(&str) + Send + Sync>;

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

    // Internal broadcast channel for announcement events (for BroadcastSubscriptionManager)
    announcement_tx: broadcast::Sender<String>,

    // Subscription management
    broadcast_subscription_manager:
        Arc<RwLock<Option<crate::subscription_manager::BroadcastSubscriptionManager>>>,

    // Shutdown signal
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,

    // Session logging
    log_callback: Arc<RwLock<Option<SessionLogCallback>>>,

    // Event callbacks
    broadcast_announced_callback: Arc<RwLock<Option<BroadcastAnnouncedCallback>>>,
    broadcast_cancelled_callback: Arc<RwLock<Option<BroadcastCancelledCallback>>>,
    connection_closed_callback: Arc<RwLock<Option<ConnectionClosedCallback>>>,

    // Catalog management is now handled by BroadcastSubscriptionManager
}

#[derive(Clone)]
struct SessionState {
    connected: bool,
    connection_attempts: usize,
    last_connection_time: Option<Instant>,
    current_session: Option<SessionHandle>,
    broadcast: Option<BroadcastHandle>,
    // Cache the broadcast consumer to avoid multiple calls to subscribe_broadcast
    broadcast_consumer_cache: Option<(String, BroadcastConsumer)>,
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
            broadcast_consumer_cache: None,
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
            broadcast_subscription_manager: Arc::new(RwLock::new(None)),
            shutdown_tx,
            shutdown_rx,
            log_callback: Arc::new(RwLock::new(None)),
            broadcast_announced_callback: Arc::new(RwLock::new(None)),
            broadcast_cancelled_callback: Arc::new(RwLock::new(None)),
            connection_closed_callback: Arc::new(RwLock::new(None)),

        };

        Ok(session)
    }

    /// Start the session and connect once (no reconnection logic)
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

        // Get callback references for announcements
        let broadcast_announced_cb = self.broadcast_announced_callback.clone();
        let broadcast_cancelled_cb = self.broadcast_cancelled_callback.clone();
        let connection_closed_cb = self.connection_closed_callback.clone();

        tokio::spawn(async move {
            // Check for shutdown signal before connecting
            if *shutdown_rx.borrow() {
                info!("Shutdown signal received before connection, stopping session");
                return;
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

                    // Update connection state
                    {
                        let mut state_guard = state.write().await;
                        state_guard.connected = true;
                        state_guard.connection_attempts = 1;
                        state_guard.last_connection_time = Some(Instant::now());
                        state_guard.current_session = Some(session_handle.clone());
                    }

                    // Create track producers for publisher sessions
                    if matches!(session_type, SessionType::Publisher) {
                        let session_for_tracks = session_clone.clone();
                        if let Err(e) = session_for_tracks.create_track_producers().await {
                            warn!("Failed to create track producers: {}", e);
                            let _ = event_tx.send(SessionEvent::Error {
                                error: format!("Failed to create track producers: {}", e),
                            });
                        } else {
                            info!("Successfully created track producers");
                            let _ = event_tx.send(SessionEvent::Connected);
                        }
                    } else {
                        // For subscribers, send Connected immediately and setup monitoring with callbacks
                        let _ = event_tx.send(SessionEvent::Connected);

                        // Setup announcement monitoring with callbacks for subscribers
                        if let Some(origin_consumer) = &session_handle.origin_consumer {
                            Self::monitor_announcements(
                                origin_consumer.clone(),
                                event_tx.clone(),
                                announcement_tx.clone(),
                                broadcast_announced_cb.clone(),
                                broadcast_cancelled_cb.clone(),
                            )
                            .await;
                        }
                    }

                    // Auto-subscription is now handled by BroadcastSubscriptionManager
                    // Users should call enable_auto_subscription() to set up automatic catalog and track management

                    // Wait for session to close or shutdown signal
                    let disconnect_reason = tokio::select! {
                        result = session_handle.session.closed() => {
                            match result {
                                Ok(()) => {
                                    info!("Session closed normally");
                                    "Session closed normally".to_string()
                                }
                                Err(e) => {
                                    error!("Session closed with error: {}", e);
                                    format!("Session error: {}", e)
                                }
                            }
                        }
                        _ = shutdown_rx.changed() => {
                            if *shutdown_rx.borrow() {
                                info!("Shutdown requested, closing session");
                                "Shutdown requested".to_string()
                            } else {
                                "Unknown shutdown reason".to_string()
                            }
                        }
                    };

                    // Call connection closed callback if set
                    let callback_guard = connection_closed_cb.read().await;
                    if let Some(callback) = callback_guard.as_ref() {
                        callback(&disconnect_reason);
                    }
                    drop(callback_guard);

                    // Send disconnected event
                    let _ = event_tx.send(SessionEvent::Disconnected {
                        reason: disconnect_reason,
                    });

                    // Mark as disconnected and clean up session state
                    {
                        let mut state_guard = state.write().await;
                        state_guard.connected = false;
                        state_guard.current_session = None;
                        state_guard.broadcast = None;
                        state_guard.broadcast_consumer_cache = None;
                    }

                    // Clear session state
                    session_clone.current_groups.write().await.clear();
                    *session_clone.catalog_published.write().await = false;

                    info!("Session closed and cleaned up");
                }
                Err(e) => {
                    let mut state_guard = state.write().await;
                    state_guard.connected = false;
                    state_guard.connection_attempts = 1;
                    state_guard.current_session = None;

                    error!("Failed to establish connection: {}", e);

                    // Call connection closed callback if set
                    let callback_guard = connection_closed_cb.read().await;
                    if let Some(callback) = callback_guard.as_ref() {
                        callback(&format!("Connection failed: {}", e));
                    }
                    drop(callback_guard);

                    let _ = event_tx.send(SessionEvent::Error {
                        error: format!("Connection failed: {}", e),
                    });

                    drop(state_guard);
                }
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
                // We can't pass the session here due to circular dependencies, so callbacks won't work in establish_connection
                // This will be handled differently - callbacks will be set up in the start() method
                Self::monitor_announcements_simple(
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

    /// Set up broadcast monitoring with callbacks (called from start method with full session access)
    async fn monitor_announcements(
        mut origin_consumer: OriginConsumer,
        event_tx: mpsc::UnboundedSender<SessionEvent>,
        announcement_tx: broadcast::Sender<String>,
        broadcast_announced_cb: Arc<RwLock<Option<BroadcastAnnouncedCallback>>>,
        broadcast_cancelled_cb: Arc<RwLock<Option<BroadcastCancelledCallback>>>,
    ) {
        tokio::spawn(async move {
            while let Some((path, broadcast)) = origin_consumer.announced().await {
                match broadcast {
                    Some(_) => {
                        debug!("Broadcast announced: {}", path);
                        let _ = event_tx.send(SessionEvent::BroadcastAnnounced {
                            path: path.to_string(),
                        });
                        // Also send to internal broadcast channel for BroadcastSubscriptionManager
                        let _ = announcement_tx.send(path.to_string());

                        // Call the broadcast announced callback if set
                        let callback_guard = broadcast_announced_cb.read().await;
                        if let Some(callback) = callback_guard.as_ref() {
                            callback(path.as_ref());
                        }
                    }
                    None => {
                        debug!("Broadcast unannounced: {}", path);
                        let _ = event_tx.send(SessionEvent::BroadcastUnannounced {
                            path: path.to_string(),
                        });

                        // Catalog cache clearing is now handled by BroadcastSubscriptionManager

                        // Call the broadcast cancelled callback if set
                        let callback_guard = broadcast_cancelled_cb.read().await;
                        if let Some(callback) = callback_guard.as_ref() {
                            callback(path.as_ref());
                        }
                    }
                }
            }
        });
    }

    /// Simple broadcast monitoring without callbacks (used during connection establishment)
    async fn monitor_announcements_simple(
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
                        // Also send to internal broadcast channel for BroadcastSubscriptionManager
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

    /// Set callback for when a broadcast is announced as active
    pub async fn set_broadcast_announced_callback(&self, callback: BroadcastAnnouncedCallback) {
        *self.broadcast_announced_callback.write().await = Some(callback);
    }

    /// Set callback for when a broadcast is cancelled or connection is closed
    pub async fn set_broadcast_cancelled_callback(&self, callback: BroadcastCancelledCallback) {
        *self.broadcast_cancelled_callback.write().await = Some(callback);
    }

    /// Set callback for when connection is closed
    pub async fn set_connection_closed_callback(&self, callback: ConnectionClosedCallback) {
        *self.connection_closed_callback.write().await = Some(callback);
    }

    // clear_catalog_cache method removed - catalog caching is now handled by BroadcastSubscriptionManager

    /// Create a BroadcastSubscriptionManager for a specific broadcast
    /// This is the recommended way to subscribe to broadcasts with catalog support
    pub async fn create_subscription_manager(
        &self,
        broadcast_name: String,
        catalog_type: CatalogType,
        requested_tracks: Vec<TrackDefinition>,
    ) -> Result<crate::subscription_manager::BroadcastSubscriptionManager> {
        crate::subscription_manager::BroadcastSubscriptionManager::new(
            self.clone(),
            broadcast_name,
            catalog_type,
            requested_tracks,
        )
        .await
    }

    /// Enable automatic subscription management for this session
    /// This will create a BroadcastSubscriptionManager internally and manage all subscriptions
    /// Use this for simple scenarios where you want automatic catalog and track handling
    pub async fn enable_auto_subscription(
        &self,
        broadcast_name: String,
        catalog_type: CatalogType,
        requested_tracks: Vec<TrackDefinition>,
    ) -> Result<()> {
        // Check if already enabled
        {
            let manager_guard = self.broadcast_subscription_manager.read().await;
            if manager_guard.is_some() {
                info!("Automatic subscription management already enabled - ignoring duplicate call");
                return Ok(());
            }
        }

        let manager = self
            .create_subscription_manager(broadcast_name, catalog_type, requested_tracks)
            .await?;

        *self.broadcast_subscription_manager.write().await = Some(manager);
        info!("Enabled automatic subscription management");
        Ok(())
    }

    /// Set data callback for the internal subscription manager
    /// Only works if enable_auto_subscription was called first
    pub async fn set_auto_subscription_data_callback<F>(&self, callback: F) -> Result<()>
    where
        F: Fn(String, Vec<u8>) + Send + Sync + 'static,
    {
        if let Some(manager) = self.broadcast_subscription_manager.read().await.as_ref() {
            manager.set_data_callback(callback).await;
            Ok(())
        } else {
            Err(anyhow::anyhow!("Automatic subscription management not enabled. Call enable_auto_subscription first."))
        }
    }

    /// Get current catalog from the internal subscription manager
    /// Only works if enable_auto_subscription was called first
    pub async fn get_auto_subscription_catalog(&self) -> Option<Catalog> {
        if let Some(manager) = self.broadcast_subscription_manager.read().await.as_ref() {
            manager.get_catalog().await
        } else {
            None
        }
    }

    /// Get active tracks from the internal subscription manager
    /// Only works if enable_auto_subscription was called first
    pub async fn get_auto_subscription_active_tracks(&self) -> Vec<String> {
        if let Some(manager) = self.broadcast_subscription_manager.read().await.as_ref() {
            manager.get_active_tracks().await
        } else {
            Vec::new()
        }
    }

    /// Check if auto subscription is active
    pub async fn is_auto_subscription_active(&self) -> bool {
        if let Some(manager) = self.broadcast_subscription_manager.read().await.as_ref() {
            manager.is_active().await
        } else {
            false
        }
    }

    /// Disable automatic subscription management and stop all subscriptions
    pub async fn disable_auto_subscription(&self) {
        if let Some(manager) = self.broadcast_subscription_manager.write().await.take() {
            manager.stop().await;
            info!("Disabled automatic subscription management");
        }
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
    /// This is an alias for set_auto_subscription_data_callback for backward compatibility
    pub async fn set_data_callback<F>(&self, callback: F) -> Result<()>
    where
        F: Fn(String, Vec<u8>) + Send + Sync + 'static,
    {
        self.set_auto_subscription_data_callback(callback).await
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
            state.broadcast_consumer_cache = None;
        }

        // Clear tracks and groups
        {
            self.tracks.write().await.clear();
            self.current_groups.write().await.clear();
            self.sequence_numbers.write().await.clear();
        }

        // Shutdown broadcast subscription manager
        self.disable_auto_subscription().await;

        info!("Session closed successfully");
        Ok(())
    }
}

/// Subscriber-specific functionality  
impl MoqSession {
    /// Subscribe to a broadcast (only available for subscriber sessions)
    pub async fn subscribe_broadcast(&self, broadcast_name: &str) -> Result<BroadcastConsumer> {
        warn!(
            "� [MoqSession] CRITICAL: subscribe_broadcast called for: '{}' - THIS SHOULD ONLY HAPPEN ONCE PER SESSION!",
            broadcast_name
        );

        // First check if we have a cached broadcast consumer for this broadcast
        {
            let state = self.state.read().await;
            if let Some((cached_name, cached_consumer)) = &state.broadcast_consumer_cache {
                if cached_name == broadcast_name {
                    info!(
                        "[MoqSession] Using cached broadcast consumer for: '{}'",
                        broadcast_name
                    );
                    return Ok(cached_consumer.clone());
                }
            }
        }

        info!(
            "[MoqSession] FIRST-TIME broadcast subscription for: '{}' - calling consume_broadcast",
            broadcast_name
        );

        // Need to create a new broadcast consumer
        let mut state = self.state.write().await;

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
                        info!(
                            "[MoqSession] Successfully consumed broadcast: '{}' - caching for reuse",
                            broadcast_name
                        );
                        
                        // Cache the broadcast consumer for future use
                        state.broadcast_consumer_cache = Some((broadcast_name.to_string(), broadcast_consumer.clone()));
                        
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

    /// Internal method to subscribe to a track without the resilient wrapper
    pub async fn subscribe_track_internal(
        &self,
        broadcast_name: &str,
        track_name: &str,
    ) -> Result<TrackConsumer> {
        info!(
            "🎵 [MoqSession] TRACK SUBSCRIPTION REQUEST: track '{}' in broadcast '{}'",
            track_name, broadcast_name
        );

        // Catalog validation is now handled by BroadcastSubscriptionManager
        // No need to validate here as the manager handles catalog and track coordination

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

    // Removed validate_track_against_catalog method - catalog validation is now handled by BroadcastSubscriptionManager

    // Removed fetch_catalog and fetch_catalog_internal methods - catalog fetching is now handled by BroadcastSubscriptionManager
}
