pub mod catalog;
pub mod config;
pub mod ffi;
pub mod session;
pub mod subscription;
pub mod track;

pub use catalog::{Catalog, CatalogType, HangCatalog, SesameCatalog, TrackDefinition, TrackType};
pub use config::{ConnectionConfig, SessionConfig, WrapperError};
pub use session::{ConnectionInfo, MoqSession, SessionEvent, SessionType};
pub use subscription::{DataCallback, SubscriptionManager};
pub use track::{StreamPublisher, TrackManager};

// Re-export commonly used types from moq-lite for convenience
pub use bytes::Bytes;
pub use moq_lite::{
    Broadcast, BroadcastConsumer, BroadcastProducer, Track, TrackConsumer, TrackProducer,
};

// Re-export tracing types for logging
use anyhow::Result;
pub use tracing::Level;
use tracing_subscriber::Layer;

/// Log callback function type
pub type LogCallback = Box<dyn Fn(&str, Level, &str) + Send + Sync>;

/// Initialize the moq-wrapper library with logging configuration
///
/// # Arguments
/// * `log_level` - The maximum log level to display (DEBUG, INFO, WARN, ERROR)
/// * `log_callback` - Optional custom log callback. If None, uses default console logging
///
/// # Example
/// ```rust
/// use moq_wrapper::init;
/// use tracing::Level;
///
/// // Initialize with INFO level and default console logging
/// init(Level::INFO, None);
///
/// // Or with a custom callback
/// init(Level::DEBUG, Some(Box::new(|target, level, message| {
///     println!("[{}] {}: {}", level, target, message);
/// })));
/// ```
pub fn init(log_level: Level, log_callback: Option<LogCallback>) {
    if let Some(callback) = log_callback {
        // Custom callback subscriber
        use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Layer};

        let callback_layer = CallbackLayer::new(callback);

        tracing_subscriber::registry()
            .with(
                callback_layer.with_filter(tracing_subscriber::filter::LevelFilter::from_level(
                    log_level,
                )),
            )
            .init();
    } else {
        // Default console logging
        tracing_subscriber::fmt().with_max_level(log_level).init();
    }
}

// Custom tracing layer for log callbacks
struct CallbackLayer {
    callback: LogCallback,
}

impl CallbackLayer {
    fn new(callback: LogCallback) -> Self {
        Self { callback }
    }
}

impl<S> Layer<S> for CallbackLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let metadata = event.metadata();
        let target = metadata.target();
        let level = *metadata.level();

        // Extract the message from the event
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        (self.callback)(target, level, &visitor.message);
    }
}

// Visitor to extract message from tracing events
#[derive(Default)]
struct MessageVisitor {
    message: String,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
            // Remove quotes from debug output
            if self.message.starts_with('"') && self.message.ends_with('"') {
                self.message = self.message[1..self.message.len() - 1].to_string();
            }
        }
    }
}

/// Create a quick publisher session with specified tracks and catalog
pub async fn create_publisher(
    url: &str,
    broadcast_name: &str,
    tracks: Vec<TrackDefinition>,
    catalog_type: CatalogType,
) -> Result<MoqSession, WrapperError> {
    let url = url::Url::parse(url)
        .map_err(|e| WrapperError::InvalidConfig(format!("Invalid URL: {}", e)))?;

    let config = SessionConfig::new(broadcast_name, url);
    let mut session = MoqSession::publisher(config, broadcast_name.to_string()).await?;

    // Pre-configure tracks
    for track_def in &tracks {
        session.add_track_definition(track_def.clone())?;
    }

    // Add catalog track if needed
    if catalog_type != CatalogType::None {
        let catalog = Catalog::new(catalog_type.clone(), &tracks)
            .ok_or_else(|| WrapperError::InvalidConfig("Failed to create catalog".to_string()))?;
        session.set_catalog(catalog)?;
    }

    session.start().await?;

    // Wait for initial connection (track producers will be created automatically)
    use tokio::time::{sleep, Duration};
    while !session.is_connected().await {
        sleep(Duration::from_millis(100)).await;
    }

    Ok(session)
}

/// Write a frame to a track, optionally starting a new group
/// If new_group is true, starts a new group before writing the frame
pub async fn write_frame(
    session: &MoqSession,
    track_name: &str,
    data: Vec<u8>,
    new_group: bool,
) -> Result<(), WrapperError> {
    if new_group {
        session
            .start_group(track_name)
            .await
            .map_err(|e| WrapperError::Session(format!("Failed to start group: {}", e)))?;
    }

    session
        .write_frame(track_name, Bytes::from(data))
        .await
        .map_err(|e| WrapperError::Session(format!("Failed to write frame: {}", e)))
}

/// Write a single frame in its own group (convenience method)
/// Creates a new group, writes the frame, and closes the group
pub async fn write_single_frame(
    session: &MoqSession,
    track_name: &str,
    data: Vec<u8>,
) -> Result<(), WrapperError> {
    session
        .write_single_frame(track_name, Bytes::from(data))
        .await
        .map_err(|e| WrapperError::Session(format!("Failed to write single frame: {}", e)))
}

/// Create a quick subscriber session with specified tracks and catalog validation
pub async fn create_subscriber(
    url: &str,
    broadcast_name: &str,
    tracks: Vec<TrackDefinition>,
    catalog_type: CatalogType,
) -> Result<MoqSession, WrapperError> {
    let url = url::Url::parse(url)
        .map_err(|e| WrapperError::InvalidConfig(format!("Invalid URL: {}", e)))?;

    let config = SessionConfig::new(broadcast_name, url);
    let mut session = MoqSession::subscriber(config, broadcast_name.to_string()).await?;

    // Configure requested tracks
    for track_def in tracks {
        session.add_track_definition(track_def)?;
    }

    // Set catalog type for validation
    session.set_catalog_type(catalog_type)?;

    session.start().await?;
    Ok(session)
}

/// Set a data callback on a session for receiving track data automatically
pub async fn set_data_callback<F>(session: &MoqSession, callback: F)
where
    F: Fn(String, Vec<u8>) + Send + Sync + 'static,
{
    session.set_data_callback(callback).await;
}

/// Close a session and stop all operations
pub async fn close_session(session: &MoqSession) -> Result<(), WrapperError> {
    session
        .close_session()
        .await
        .map_err(|e| WrapperError::Session(format!("Failed to close session: {}", e)))
}

/// Simplified publish data function that handles group creation internally
pub async fn publish_data(
    session: &MoqSession,
    track_name: &str,
    data: Vec<u8>,
) -> Result<(), WrapperError> {
    session.publish_data(track_name, data).await
}
