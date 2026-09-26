//! A publisher and a subscriber keep working across a relay that goes away and
//! comes back on the same address. The relay is a minimal in-process one: every
//! session it accepts publishes into and subscribes from one shared origin.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use moq_native::moq_net::Origin;
use moq_wrapper::{CatalogType, ConnectionConfig, MoqSession, SessionConfig, TrackDefinition};

const BROADCAST: &str = "reconnect-test";
const TRACK: &str = "data";

/// A relay listening on `port` (0 picks one) until the returned handle is aborted.
async fn start_relay(port: u16) -> (u16, tokio::task::JoinHandle<()>) {
    let mut config = moq_native::ServerConfig::default();
    config.bind = Some(format!("127.0.0.1:{port}"));
    config.tls.generate = vec!["localhost".into()];
    let mut server = config.init().expect("failed to init relay");
    let port = server.local_addr().expect("relay has no address").port();

    let task = tokio::spawn(async move {
        let origin = Origin::random().produce();
        let mut sessions = Vec::new();
        while let Some(request) = server.accept().await {
            let request = request
                .with_publisher(origin.consume())
                .with_subscriber(origin.clone());
            if let Ok(session) = request.ok().await {
                sessions.push(session);
            }
        }
    });
    (port, task)
}

fn connection(port: u16, linger: Duration) -> SessionConfig {
    let mut client_config = ConnectionConfig::default().client_config;
    client_config.tls.disable_verify = Some(true);
    // A relay that vanishes without closing is noticed within a second.
    client_config.quic.idle_timeout = Some(Duration::from_secs(1));
    client_config.quic.keep_alive = Some(Duration::from_millis(250));
    let connection = ConnectionConfig {
        url: format!("https://127.0.0.1:{port}").parse().unwrap(),
        reconnect_delay: Duration::from_millis(200),
        max_reconnect_delay: Duration::from_millis(500),
        connect_timeout: Duration::from_secs(2),
        broadcast_linger: linger,
        client_config,
        ..Default::default()
    };
    SessionConfig {
        broadcast_name: BROADCAST.to_string(),
        connection,
    }
}

async fn wait_until(what: &str, within: Duration, mut check: impl AsyncFnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + within;
    while !check().await {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Publishes a frame with `payload` every 50 ms until the subscriber has seen it.
async fn deliver(publisher: &MoqSession, received: &Arc<Mutex<Vec<String>>>, payload: &str) {
    wait_until(
        &format!("'{payload}' to arrive"),
        Duration::from_secs(15),
        || async {
            if publisher.is_connected().await {
                let _ =
                    moq_wrapper::write_frame(publisher, TRACK, payload.as_bytes().to_vec(), true)
                        .await;
            }
            received.lock().unwrap().iter().any(|p| p == payload)
        },
    )
    .await;
}

async fn run(linger: Duration, outage: Duration) {
    let (port, relay) = start_relay(0).await;
    let tracks = vec![TrackDefinition::data(TRACK, 1)];

    let publisher = MoqSession::publisher(
        connection(port, linger),
        BROADCAST.to_string(),
        CatalogType::None,
        tracks.clone(),
    )
    .await
    .unwrap();
    publisher.start().await.unwrap();

    let subscriber = MoqSession::subscriber(
        connection(port, linger),
        BROADCAST.to_string(),
        CatalogType::None,
        tracks,
    )
    .await
    .unwrap();
    let received = Arc::new(Mutex::new(Vec::new()));
    let sink = received.clone();
    subscriber
        .set_data_callback(move |_track, data| {
            sink.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&data).into_owned());
        })
        .await
        .unwrap();
    subscriber.start().await.unwrap();

    wait_until(
        "both sessions to connect",
        Duration::from_secs(10),
        || async { publisher.is_connected().await && subscriber.is_connected().await },
    )
    .await;
    deliver(&publisher, &received, "before").await;

    // The relay dies without a word, as when the network goes down.
    relay.abort();
    let _ = relay.await;
    wait_until(
        "both sessions to notice",
        Duration::from_secs(10),
        || async { !publisher.is_connected().await && !subscriber.is_connected().await },
    )
    .await;
    tokio::time::sleep(outage).await;

    let (_, relay) = start_relay(port).await;
    wait_until(
        "both sessions to reconnect",
        Duration::from_secs(15),
        || async { publisher.is_connected().await && subscriber.is_connected().await },
    )
    .await;
    deliver(&publisher, &received, "after").await;

    assert!(publisher.connection_info().await.connection_attempts >= 2);
    assert!(subscriber.connection_info().await.last_error.is_none());

    publisher.close_session().await.unwrap();
    subscriber.close_session().await.unwrap();
    relay.abort();
}

/// A short outage: the subscriber's broadcast lingers and the reconnect splices in.
#[tokio::test(flavor = "multi_thread")]
async fn sessions_reconnect_after_a_short_relay_outage() {
    run(Duration::from_secs(15), Duration::from_millis(500)).await;
}

/// An outage longer than the linger: the broadcast is torn down and subscribed
/// again once the reconnected publisher announces it.
#[tokio::test(flavor = "multi_thread")]
async fn sessions_reconnect_after_an_outage_longer_than_the_linger() {
    run(Duration::from_millis(500), Duration::from_secs(3)).await;
}

/// The default: no linger, so the broadcast is torn down at once and subscribed
/// again after the reconnect.
#[tokio::test(flavor = "multi_thread")]
async fn sessions_reconnect_without_a_linger() {
    run(
        ConnectionConfig::default().broadcast_linger,
        Duration::from_millis(500),
    )
    .await;
}

/// A relay that is not up when the session starts is reached once it comes up.
#[tokio::test(flavor = "multi_thread")]
async fn a_publisher_created_before_its_relay_connects_when_the_relay_comes_up() {
    // Reserve a port, then free it so nothing listens there yet.
    let (port, relay) = start_relay(0).await;
    relay.abort();
    let _ = relay.await;

    let publisher = moq_wrapper::create_publisher_with_config(
        connection(port, Duration::from_secs(15)),
        BROADCAST,
        vec![TrackDefinition::data(TRACK, 1)],
        CatalogType::None,
    )
    .await
    .expect("an unreachable relay must not fail the session");
    assert!(!publisher.is_connected().await);

    let (_, relay) = start_relay(port).await;
    wait_until(
        "the publisher to connect",
        Duration::from_secs(15),
        || async { publisher.is_connected().await },
    )
    .await;

    publisher.close_session().await.unwrap();
    relay.abort();
}
