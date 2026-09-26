use anyhow::{Context, Result};
use bytes::Bytes;
use rand::Rng;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, watch, RwLock};
use tokio::time::Instant;
use tracing::{debug, info, warn, Level};

use moq_native::moq_net::{
    self, announce as moq_announce, broadcast as moq_broadcast, group as moq_group,
    origin as moq_origin, track as moq_track, Origin, Timestamp,
};
use moq_native::{Client, Reconnect, Status as ReconnectStatus};

type BroadcastConsumer = moq_broadcast::Consumer;
type BroadcastProducer = moq_broadcast::Producer;
type GroupProducer = moq_group::Producer;
type OriginConsumer = moq_origin::Consumer;
type OriginProducer = moq_origin::Producer;
type TrackInfo = moq_track::Info;
type TrackConsumer = moq_track::Subscriber;
type TrackProducer = moq_track::Producer;

use crate::catalog::{Catalog, CatalogType, TrackDefinition};
use crate::config::{SessionConfig, WrapperError};

/// Log callback function type for session-specific logging
pub type SessionLogCallback = Box<dyn Fn(&str, Level, &str) + Send + Sync>;

/// Type alias for data callback function
pub type DataCallback = Arc<dyn Fn(String, Vec<u8>) + Send + Sync>;

/// Type alias for optional data callback stored in session
pub type OptionalDataCallback = Arc<RwLock<Option<DataCallback>>>;

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubscriptionMode {
    ExactBroadcast,
    RoomPrefix,
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

/// Resolves once `true` is sent on the shutdown channel or its sender is gone.
async fn shutdown_requested(rx: &mut watch::Receiver<bool>) {
    let _ = rx.wait_for(|stop| *stop).await;
}

/// Callback function types for session events
pub type BroadcastAnnouncedCallback = Box<dyn Fn(&str) + Send + Sync>;
pub type BroadcastCancelledCallback = Box<dyn Fn(&str) + Send + Sync>;
pub type ConnectionClosedCallback = Box<dyn Fn(&str) + Send + Sync>;

/// A high-level wrapper around moq-native that provides:
/// - Automatic reconnection for both publish and subscribe sessions: the origin,
///   the published broadcast and its tracks live as long as the session, and
///   moq-native's reconnect loop attaches every new relay connection to them
/// - Session lifecycle management
/// - Event notifications
/// - Easy-to-use API for publishing and subscribing with direct frame operations
#[derive(Clone)]
pub struct MoqSession {
    config: SessionConfig,
    session_type: SessionType,
    client: Client,
    broadcast_name: String, // Store the broadcast name for publishers
    subscription_mode: SubscriptionMode,

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
    subscribe_all_catalog_tracks: Arc<RwLock<bool>>,

    // Event notification
    event_tx: mpsc::UnboundedSender<SessionEvent>,
    event_rx: Arc<RwLock<Option<mpsc::UnboundedReceiver<SessionEvent>>>>,

    // Internal broadcast channel for announcement events (for BroadcastSubscriptionManager)
    announcement_tx: broadcast::Sender<String>,

    // Subscription management
    broadcast_subscription_managers:
        Arc<RwLock<HashMap<String, crate::subscription_manager::BroadcastSubscriptionManager>>>,

    // Shutdown signal
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,

    // Session logging
    log_callback: Arc<RwLock<Option<SessionLogCallback>>>,

    // Event callbacks
    broadcast_announced_callback: Arc<RwLock<Option<BroadcastAnnouncedCallback>>>,
    broadcast_cancelled_callback: Arc<RwLock<Option<BroadcastCancelledCallback>>>,
    connection_closed_callback: Arc<RwLock<Option<ConnectionClosedCallback>>>,

    // Data callback for BroadcastSubscriptionManager
    data_callback: OptionalDataCallback,
    // Catalog management is now handled by BroadcastSubscriptionManager
}

#[derive(Clone)]
struct SessionState {
    connected: bool,
    /// Relay connections made so far, the first one and every reconnect.
    connection_attempts: usize,
    last_connection_time: Option<Instant>,
    /// Why the reconnect loop stopped for good, once it has.
    last_error: Option<String>,
    /// The origin every relay connection of this session publishes or subscribes through.
    origin: Option<OriginProducer>,
    /// Subscribers: the consumer side of the origin, for requesting broadcasts.
    origin_consumer: Option<OriginConsumer>,
    broadcast: Option<BroadcastHandle>,
    // Store broadcast consumers for subscribers, keyed by broadcast path.
    broadcast_consumers: HashMap<String, BroadcastConsumer>,
}

#[derive(Clone)]
struct TrackHandle {
    producer: Option<TrackProducer>,
    track_info: TrackInfo,
    #[allow(dead_code)]
    track_definition: Option<TrackDefinition>,
}

#[derive(Clone)]
struct BroadcastHandle {
    producer: Option<BroadcastProducer>,
}

impl MoqSession {
    /// Create a new publisher session
    pub async fn publisher(
        config: SessionConfig,
        broadcast_name: String,
        catalog_type: CatalogType,
        tracks: Vec<TrackDefinition>,
    ) -> Result<Self> {
        Self::new(
            config,
            SessionType::Publisher,
            broadcast_name,
            SubscriptionMode::ExactBroadcast,
            catalog_type,
            tracks,
            false,
        )
        .await
    }

    /// Create a new subscriber session
    pub async fn subscriber(
        config: SessionConfig,
        broadcast_name: String,
        catalog_type: CatalogType,
        tracks: Vec<TrackDefinition>,
    ) -> Result<Self> {
        Self::subscriber_with_options(config, broadcast_name, catalog_type, tracks, false).await
    }

    /// Create a new subscriber session with subscription options.
    pub async fn subscriber_with_options(
        config: SessionConfig,
        broadcast_name: String,
        catalog_type: CatalogType,
        tracks: Vec<TrackDefinition>,
        subscribe_all_catalog_tracks: bool,
    ) -> Result<Self> {
        Self::new(
            config,
            SessionType::Subscriber,
            broadcast_name,
            SubscriptionMode::ExactBroadcast,
            catalog_type,
            tracks,
            subscribe_all_catalog_tracks,
        )
        .await
    }

    /// Create a new room subscriber session.
    ///
    /// A room subscriber treats `room_prefix` as an announcement prefix and
    /// subscribes to the configured tracks on each matching announced broadcast.
    pub async fn room_subscriber(
        config: SessionConfig,
        room_prefix: String,
        catalog_type: CatalogType,
        tracks: Vec<TrackDefinition>,
    ) -> Result<Self> {
        Self::room_subscriber_with_options(config, room_prefix, catalog_type, tracks, false).await
    }

    /// Create a new room subscriber session with subscription options.
    pub async fn room_subscriber_with_options(
        config: SessionConfig,
        room_prefix: String,
        catalog_type: CatalogType,
        tracks: Vec<TrackDefinition>,
        subscribe_all_catalog_tracks: bool,
    ) -> Result<Self> {
        Self::new(
            config,
            SessionType::Subscriber,
            room_prefix,
            SubscriptionMode::RoomPrefix,
            catalog_type,
            tracks,
            subscribe_all_catalog_tracks,
        )
        .await
    }

    async fn new(
        config: SessionConfig,
        session_type: SessionType,
        broadcast_name: String,
        subscription_mode: SubscriptionMode,
        catalog_type: CatalogType,
        tracks: Vec<TrackDefinition>,
        subscribe_all_catalog_tracks: bool,
    ) -> Result<Self> {
        let mut client_config = config.connection.resolved_client_config()?;

        // Force IPv4 binding on Windows to avoid IPv6 issues
        #[cfg(windows)]
        {
            client_config.bind = "0.0.0.0:0"
                .parse()
                .context("Failed to parse IPv4 bind address")?;
        }

        // Also respect the explicit ipv4_only flag
        if config.connection.ipv4_only {
            client_config.bind = "0.0.0.0:0"
                .parse()
                .context("Failed to parse IPv4 bind address")?;
        }

        let client = crate::runtime::shared_client(&client_config)?;

        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (announcement_tx, _) = broadcast::channel(100); // Buffer up to 100 announcements

        let state = Arc::new(RwLock::new(SessionState {
            connected: false,
            connection_attempts: 0,
            last_connection_time: None,
            last_error: None,
            origin: None,
            origin_consumer: None,
            broadcast: None,
            broadcast_consumers: HashMap::new(),
        }));

        let session = Self {
            config,
            session_type: session_type.clone(),
            client,
            broadcast_name,
            subscription_mode,
            state,
            tracks: Arc::new(RwLock::new(HashMap::new())),
            current_groups: Arc::new(RwLock::new(HashMap::new())),
            sequence_numbers: Arc::new(RwLock::new(HashMap::new())),
            catalog: Arc::new(RwLock::new(None)),
            catalog_type: Arc::new(RwLock::new(catalog_type.clone())),
            catalog_published: Arc::new(RwLock::new(false)),
            requested_tracks: Arc::new(RwLock::new(Vec::new())),
            subscribe_all_catalog_tracks: Arc::new(RwLock::new(subscribe_all_catalog_tracks)),
            event_tx,
            event_rx: Arc::new(RwLock::new(Some(event_rx))),
            announcement_tx,
            broadcast_subscription_managers: Arc::new(RwLock::new(HashMap::new())),
            shutdown_tx,
            shutdown_rx,
            log_callback: Arc::new(RwLock::new(None)),
            broadcast_announced_callback: Arc::new(RwLock::new(None)),
            broadcast_cancelled_callback: Arc::new(RwLock::new(None)),
            connection_closed_callback: Arc::new(RwLock::new(None)),
            data_callback: Arc::new(RwLock::new(None)),
        };

        // loop through tracks and add
        for track_def in tracks.iter() {
            session.add_track_definition(track_def.clone()).await?;
        }

        // Set catalog if needed (only for publishers)
        if matches!(session_type, SessionType::Publisher) && catalog_type != CatalogType::None {
            let catalog = Catalog::new(catalog_type.clone(), &tracks)
                .ok_or_else(|| anyhow::anyhow!("Failed to create catalog"))?;
            session.set_catalog(catalog).await?;
        }

        Ok(session)
    }

    /// Start the session. The origin, and for a publisher its broadcast, tracks
    /// and catalog, are set up once; a background loop then keeps a relay
    /// connection up for as long as the session lives, reconnecting with backoff
    /// after a drop. Each new connection publishes or subscribes the same origin,
    /// so tracks, catalog and subscriptions carry over.
    pub async fn start(&self) -> Result<()> {
        session_log!(self, info, "Starting MoQ session: {:?}", self.session_type);
        if *self.shutdown_rx.borrow() {
            info!("Shutdown signal received before start, not connecting");
            return Ok(());
        }

        let (client, announcement_consumer) = self.prepare_origin().await?;
        if matches!(self.session_type, SessionType::Publisher) {
            self.create_track_producers().await?;
        }

        let reconnect = client.reconnect(self.config.connection.url.clone());

        if matches!(self.session_type, SessionType::Subscriber) {
            Self::monitor_announcements(
                announcement_consumer,
                self.event_tx.clone(),
                self.announcement_tx.clone(),
                self.broadcast_announced_callback.clone(),
                self.broadcast_cancelled_callback.clone(),
                self.clone(),
            )
            .await;
        }

        let session = self.clone();
        tokio::spawn(async move { session.supervise(reconnect).await });
        Ok(())
    }

    /// Follows the reconnect loop's status into the session state until the loop
    /// gives up or the session shuts down. Dropping the handle stops the loop.
    async fn supervise(&self, mut reconnect: Reconnect) {
        let mut shutdown_rx = self.shutdown_rx.clone();
        let closed_reason = loop {
            let status = tokio::select! {
                status = reconnect.status() => status,
                _ = shutdown_requested(&mut shutdown_rx) => {
                    info!("Shutdown requested, closing session");
                    break "Shutdown requested".to_string();
                }
            };
            match status {
                Ok(ReconnectStatus::Connected) => {
                    {
                        let mut state = self.state.write().await;
                        state.connected = true;
                        state.connection_attempts += 1;
                        state.last_connection_time = Some(Instant::now());
                    }
                    info!("Connected to {}", self.config.connection.url);
                    let _ = self.event_tx.send(SessionEvent::Connected);
                }
                Ok(ReconnectStatus::Disconnected) => {
                    self.state.write().await.connected = false;
                    warn!(
                        "Connection to {} dropped, reconnecting",
                        self.config.connection.url
                    );
                    let _ = self.event_tx.send(SessionEvent::Disconnected {
                        reason: "Connection dropped, reconnecting".to_string(),
                    });
                }
                Ok(_) => {}
                Err(e) => {
                    let reason = format!("Reconnection gave up: {}", e);
                    warn!("{}", reason);
                    self.state.write().await.last_error = Some(reason.clone());
                    let _ = self.event_tx.send(SessionEvent::Error {
                        error: reason.clone(),
                    });
                    break reason;
                }
            }
        };
        drop(reconnect);

        let was_connected = {
            let mut state = self.state.write().await;
            let was_connected = state.connected;
            state.connected = false;
            was_connected
        };
        let _ = self.event_tx.send(SessionEvent::Disconnected {
            reason: closed_reason.clone(),
        });

        // The connection is gone for good: a loop that gave up, or a shutdown of a
        // connected session. A drop the loop recovers from is only seen through
        // is_connected().
        let shutdown = *self.shutdown_rx.borrow();
        if !shutdown || was_connected {
            let callback_guard = self.connection_closed_callback.read().await;
            if let Some(callback) = callback_guard.as_ref() {
                callback(&closed_reason);
            }
        }
        debug!("Session management task terminated");
    }

    /// Creates the origin every relay connection of this session goes through,
    /// and the client that attaches connections to it. A publisher's broadcast is
    /// created here once; a subscriber's broadcasts linger across a drop for
    /// `broadcast_linger`.
    async fn prepare_origin(&self) -> Result<(Client, moq_announce::Consumer)> {
        debug!("Preparing origin for: {}", self.config.connection.url);
        let mut state = self.state.write().await;

        match self.session_type {
            SessionType::Publisher => {
                let origin = Origin::random().produce();
                let broadcast_producer = origin
                    .create_broadcast(
                        self.broadcast_name.as_str(),
                        moq_broadcast::Route::announced(),
                    )
                    .context("Failed to create announced broadcast")?;
                state.broadcast = Some(BroadcastHandle {
                    producer: Some(broadcast_producer),
                });
                let client = self.client.clone().with_publisher(&origin);
                let announcements = origin.consume().announced();
                state.origin = Some(origin);
                Ok((client, announcements))
            }
            SessionType::Subscriber => {
                let origin = Origin::random()
                    .produce()
                    .with_linger(self.config.connection.broadcast_linger);
                let scoped_origin = if self.broadcast_name.is_empty() {
                    origin.clone()
                } else {
                    let path: moq_net::Path<'_> = self.broadcast_name.as_str().into();
                    origin.scope(&[path]).ok_or_else(|| {
                        WrapperError::Session(format!(
                            "Unable to subscribe to broadcast prefix '{}'",
                            self.broadcast_name
                        ))
                    })?
                };
                let origin_consumer = scoped_origin.consume();
                let announcements = origin_consumer.announced();
                let client = self.client.clone().with_subscriber(scoped_origin);
                state.origin_consumer = Some(origin_consumer);
                state.origin = Some(origin);
                Ok((client, announcements))
            }
        }
    }

    /// Set up broadcast monitoring with callbacks (called from start method with full session access)
    async fn monitor_announcements(
        mut announcement_consumer: moq_announce::Consumer,
        event_tx: mpsc::UnboundedSender<SessionEvent>,
        announcement_tx: broadcast::Sender<String>,
        broadcast_announced_cb: Arc<RwLock<Option<BroadcastAnnouncedCallback>>>,
        broadcast_cancelled_cb: Arc<RwLock<Option<BroadcastCancelledCallback>>>,
        session: MoqSession, // Add session reference to handle BroadcastSubscriptionManager lifecycle
    ) {
        let mut shutdown_rx = session.shutdown_rx.clone();
        tokio::spawn(async move {
            loop {
                let update = tokio::select! {
                    update = announcement_consumer.next() => match update {
                        Some(update) => update,
                        None => break,
                    },
                    _ = shutdown_requested(&mut shutdown_rx) => break,
                };
                let path = update.path.to_string();
                match update.broadcast {
                    Some(broadcast_consumer) => {
                        let _ =
                            event_tx.send(SessionEvent::BroadcastAnnounced { path: path.clone() });
                        // Also send to internal broadcast channel for BroadcastSubscriptionManager
                        let _ = announcement_tx.send(path.clone());

                        // Call the broadcast announced callback before any subscription
                        // manager is started, so the announce is guaranteed to be observed
                        // before the first media frame from this broadcast is delivered.
                        {
                            let callback_guard = broadcast_announced_cb.read().await;
                            if let Some(callback) = callback_guard.as_ref() {
                                callback(&path);
                            }
                        }

                        // Handle announcements for exact broadcasts or room prefixes.
                        if session.should_subscribe_to_announcement(&path) {
                            session
                                .state
                                .write()
                                .await
                                .broadcast_consumers
                                .insert(path.clone(), broadcast_consumer);
                            let _ = session.create_or_recreate_manager_for(path.clone()).await;
                        }
                    }
                    None => {
                        debug!("Broadcast unannounced: {}", path);
                        let _ = event_tx
                            .send(SessionEvent::BroadcastUnannounced { path: path.clone() });

                        // Stop and join the subscription manager first so no track task can
                        // deliver a frame after the cancelled callback has fired.
                        session.remove_subscription_manager(&path).await;

                        // Call the broadcast cancelled callback if set
                        let callback_guard = broadcast_cancelled_cb.read().await;
                        if let Some(callback) = callback_guard.as_ref() {
                            callback(&path);
                        }
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
            last_error: state.last_error.clone(),
        }
    }

    /// Stop the session and close all connections
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping MoQ session");
        self.request_shutdown();
        Ok(())
    }

    /// Signal every task spawned for this session to stop. Does not block.
    pub fn request_shutdown(&self) {
        self.shutdown_tx.send_replace(true);
    }

    pub(crate) fn shutdown_receiver(&self) -> watch::Receiver<bool> {
        self.shutdown_rx.clone()
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
        let subscribe_all_catalog_tracks = *self.subscribe_all_catalog_tracks.read().await;
        crate::subscription_manager::BroadcastSubscriptionManager::new(
            self.clone(),
            broadcast_name,
            catalog_type,
            requested_tracks,
            subscribe_all_catalog_tracks,
        )
        .await
    }

    /// Set data callback for the internal subscription manager
    /// Stores the callback in session and applies it to existing manager if present
    pub async fn set_subscription_data_callback<F>(&self, callback: F) -> Result<()>
    where
        F: Fn(String, Vec<u8>) + Send + Sync + 'static,
    {
        // Store the callback in the session for future manager creation/recreation
        let callback_arc = Arc::new(callback);
        *self.data_callback.write().await = Some(callback_arc.clone());
        debug!("Data callback stored in session");

        // Apply to existing managers if present
        let include_broadcast_in_callback = self.subscription_mode == SubscriptionMode::RoomPrefix;
        for (broadcast_name, manager) in self.broadcast_subscription_managers.read().await.iter() {
            debug!(
                "Applying data callback to BroadcastSubscriptionManager for {}",
                broadcast_name
            );
            let callback_for_manager = callback_arc.clone();
            let broadcast_name = broadcast_name.clone();
            manager
                .set_data_callback(move |track: String, data: Vec<u8>| {
                    let name = if include_broadcast_in_callback {
                        format!("{}/{}", broadcast_name, track)
                    } else {
                        track
                    };
                    callback_for_manager(name, data);
                })
                .await;
        }

        Ok(())
    }

    fn should_subscribe_to_announcement(&self, path: &str) -> bool {
        match self.subscription_mode {
            SubscriptionMode::ExactBroadcast => path == self.broadcast_name,
            SubscriptionMode::RoomPrefix => {
                let prefix = self.broadcast_name.trim_matches('/');
                if prefix.is_empty() {
                    true
                } else {
                    path == prefix
                        || path
                            .strip_prefix(prefix)
                            .is_some_and(|suffix| suffix.starts_with('/'))
                }
            }
        }
    }

    async fn stop_all_subscription_managers(&self) {
        let managers = {
            let mut guard = self.broadcast_subscription_managers.write().await;
            guard
                .drain()
                .map(|(_, manager)| manager)
                .collect::<Vec<_>>()
        };

        for manager in managers {
            manager.stop().await;
        }
    }

    async fn remove_subscription_manager(&self, broadcast_name: &str) {
        let manager = self
            .broadcast_subscription_managers
            .write()
            .await
            .remove(broadcast_name);

        if let Some(manager) = manager {
            manager.stop().await;
        }

        self.state
            .write()
            .await
            .broadcast_consumers
            .remove(broadcast_name);
    }

    /// Create or recreate a BroadcastSubscriptionManager with stored configuration.
    async fn create_or_recreate_manager_for(&self, broadcast_name: String) -> Result<()> {
        // Stop existing manager for this broadcast if present.
        if let Some(manager) = self
            .broadcast_subscription_managers
            .write()
            .await
            .remove(&broadcast_name)
        {
            debug!(
                "Stopping existing BroadcastSubscriptionManager for {}",
                broadcast_name
            );
            manager.stop().await;
        }

        // For subscriber sessions, subscribe to the broadcast first if this
        // manager was not created from an announce update that already provided
        // the broadcast consumer.
        if matches!(self.session_type, SessionType::Subscriber) {
            let has_broadcast_consumer = self
                .state
                .read()
                .await
                .broadcast_consumers
                .contains_key(&broadcast_name);

            if !has_broadcast_consumer {
                debug!(
                    "Subscribing to broadcast '{}' before creating manager",
                    broadcast_name
                );
                if let Err(e) = self.subscribe_broadcast(&broadcast_name).await {
                    warn!(
                        "Failed to subscribe to broadcast '{}': {}",
                        broadcast_name, e
                    );
                    return Err(e);
                }
            }
        }

        // Get configuration from session storage
        let catalog_type = self.catalog_type.read().await.clone();
        let requested_tracks = self.requested_tracks.read().await.clone();
        let data_callback = self.data_callback.read().await.clone();
        let include_broadcast_in_callback = self.subscription_mode == SubscriptionMode::RoomPrefix;

        // Create the new manager
        match self
            .create_subscription_manager(broadcast_name.clone(), catalog_type, requested_tracks)
            .await
        {
            Ok(new_manager) => {
                // Set the data callback if one exists
                if let Some(callback) = data_callback {
                    let broadcast_for_callback = broadcast_name.clone();
                    new_manager
                        .set_data_callback(move |track: String, data: Vec<u8>| {
                            let name = if include_broadcast_in_callback {
                                format!("{}/{}", broadcast_for_callback, track)
                            } else {
                                track
                            };
                            callback(name, data);
                        })
                        .await;
                }

                // A shutdown that raced this creation has already drained the map.
                if *self.shutdown_rx.borrow() {
                    new_manager.stop().await;
                    return Ok(());
                }

                self.broadcast_subscription_managers
                    .write()
                    .await
                    .insert(broadcast_name, new_manager);
                Ok(())
            }
            Err(e) => {
                warn!("Failed to create BroadcastSubscriptionManager: {}", e);
                Err(e)
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct ConnectionInfo {
    pub connected: bool,
    /// Relay connections made so far, the first one and every reconnect.
    pub connection_attempts: usize,
    pub last_connection_time: Option<Instant>,
    /// Why the reconnect loop stopped for good, once it has.
    pub last_error: Option<String>,
}

/// Publisher-specific functionality
impl MoqSession {
    /// Add a track definition to the session
    pub async fn add_track_definition(&self, track_def: TrackDefinition) -> Result<()> {
        let track_info =
            TrackInfo::default().with_priority(track_def.priority.try_into().unwrap_or(u8::MAX));

        let track_handle = TrackHandle {
            producer: None, // Will be created when session connects
            track_info,
            track_definition: Some(track_def.clone()),
        };

        // Store track for later creation when session connects
        self.tracks
            .write()
            .await
            .insert(track_def.name.clone(), track_handle);

        // Random starting group sequence number for this track
        let random_start: u64 = rand::thread_rng().gen_range(1..=10000);

        self.sequence_numbers
            .write()
            .await
            .insert(track_def.name.clone(), random_start);

        debug!(
            "Track '{}' initialized with random starting group sequence: {}",
            track_def.name, random_start
        );

        if matches!(self.session_type, SessionType::Subscriber) {
            self.requested_tracks.write().await.push(track_def.clone());
        }

        debug!(
            "Added track definition: {} ({})",
            track_def.name, track_def.track_type
        );
        Ok(())
    }

    /// Set catalog for publisher
    pub async fn set_catalog(&self, catalog: Catalog) -> Result<()> {
        if !matches!(self.session_type, SessionType::Publisher) {
            return Err(
                WrapperError::Session("Only publishers can set catalog".to_string()).into(),
            );
        }

        *self.catalog.write().await = Some(catalog);

        // Add catalog.json track
        let catalog_track = TrackDefinition::data("catalog.json", u32::MAX); // Highest priority
        self.add_track_definition(catalog_track).await?;

        debug!("Set catalog for publisher");
        Ok(())
    }

    /// Set catalog type for subscriber
    pub async fn set_catalog_type(&self, catalog_type: CatalogType) -> Result<()> {
        *self.catalog_type.write().await = catalog_type.clone();

        debug!("Set catalog type: {:?}", catalog_type);
        Ok(())
    }

    /// Set a log callback to receive session-specific log messages
    ///
    /// # Arguments
    /// * `callback` - Optional callback function that receives (target, level, message)
    ///
    /// # Example
    /// ```ignore
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
                    track_producer.write_frame(Timestamp::now(), Bytes::from(catalog_json))?;
                    debug!("Published catalog data");
                }
            }
        }
        Ok(())
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
            .map_err(|e| WrapperError::Session(format!("Failed to create group: {}", e)))?;

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

        group.write_frame(Timestamp::now(), data)?;
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
        if let Some(mut group) = groups.remove(track_name) {
            group.finish()?;
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
    pub async fn publish_broadcast(&self, _broadcast: BroadcastConsumer) -> Result<()> {
        if let SessionType::Publisher = self.session_type {
            Err(WrapperError::Session(
                "Publishing external broadcast handles is not supported by the current upstream moq-net API; use the session's configured tracks".to_string(),
            )
            .into())
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
                    let track_producer = broadcast_producer
                        .create_track(name.as_str(), Some(handle.track_info.clone()))?;
                    handle.producer = Some(track_producer);
                    debug!("Created track producer for: {}", name);
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
                debug!("Published catalog data after track setup");
            }
        }

        Ok(())
    }

    /// Set a data callback for receiving track data automatically
    /// This is an alias for set_subscription_data_callback for backward compatibility
    pub async fn set_data_callback<F>(&self, callback: F) -> Result<()>
    where
        F: Fn(String, Vec<u8>) + Send + Sync + 'static,
    {
        self.set_subscription_data_callback(callback).await
    }

    /// Close the session and stop all operations
    pub async fn close_session(&self) -> Result<()> {
        debug!("Closing MoQ session");

        self.request_shutdown();

        // Clear session state
        {
            let mut state = self.state.write().await;
            state.connected = false;
            state.origin = None;
            state.origin_consumer = None;
            state.broadcast = None;
            state.broadcast_consumers.clear();
        }

        // Clear tracks and groups
        {
            self.tracks.write().await.clear();
            self.current_groups.write().await.clear();
            self.sequence_numbers.write().await.clear();
        }

        // Shutdown broadcast subscription managers
        self.stop_all_subscription_managers().await;

        debug!("Session closed successfully");
        Ok(())
    }
}

/// Subscriber-specific functionality  
impl MoqSession {
    /// Subscribe to a broadcast (only available for subscriber sessions)
    pub async fn subscribe_broadcast(&self, broadcast_name: &str) -> Result<BroadcastConsumer> {
        debug!(
            "[MoqSession] subscribe_broadcast called for: '{}'",
            broadcast_name
        );

        if !matches!(self.session_type, SessionType::Subscriber) {
            return Err(WrapperError::Session("Not a subscriber session".to_string()).into());
        }

        let mut state = self.state.write().await;
        if !state.connected {
            return Err(WrapperError::Session("Not connected".to_string()).into());
        }

        if let Some(origin_consumer) = &state.origin_consumer.clone() {
            let broadcast_consumer = origin_consumer
                .request_broadcast(broadcast_name)
                .await
                .map_err(|e| {
                    WrapperError::Session(format!(
                        "Failed to consume broadcast '{}': {}",
                        broadcast_name, e
                    ))
                })?;

            info!(
                "[MoqSession] Successfully consumed broadcast: '{}'",
                broadcast_name
            );
            state
                .broadcast_consumers
                .insert(broadcast_name.to_string(), broadcast_consumer.clone());
            Ok(broadcast_consumer)
        } else {
            Err(WrapperError::Session("No origin consumer available".to_string()).into())
        }
    }

    /// Get the stored broadcast consumer for the default broadcast.
    pub async fn get_broadcast_consumer(&self) -> Result<BroadcastConsumer> {
        self.get_broadcast_consumer_for(&self.broadcast_name).await
    }

    /// Get the stored broadcast consumer for a broadcast path.
    pub async fn get_broadcast_consumer_for(
        &self,
        broadcast_name: &str,
    ) -> Result<BroadcastConsumer> {
        let state = self.state.read().await;
        if let Some(broadcast_consumer) = state.broadcast_consumers.get(broadcast_name) {
            Ok(broadcast_consumer.clone())
        } else {
            Err(WrapperError::Session(format!(
                "No broadcast consumer available for '{}' - session may not be connected or subscriber not initialized",
                broadcast_name
            )).into())
        }
    }

    /// Internal method to subscribe to a track without the resilient wrapper
    pub async fn subscribe_track_internal(
        &self,
        broadcast_name: &str,
        track_name: &str,
    ) -> Result<TrackConsumer> {
        let broadcast = self.get_broadcast_consumer_for(broadcast_name).await?;
        let track_consumer = broadcast.track(track_name)?.subscribe(None).await?;

        debug!(
            "[MoqSession] Subscribed to track: {} in broadcast: {}",
            track_name, broadcast_name
        );
        Ok(track_consumer)
    }
}
