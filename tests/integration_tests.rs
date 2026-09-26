use std::sync::Arc;
use std::time::Duration;

use moq_wrapper::{
    close_session, create_publisher, create_room_subscriber_with_options,
    create_subscriber_with_options, write_single_frame, Bytes, CatalogType, ConnectionConfig,
    MoqSession, SessionConfig, TrackDefinition, TrackManager,
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
        reconnect_timeout: Duration::from_secs(60),
        reconnect_delay: Duration::from_millis(500),
        connect_timeout: Duration::from_secs(3),
        ..Default::default()
    };

    let session_config = SessionConfig {
        broadcast_name: "test-config".to_string(),
        connection: connection_config,
    };

    // Test that configuration is properly stored
    assert_eq!(session_config.broadcast_name, "test-config");
    assert_eq!(
        session_config.connection.reconnect_delay,
        Duration::from_millis(500)
    );

    // The dial timeout and the reconnect backoff reach moq-native.
    let client_config = session_config.connection.resolved_client_config().unwrap();
    assert_eq!(client_config.timeout, Some(Duration::from_secs(3)));
    assert_eq!(client_config.backoff.initial, Duration::from_millis(500));
    assert_eq!(client_config.backoff.max, Duration::from_secs(10));
    assert_eq!(client_config.backoff.timeout, Duration::from_secs(60));

    // By default a session retries for as long as it lives.
    let default_client_config = ConnectionConfig::default()
        .resolved_client_config()
        .unwrap();
    assert!(default_client_config.backoff.timeout.is_zero());

    // A zero delay would retry without pacing.
    let unpaced = ConnectionConfig {
        reconnect_delay: Duration::ZERO,
        ..Default::default()
    };
    assert!(unpaced.resolved_client_config().is_err());
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

/// A subscriber that joins while a group is still open must receive that group
/// from its first frame, not from the frame that happens to be current.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live MoQ relay"]
async fn live_subscriber_joining_mid_group_receives_open_group_from_first_frame(
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
    let broadcast_name = format!("mid-group-smoke/{}", nonce);
    let track_name = "frames".to_string();
    let frame_interval = Duration::from_millis(200);
    let frames_before_join = 5usize;

    let publisher = create_publisher(
        &relay_url,
        &broadcast_name,
        vec![TrackDefinition::data(track_name.clone(), 0)],
        CatalogType::None,
    )
    .await?;
    let publisher = Arc::new(publisher);

    // Keep writing frames into whatever group is open until told to stop.
    let (next_group_tx, mut next_group_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
    let (written_tx, mut written_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let publish_task = tokio::spawn({
        let publisher = publisher.clone();
        let track_name = track_name.clone();
        async move {
            let mut group = 0usize;
            let mut frame = 0usize;
            publisher.start_group(&track_name).await?;
            loop {
                if next_group_rx.try_recv().is_ok() {
                    publisher.close_group(&track_name).await?;
                    publisher.start_group(&track_name).await?;
                    group += 1;
                    frame = 0;
                }
                let payload = format!("g{}-f{}", group, frame);
                publisher
                    .write_frame(&track_name, Bytes::from(payload.clone()))
                    .await?;
                let _ = written_tx.send(payload);
                frame += 1;
                tokio::time::sleep(frame_interval).await;
            }
            #[allow(unreachable_code)]
            anyhow::Ok(())
        }
    });

    // Wait until several frames of group 0 are already on the relay.
    let mut written = Vec::new();
    while written.len() < frames_before_join {
        match tokio::time::timeout(Duration::from_secs(10), written_rx.recv()).await? {
            Some(payload) => written.push(payload),
            None => anyhow::bail!("publisher task ended early"),
        }
    }
    assert_eq!(
        written.last().unwrap(),
        &format!("g0-f{}", frames_before_join - 1)
    );
    println!("publisher wrote {:?} before subscriber joined", written);

    // Now join.
    let subscriber = create_subscriber_with_options(
        &relay_url,
        &broadcast_name,
        vec![TrackDefinition::data(track_name.clone(), 0)],
        CatalogType::None,
        false,
    )
    .await?;
    let (data_tx, mut data_rx) = tokio::sync::mpsc::unbounded_channel();
    subscriber
        .set_data_callback(move |track, data| {
            let _ = data_tx.send((track, String::from_utf8_lossy(&data).to_string()));
        })
        .await?;

    let mut received = Vec::new();
    let first = tokio::time::timeout(Duration::from_secs(15), data_rx.recv())
        .await
        .map_err(|_| anyhow::anyhow!("subscriber never received a frame via {}", relay_url))?
        .ok_or_else(|| anyhow::anyhow!("data channel closed"))?;
    println!("subscriber's first frame: {:?}", first);
    assert_eq!(first.0, track_name);
    received.push(first.1);

    // Drain a few more frames of the open group, then roll to a new group.
    while received.len() < frames_before_join + 3 {
        let (_, payload) = tokio::time::timeout(Duration::from_secs(10), data_rx.recv())
            .await?
            .ok_or_else(|| anyhow::anyhow!("data channel closed"))?;
        received.push(payload);
    }
    next_group_tx.send(())?;
    let g1_first = loop {
        let (_, payload) = tokio::time::timeout(Duration::from_secs(10), data_rx.recv())
            .await?
            .ok_or_else(|| anyhow::anyhow!("data channel closed"))?;
        received.push(payload.clone());
        if payload.starts_with("g1-") {
            break payload;
        }
    };
    println!("subscriber received in order: {:?}", received);

    publish_task.abort();
    close_session(&subscriber).await?;
    close_session(&publisher).await?;

    // The open group must have been delivered from its very first frame,
    // and every frame of it in order, followed by the first frame of group 1.
    assert_eq!(
        received[0], "g0-f0",
        "open group was not delivered from its first frame"
    );
    for (index, payload) in received.iter().enumerate() {
        if payload.starts_with("g0-") {
            assert_eq!(
                payload,
                &format!("g0-f{}", index),
                "gap or reorder in open group"
            );
        }
    }
    assert_eq!(g1_first, "g1-f0");
    Ok(())
}
