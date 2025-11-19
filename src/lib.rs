pub mod catalog;
pub mod config;
pub mod ffi;
pub mod session;
pub mod subscription_manager;
pub mod track;

pub use catalog::{Catalog, CatalogType, HangCatalog, SesameCatalog, TrackDefinition, TrackType};
pub use config::{ConnectionConfig, SessionConfig, WrapperError};
pub use session::{
    ConnectionInfo, DataCallback, MoqSession, SessionEvent, SessionLogCallback, SessionType,
};
pub use subscription_manager::BroadcastSubscriptionManager;
pub use track::{StreamPublisher, TrackManager};

// Re-export commonly used types from moq-lite for convenience
pub use bytes::Bytes;
pub use moq_lite::{
    Broadcast, BroadcastConsumer, BroadcastProducer, Track, TrackConsumer, TrackProducer,
};

// Re-export tracing types for logging
use anyhow::Result;
use std::sync::Once;
pub use tracing::Level;

static TRACING_INIT: Once = Once::new();

/// Set the global log level for internal library tracing (optional)
///
/// This initializes the global tracing subscriber for internal library diagnostics.
/// Session-specific logging is handled separately via MoqSession::set_log_callback().
/// This function is optional - the library works fine without it.
///
/// # Arguments
/// * `log_level` - The maximum log level to display (DEBUG, INFO, WARN, ERROR)
///
/// # Example
/// ```rust
/// use moq_wrapper::set_log_level;
/// use tracing::Level;
///
/// // Set global logging to INFO level (optional)
/// set_log_level(Level::INFO);
/// ```
pub fn set_log_level(log_level: Level) {
    // Initialize tracing subscriber only once per process to avoid panic
    // Subsequent calls will be ignored, but this prevents the crash
    TRACING_INIT.call_once(|| {
        tracing_subscriber::fmt().with_max_level(log_level).init();
    });
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
    let session = MoqSession::publisher(
        config,
        broadcast_name.to_string(),
        catalog_type,
        tracks.clone(),
    )
    .await?;

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
    let session = MoqSession::subscriber(
        config,
        broadcast_name.to_string(),
        catalog_type,
        tracks.clone(),
    )
    .await?;

    // Start the session to establish connection
    session.start().await?;

    Ok(session)
}

/// Set a data callback on a session for receiving track data automatically
pub async fn set_data_callback<F>(session: &MoqSession, callback: F) -> Result<(), WrapperError>
where
    F: Fn(String, Vec<u8>) + Send + Sync + 'static,
{
    session
        .set_data_callback(callback)
        .await
        .map_err(|e| WrapperError::Session(e.to_string()))
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
