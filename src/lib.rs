pub mod catalog;
pub mod config;
pub mod ffi;
pub mod runtime;
pub mod session;
pub mod subscription_manager;
pub mod track;

pub use catalog::{Catalog, CatalogType, HangCatalog, SesameCatalog, TrackDefinition, TrackType};
pub use config::{ConnectionConfig, SessionConfig, WrapperError};
pub use runtime::{shared_client, shared_runtime};
pub use session::{
    ConnectionInfo, DataCallback, MoqSession, SessionEvent, SessionLogCallback, SessionType,
    SubscriptionMode,
};
pub use subscription_manager::BroadcastSubscriptionManager;
pub use track::{StreamPublisher, TrackManager};

// Re-export commonly used upstream MoQ types for convenience.
pub use bytes::Bytes;
pub use moq_native::moq_net::broadcast::{
    Consumer as BroadcastConsumer, Producer as BroadcastProducer,
};
pub use moq_native::moq_net::track::{
    Info as Track, Producer as TrackProducer, Subscriber as TrackConsumer,
};

// Re-export tracing types for logging
use anyhow::Result;
use std::sync::Once;
use tracing::warn;
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
    create_publisher_with_config(config, broadcast_name, tracks, catalog_type).await
}

/// Create a publisher session from a full configuration, for callers that need
/// more than the URL (TLS options, backoff, timeouts)
pub async fn create_publisher_with_config(
    config: SessionConfig,
    broadcast_name: &str,
    tracks: Vec<TrackDefinition>,
    catalog_type: CatalogType,
) -> Result<MoqSession, WrapperError> {
    let url_display = config.connection.url.to_string();
    let connect_timeout = config.connection.connect_timeout;
    let session = MoqSession::publisher(
        config,
        broadcast_name.to_string(),
        catalog_type,
        tracks.clone(),
    )
    .await?;

    session.start().await?;

    // Give the first connection a moment so early writes are not dropped. A relay
    // that is not reachable yet does not fail the session: the reconnect loop
    // keeps trying, and writes succeed once is_connected() turns true. Only a
    // loop that gave up for good (a rejected token, a refusing relay) is an error.
    use tokio::time::{sleep, timeout, Duration};
    let wait_for_connection = async {
        loop {
            let info = session.connection_info().await;
            if info.connected {
                return Ok(());
            }
            if let Some(error) = info.last_error {
                return Err(WrapperError::ReconnectionFailed(format!(
                    "{}: {}",
                    url_display, error
                )));
            }
            sleep(Duration::from_millis(100)).await;
        }
    };

    let wait = if connect_timeout.is_zero() {
        Duration::from_secs(10)
    } else {
        connect_timeout + Duration::from_secs(5)
    };
    match timeout(wait, wait_for_connection).await {
        Ok(result) => result?,
        Err(_) => warn!(
            "Not connected to {} yet, the session keeps retrying in the background",
            url_display
        ),
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
    create_subscriber_with_options(url, broadcast_name, tracks, catalog_type, false).await
}

/// Create a quick subscriber session with subscription options.
pub async fn create_subscriber_with_options(
    url: &str,
    broadcast_name: &str,
    tracks: Vec<TrackDefinition>,
    catalog_type: CatalogType,
    subscribe_all_catalog_tracks: bool,
) -> Result<MoqSession, WrapperError> {
    let url = url::Url::parse(url)
        .map_err(|e| WrapperError::InvalidConfig(format!("Invalid URL: {}", e)))?;

    let config = SessionConfig::new(broadcast_name, url);
    let session = MoqSession::subscriber_with_options(
        config,
        broadcast_name.to_string(),
        catalog_type,
        tracks.clone(),
        subscribe_all_catalog_tracks,
    )
    .await?;

    // Start the session to establish connection
    session.start().await?;

    Ok(session)
}

/// Create a subscriber session that treats `room_prefix` as an announcement prefix.
///
/// Every announced broadcast under the prefix gets its own track subscriptions.
/// Data callbacks receive `broadcast_path/track_name` so callers can distinguish
/// sources inside the room.
pub async fn create_room_subscriber(
    url: &str,
    room_prefix: &str,
    tracks: Vec<TrackDefinition>,
    catalog_type: CatalogType,
) -> Result<MoqSession, WrapperError> {
    create_room_subscriber_with_options(url, room_prefix, tracks, catalog_type, false).await
}

/// Create a room subscriber session with subscription options.
pub async fn create_room_subscriber_with_options(
    url: &str,
    room_prefix: &str,
    tracks: Vec<TrackDefinition>,
    catalog_type: CatalogType,
    subscribe_all_catalog_tracks: bool,
) -> Result<MoqSession, WrapperError> {
    let url = url::Url::parse(url)
        .map_err(|e| WrapperError::InvalidConfig(format!("Invalid URL: {}", e)))?;

    let config = SessionConfig::new(room_prefix, url);
    let session = MoqSession::room_subscriber_with_options(
        config,
        room_prefix.to_string(),
        catalog_type,
        tracks.clone(),
        subscribe_all_catalog_tracks,
    )
    .await?;

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
