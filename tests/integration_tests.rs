use std::sync::Arc;
use std::time::Duration;

use moq_wrapper::{
    close_session, create_publisher, create_room_subscriber_with_options,
    create_subscriber_with_options, write_single_frame, CatalogType, ConnectionConfig, MoqSession,
    SessionConfig, TrackDefinition, TrackManager,
};

/// This is a basic integration test that doesn't require an actual relay server.
/// It tests the API surface and basic functionality.
#[tokio::test]
async fn test_session_creation() {
    // Test creating publisher and subscriber sessions
    let url = url::Url::parse("https://example.com/test").unwrap();
    let config = SessionConfig::new("test-broadcast", url);

    let publisher = MoqSession::publisher(
        config.clone(),
        "test-broadcast".to_string(),
        CatalogType::None,
        vec![],
    )
    .await;
    assert!(publisher.is_ok());

    let subscriber = MoqSession::subscriber(
        config,
        "test-broadcast".to_string(),
        CatalogType::None,
        vec![],
    )
    .await;
    assert!(subscriber.is_ok());
}

/// Session setup must not block the runtime, so it works on a current-thread runtime.
#[tokio::test(flavor = "current_thread")]
async fn test_publisher_with_catalog_on_current_thread_runtime() {
    let url = url::Url::parse("https://example.com/test").unwrap();
    let config = SessionConfig::new("test-broadcast", url);

    let publisher = MoqSession::publisher(
        config,
        "test-broadcast".to_string(),
        CatalogType::Sesame,
        vec![
            TrackDefinition::video("video", 1),
            TrackDefinition::audio("audio", 2),
        ],
    )
    .await
    .unwrap();

    let mut tracks = publisher.list_tracks().await;
    tracks.sort();
    assert_eq!(tracks, vec!["audio", "catalog.json", "video"]);
}

#[tokio::test]
async fn test_room_subscriber_creation() {
    let url = url::Url::parse("https://example.com/test").unwrap();
    let config = SessionConfig::new("room/demo", url);

    let subscriber =
        MoqSession::room_subscriber(config, "room/demo".to_string(), CatalogType::None, vec![])
            .await;
    assert!(subscriber.is_ok());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live MoQ relay"]
async fn live_room_subscriber_receives_announced_broadcast() -> anyhow::Result<()> {
    let relay_url = std::env::var("MOQ_RELAY_URL")
        .unwrap_or_else(|_| "https://r2.moq.sesame-streams.com:4433".to_string());
    let nonce = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis()
    );
    let room_prefix = format!("codex-room-smoke/{}", nonce);
    let broadcast_name = format!("{}/alice", room_prefix);
    let track_name = "chat".to_string();
    let payload = format!("hello-from-{}", nonce).into_bytes();

    let publisher = create_publisher(
        &relay_url,
        &broadcast_name,
        vec![TrackDefinition::data(track_name.clone(), 0)],
        CatalogType::None,
    )
    .await?;

    let room_config = SessionConfig::new(&room_prefix, url::Url::parse(&relay_url)?);
    let room_subscriber = MoqSession::room_subscriber(
        room_config,
        room_prefix.clone(),
        CatalogType::None,
        vec![TrackDefinition::data(track_name.clone(), 0)],
    )
    .await?;

    let (data_tx, mut data_rx) = tokio::sync::mpsc::unbounded_channel();
    room_subscriber
        .set_data_callback(move |track, data| {
            let _ = data_tx.send((track, data));
        })
        .await?;
    room_subscriber.start().await?;

    let expected_track = format!("{}/{}", broadcast_name, track_name);
    let started = tokio::time::Instant::now();
    let mut publish_interval = tokio::time::interval(Duration::from_millis(250));

    let received = loop {
        if started.elapsed() > Duration::from_secs(15) {
            anyhow::bail!(
                "timed out waiting for {} on {} via {}",
                expected_track,
                room_prefix,
                relay_url
            );
        }

        tokio::select! {
            _ = publish_interval.tick() => {
                write_single_frame(&publisher, &track_name, payload.clone()).await?;
            }
            maybe_frame = data_rx.recv() => {
                if let Some((track, data)) = maybe_frame {
                    if track == expected_track && data == payload {
                        break true;
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(50)) => {}
        }
    };

    assert!(received);
    close_session(&room_subscriber).await?;
    close_session(&publisher).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live MoQ relay"]
async fn live_catalog_all_tracks_exact_subscriber_receives_catalog_track() -> anyhow::Result<()> {
    live_catalog_all_tracks_subscriber_receives_catalog_track(false).await
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live MoQ relay"]
async fn live_catalog_all_tracks_room_subscriber_receives_catalog_track() -> anyhow::Result<()> {
    live_catalog_all_tracks_subscriber_receives_catalog_track(true).await
}

async fn live_catalog_all_tracks_subscriber_receives_catalog_track(
    room: bool,
) -> anyhow::Result<()> {
    let relay_url = std::env::var("MOQ_RELAY_URL")
        .unwrap_or_else(|_| "https://r2.moq.sesame-streams.com:4433".to_string());
    let nonce = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis()
    );
    let room_prefix = format!("codex-catalog-smoke/{}", nonce);
    let broadcast_name = if room {
        format!("{}/publisher-1", room_prefix)
    } else {
        room_prefix.clone()
    };
    let track_name = "catalog-clock".to_string();
    let payload = format!("catalog-hello-from-{}", nonce).into_bytes();

    let publisher = create_publisher(
        &relay_url,
        &broadcast_name,
        vec![TrackDefinition::data(track_name.clone(), 0)],
        CatalogType::Sesame,
    )
    .await?;

    let subscriber = if room {
        create_room_subscriber_with_options(
            &relay_url,
            &room_prefix,
            Vec::new(),
            CatalogType::Sesame,
            true,
        )
        .await?
    } else {
        create_subscriber_with_options(
            &relay_url,
            &broadcast_name,
            Vec::new(),
            CatalogType::Sesame,
            true,
        )
        .await?
    };

    let (data_tx, mut data_rx) = tokio::sync::mpsc::unbounded_channel();
    subscriber
        .set_data_callback(move |track, data| {
            let _ = data_tx.send((track, data));
        })
        .await?;

    let expected_track = if room {
        format!("{}/{}", broadcast_name, track_name)
    } else {
        track_name.clone()
    };
    let started = tokio::time::Instant::now();
    let mut publish_interval = tokio::time::interval(Duration::from_millis(250));

    let received = loop {
        if started.elapsed() > Duration::from_secs(15) {
            anyhow::bail!(
                "timed out waiting for {} via {} with catalog-all mode",
                expected_track,
                relay_url
            );
        }

        tokio::select! {
            _ = publish_interval.tick() => {
                write_single_frame(&publisher, &track_name, payload.clone()).await?;
            }
            maybe_frame = data_rx.recv() => {
                if let Some((track, data)) = maybe_frame {
                    if track == expected_track && data == payload {
                        break true;
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(50)) => {}
        }
    };

    assert!(received);
    close_session(&subscriber).await?;
    close_session(&publisher).await?;

    Ok(())
}

#[tokio::test]
async fn test_track_manager() {
    let url = url::Url::parse("https://example.com/test").unwrap();
    let config = SessionConfig::new("test-broadcast", url);

    let session = Arc::new(
        MoqSession::publisher(
            config,
            "test-broadcast".to_string(),
            CatalogType::None,
            vec![],
        )
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
        MoqSession::publisher(
            config,
            "test-broadcast".to_string(),
            CatalogType::None,
            vec![],
        )
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
    };

    // Test that configuration is properly stored
    assert_eq!(session_config.broadcast_name, "test-config");
    assert_eq!(session_config.connection.max_reconnect_attempts, 5);
    assert_eq!(
        session_config.connection.reconnect_delay,
        Duration::from_millis(500)
    );
}

#[tokio::test]
async fn test_session_state() {
    let url = url::Url::parse("https://example.com/test").unwrap();
    let config = SessionConfig::new("test-broadcast", url);

    let session = MoqSession::publisher(
        config,
        "test-broadcast".to_string(),
        CatalogType::None,
        vec![],
    )
    .await
    .unwrap();

    // Should start disconnected
    assert!(!session.is_connected().await);

    let info = session.connection_info().await;
    assert!(!info.connected);
    assert_eq!(info.connection_attempts, 0);
    assert!(info.last_connection_time.is_none());
}
