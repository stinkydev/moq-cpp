use std::sync::Arc;
use std::time::Duration;

use moq_wrapper::{ConnectionConfig, MoqSession, SessionConfig, TrackManager};

/// This is a basic integration test that doesn't require an actual relay server.
/// It tests the API surface and basic functionality.
#[tokio::test]
async fn test_session_creation() {
    // Test creating publisher and subscriber sessions
    let url = url::Url::parse("https://example.com/test").unwrap();
    let config = SessionConfig::new("test-broadcast", url);

    let publisher = MoqSession::publisher(config.clone(), "test-broadcast".to_string()).await;
    assert!(publisher.is_ok());

    let subscriber = MoqSession::subscriber(config, "test-broadcast".to_string()).await;
    assert!(subscriber.is_ok());
}

#[tokio::test]
async fn test_track_manager() {
    let url = url::Url::parse("https://example.com/test").unwrap();
    let config = SessionConfig::new("test-broadcast", url);

    let session = Arc::new(
        MoqSession::publisher(config, "test-broadcast".to_string())
            .await
            .unwrap(),
    );
    let track_manager = TrackManager::new(session);

    // Test track manager creation
    assert!(track_manager.list_tracks().await.is_empty());
}

#[tokio::test]
async fn test_stream_publisher() {
    // This test creates a stream publisher but doesn't actually connect
    // since we don't have a test relay server

    let url = url::Url::parse("https://example.com/test").unwrap();
    let config = SessionConfig::new("test-broadcast", url);

    let session = Arc::new(
        MoqSession::publisher(config, "test-broadcast".to_string())
            .await
            .unwrap(),
    );
    let track_manager = TrackManager::new(session);

    // The track creation will fail without a connection, but we can test the API
    let result = track_manager
        .create_publish_track("test-track".to_string(), 0)
        .await;
    // This should fail because we're not connected, which is expected
    assert!(result.is_err());
}

#[tokio::test]
async fn test_configuration() {
    let connection_config = ConnectionConfig {
        url: url::Url::parse("https://test.example.com/path").unwrap(),
        max_reconnect_attempts: 5,
        reconnect_delay: Duration::from_millis(500),
        ..Default::default()
    };

    let session_config = SessionConfig {
        broadcast_name: "test-config".to_string(),
        connection: connection_config,
        auto_reconnect: true,
    };

    // Test that configuration is properly stored
    assert_eq!(session_config.broadcast_name, "test-config");
    assert_eq!(session_config.connection.max_reconnect_attempts, 5);
    assert_eq!(
        session_config.connection.reconnect_delay,
        Duration::from_millis(500)
    );
    assert!(session_config.auto_reconnect);
}

#[tokio::test]
async fn test_session_state() {
    let url = url::Url::parse("https://example.com/test").unwrap();
    let config = SessionConfig::new("test-broadcast", url);

    let session = MoqSession::publisher(config, "test-broadcast".to_string())
        .await
        .unwrap();

    // Should start disconnected
    assert!(!session.is_connected().await);

    let info = session.connection_info().await;
    assert!(!info.connected);
    assert_eq!(info.connection_attempts, 0);
    assert!(info.last_connection_time.is_none());
}
