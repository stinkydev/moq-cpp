use anyhow::{bail, Result};
use chrono::{SecondsFormat, Utc};
use clap::{Parser, Subcommand};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::time::{interval, timeout, Duration};
use tracing::{info, warn};

use moq_wrapper::{
    close_session, create_publisher, create_room_subscriber_with_options,
    create_subscriber_with_options, set_log_level, write_single_frame, CatalogType, Level,
    MoqSession, SessionEvent, TrackDefinition,
};

const DEFAULT_RELAY: &str = "https://r2.moq.sesame-streams.com:4433";
const DEFAULT_TRACK: &str = "clock";

#[derive(Parser)]
#[command(author, version, about = "MoQ clock example using moq-wrapper")]
struct Args {
    /// MoQ relay URL.
    #[arg(long, default_value = DEFAULT_RELAY)]
    url: String,

    /// Broadcast name, or room prefix when publishing multiple broadcasts.
    #[arg(long, default_value = "clock-rust")]
    broadcast: String,

    /// Track name.
    #[arg(long, default_value = DEFAULT_TRACK)]
    track: String,

    /// Catalog type to use (none, sesame, hang).
    #[arg(long, default_value = "none", value_parser = parse_catalog_type)]
    catalog: CatalogType,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Publish clock data.
    Publish {
        /// Number of publishers to start. Values greater than 1 publish under
        /// `<broadcast>/publisher-N`.
        #[arg(long, default_value_t = 1)]
        publishers: usize,

        /// Delay between clock frames for each publisher.
        #[arg(long, default_value_t = 1000)]
        interval_ms: u64,
    },

    /// Subscribe to clock data.
    Subscribe {
        /// Treat the broadcast argument as an announcement prefix.
        #[arg(long)]
        room: bool,

        /// Override the room prefix used with `--room`.
        #[arg(long)]
        room_prefix: Option<String>,

        /// Subscribe to every track listed in catalog.json.
        #[arg(long)]
        all_catalog_tracks: bool,
    },

    /// Run a self-contained catalog-track subscription smoke test.
    CatalogSmoke {
        /// Treat the broadcast argument as an announcement prefix.
        #[arg(long)]
        room: bool,

        /// Number of publishers to start. Room mode uses one broadcast per publisher.
        #[arg(long, default_value_t = 2)]
        publishers: usize,

        /// Maximum number of frames to send per publisher while waiting.
        #[arg(long, default_value_t = 20)]
        frames: usize,

        /// Delay between clock frames for each publisher.
        #[arg(long, default_value_t = 250)]
        interval_ms: u64,

        /// Timeout for the smoke test.
        #[arg(long, default_value_t = 15)]
        timeout_secs: u64,
    },
}

fn parse_catalog_type(s: &str) -> Result<CatalogType, String> {
    match s.to_lowercase().as_str() {
        "none" => Ok(CatalogType::None),
        "sesame" => Ok(CatalogType::Sesame),
        "hang" => Ok(CatalogType::Hang),
        _ => Err(format!(
            "Invalid catalog type: {s}. Valid options: none, sesame, hang"
        )),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    set_log_level(Level::INFO);

    match &args.command {
        Command::Publish {
            publishers,
            interval_ms,
        } => run_publisher(&args, *publishers, *interval_ms).await,
        Command::Subscribe {
            room,
            room_prefix,
            all_catalog_tracks,
        } => run_subscriber(&args, *room, room_prefix.as_deref(), *all_catalog_tracks).await,
        Command::CatalogSmoke {
            room,
            publishers,
            frames,
            interval_ms,
            timeout_secs,
        } => {
            run_catalog_smoke(
                &args,
                *room,
                *publishers,
                *frames,
                *interval_ms,
                *timeout_secs,
            )
            .await
        }
    }
}

async fn run_publisher(args: &Args, publishers: usize, interval_ms: u64) -> Result<()> {
    if publishers == 0 {
        bail!("--publishers must be at least 1");
    }

    let interval = Duration::from_millis(interval_ms.max(1));
    let track = TrackDefinition::data(args.track.clone(), 0);
    let mut sessions = Vec::new();
    let mut tasks = Vec::new();

    for broadcast in publisher_broadcasts(&args.broadcast, publishers) {
        let session = Arc::new(
            create_publisher(
                &args.url,
                &broadcast,
                vec![track.clone()],
                args.catalog.clone(),
            )
            .await?,
        );

        spawn_event_logger(session.clone(), broadcast.clone());

        let task = tokio::spawn(
            ClockPublisher::new(
                session.clone(),
                broadcast.clone(),
                args.track.clone(),
                interval,
            )
            .run(),
        );

        info!(
            "Publishing clock frames on broadcast '{}' track '{}'",
            broadcast, args.track
        );
        sessions.push(session);
        tasks.push(task);
    }

    info!("Press Ctrl+C to stop");
    tokio::signal::ctrl_c().await?;

    for session in &sessions {
        close_session(session).await?;
    }
    for task in tasks {
        task.abort();
    }

    Ok(())
}

async fn run_subscriber(
    args: &Args,
    room: bool,
    room_prefix: Option<&str>,
    all_catalog_tracks: bool,
) -> Result<()> {
    let catalog_type = effective_subscriber_catalog(&args.catalog, all_catalog_tracks);
    let tracks = if all_catalog_tracks {
        Vec::new()
    } else {
        vec![TrackDefinition::data(args.track.clone(), 0)]
    };

    let session = if room {
        let prefix = room_prefix.unwrap_or(&args.broadcast);
        info!(
            "Subscribing to room prefix '{}'{}",
            prefix,
            if all_catalog_tracks {
                " using catalog tracks"
            } else {
                " on configured track"
            }
        );
        create_room_subscriber_with_options(
            &args.url,
            prefix,
            tracks,
            catalog_type,
            all_catalog_tracks,
        )
        .await?
    } else {
        info!(
            "Subscribing to broadcast '{}'{}",
            args.broadcast,
            if all_catalog_tracks {
                " using catalog tracks"
            } else {
                " on configured track"
            }
        );
        create_subscriber_with_options(
            &args.url,
            &args.broadcast,
            tracks,
            catalog_type,
            all_catalog_tracks,
        )
        .await?
    };

    let session = Arc::new(session);
    spawn_event_logger(session.clone(), "subscriber".to_string());

    let state = Arc::new(std::sync::Mutex::new(ClockState::default()));
    session
        .set_data_callback({
            let state = state.clone();
            move |track: String, data: Vec<u8>| {
                state.lock().unwrap().process_frame(track, data);
            }
        })
        .await?;

    info!("Listening for clock data. Press Ctrl+C to stop");
    tokio::signal::ctrl_c().await?;

    close_session(&session).await?;
    Ok(())
}

async fn run_catalog_smoke(
    args: &Args,
    room: bool,
    publishers: usize,
    frames: usize,
    interval_ms: u64,
    timeout_secs: u64,
) -> Result<()> {
    if publishers == 0 {
        bail!("--publishers must be at least 1");
    }
    if frames == 0 {
        bail!("--frames must be at least 1");
    }

    let publisher_count = if room { publishers } else { 1 };
    let broadcasts = publisher_broadcasts(&args.broadcast, publisher_count);
    let track = TrackDefinition::data(args.track.clone(), 0);
    let mut publisher_sessions = Vec::new();

    for broadcast in &broadcasts {
        let session = Arc::new(
            create_publisher(
                &args.url,
                broadcast,
                vec![track.clone()],
                CatalogType::Sesame,
            )
            .await?,
        );
        spawn_event_logger(session.clone(), broadcast.clone());
        publisher_sessions.push((broadcast.clone(), session));
    }

    let subscriber = if room {
        create_room_subscriber_with_options(
            &args.url,
            &args.broadcast,
            Vec::new(),
            CatalogType::Sesame,
            true,
        )
        .await?
    } else {
        create_subscriber_with_options(
            &args.url,
            &args.broadcast,
            Vec::new(),
            CatalogType::Sesame,
            true,
        )
        .await?
    };
    let subscriber = Arc::new(subscriber);
    spawn_event_logger(subscriber.clone(), "catalog-smoke".to_string());

    let (data_tx, mut data_rx) = tokio::sync::mpsc::unbounded_channel();
    subscriber
        .set_data_callback(move |track: String, data: Vec<u8>| {
            let _ = data_tx.send((track, data));
        })
        .await?;

    let expected_tracks: HashSet<String> = if room {
        broadcasts
            .iter()
            .map(|broadcast| format!("{}/{}", broadcast, args.track))
            .collect()
    } else {
        HashSet::from([args.track.clone()])
    };

    info!(
        "Running catalog smoke test with {} publisher(s), {} mode, track '{}'",
        publisher_count,
        if room { "room" } else { "exact" },
        args.track
    );

    let publish_task = tokio::spawn({
        let publisher_sessions = publisher_sessions.clone();
        let track_name = args.track.clone();
        let publish_delay = Duration::from_millis(interval_ms.max(1));
        async move {
            let mut ticker = interval(publish_delay);
            for frame_index in 1..=frames {
                ticker.tick().await;
                for (broadcast, session) in &publisher_sessions {
                    let payload = format!("catalog-smoke {} {}", broadcast, frame_index);
                    write_single_frame(session, &track_name, payload.into_bytes()).await?;
                }
            }
            anyhow::Ok(())
        }
    });

    let observed = timeout(Duration::from_secs(timeout_secs.max(1)), async {
        let mut observed = HashSet::new();
        while observed.len() < expected_tracks.len() {
            if let Some((track, data)) = data_rx.recv().await {
                let payload = String::from_utf8_lossy(&data);
                info!("Catalog smoke frame on '{}': {}", track, payload);
                if expected_tracks.contains(&track) {
                    observed.insert(track);
                }
            }
        }
        observed
    })
    .await
    .map_err(|_| {
        anyhow::anyhow!(
            "timed out waiting for catalog tracks: {:?}",
            expected_tracks
        )
    })?;

    publish_task.abort();
    close_session(&subscriber).await?;
    for (_, session) in &publisher_sessions {
        close_session(session).await?;
    }

    info!("Catalog smoke test passed for tracks: {:?}", observed);
    Ok(())
}

fn publisher_broadcasts(base: &str, publishers: usize) -> Vec<String> {
    if publishers == 1 {
        return vec![base.to_string()];
    }

    let prefix = base.trim_end_matches('/');
    (1..=publishers)
        .map(|index| format!("{prefix}/publisher-{index}"))
        .collect()
}

fn effective_subscriber_catalog(catalog: &CatalogType, all_catalog_tracks: bool) -> CatalogType {
    if all_catalog_tracks && *catalog == CatalogType::None {
        CatalogType::Sesame
    } else {
        catalog.clone()
    }
}

fn spawn_event_logger(session: Arc<MoqSession>, label: String) {
    tokio::spawn(async move {
        while let Some(event) = session.next_event().await {
            match event {
                SessionEvent::Connected => info!("[{}] connected", label),
                SessionEvent::Disconnected { reason } => {
                    warn!("[{}] disconnected: {}", label, reason)
                }
                SessionEvent::BroadcastAnnounced { path } => {
                    info!("[{}] announced: {}", label, path)
                }
                SessionEvent::BroadcastUnannounced { path } => {
                    info!("[{}] unannounced: {}", label, path)
                }
                SessionEvent::TrackRequested { name } => {
                    info!("[{}] track requested: {}", label, name)
                }
                SessionEvent::Error { error } => warn!("[{}] error: {}", label, error),
            }
        }
    });
}

struct ClockPublisher {
    session: Arc<MoqSession>,
    broadcast: String,
    track: String,
    interval: Duration,
}

impl ClockPublisher {
    fn new(session: Arc<MoqSession>, broadcast: String, track: String, interval: Duration) -> Self {
        Self {
            session,
            broadcast,
            track,
            interval,
        }
    }

    async fn run(self) -> Result<()> {
        let mut ticker = interval(self.interval);

        loop {
            ticker.tick().await;

            let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
            let payload = format!("{} {}", self.broadcast, timestamp);

            if let Err(err) =
                write_single_frame(&self.session, &self.track, payload.clone().into_bytes()).await
            {
                warn!(
                    "Failed to publish on broadcast '{}' track '{}': {}",
                    self.broadcast, self.track, err
                );
                continue;
            }

            info!(
                "Published '{}' on broadcast '{}' track '{}'",
                payload, self.broadcast, self.track
            );
        }
    }
}

#[derive(Default)]
struct ClockState {
    frame_counts: HashMap<String, usize>,
}

impl ClockState {
    fn process_frame(&mut self, track: String, data: Vec<u8>) {
        let count = self.frame_counts.entry(track.clone()).or_insert(0);
        *count += 1;

        let payload = String::from_utf8_lossy(&data);
        info!("Frame #{} on '{}': {}", count, track, payload);
    }
}
