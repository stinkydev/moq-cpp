use std::time::Duration;

use anyhow::{bail, Context};
use clap::{Parser, Subcommand};
use moq_native::moq_net::{self, Origin};
use url::Url;

mod clock;

const DEFAULT_RELAY: &str = "https://r2.moq.sesame-streams.com:4433";
const DEFAULT_TRACK: &str = "clock";

#[derive(Parser, Clone)]
pub struct Config {
    /// Connect to the given URL starting with https://.
    #[arg(long, default_value = DEFAULT_RELAY)]
    pub url: Url,

    /// Broadcast name, or room prefix when publishing multiple broadcasts.
    #[arg(long, default_value = "clock-native")]
    pub broadcast: String,

    /// The MoQ client configuration.
    #[command(flatten)]
    pub client: moq_native::ClientConfig,

    /// The name of the clock track.
    #[arg(long, default_value = DEFAULT_TRACK)]
    pub track: String,

    /// The log configuration.
    #[command(flatten)]
    pub log: moq_native::Log,

    /// Whether to publish the clock or consume it.
    #[command(subcommand)]
    pub role: Command,
}

#[derive(Subcommand, Clone)]
pub enum Command {
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
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::parse();
    config.log.init()?;

    let client = config.client.clone().init()?;
    tracing::info!(url = %config.url, "connecting to server");

    match &config.role {
        Command::Publish {
            publishers,
            interval_ms,
        } => publish(config.clone(), client, *publishers, *interval_ms).await,
        Command::Subscribe { room, room_prefix } => {
            subscribe(config.clone(), client, *room, room_prefix.clone()).await
        }
    }
}

async fn publish(
    config: Config,
    client: moq_native::Client,
    publishers: usize,
    interval_ms: u64,
) -> anyhow::Result<()> {
    if publishers == 0 {
        bail!("--publishers must be at least 1");
    }

    let origin = Origin::random().produce();
    let interval = Duration::from_millis(interval_ms.max(1));
    let mut broadcasts = Vec::new();
    let mut tasks = Vec::new();

    for broadcast_name in publisher_broadcasts(&config.broadcast, publishers) {
        let mut broadcast = origin
            .create_broadcast(&broadcast_name, moq_net::broadcast::Route::announced())
            .context("failed to create announced broadcast")?;
        let track = broadcast.create_track(config.track.as_str(), None)?;
        tasks.push(tokio::spawn(
            clock::Publisher::new(broadcast_name.clone(), track, interval).run(),
        ));
        tracing::info!(broadcast = %broadcast_name, track = %config.track, "publishing");
        broadcasts.push(broadcast);
    }

    let session = client.with_publisher(&origin).connect(config.url).await?;

    let closed = tokio::select! {
        err = session.closed() => Some(err),
        _ = tokio::signal::ctrl_c() => None,
    };

    for task in tasks {
        task.abort();
    }
    for mut broadcast in broadcasts {
        broadcast.finish();
    }

    if let Some(err) = closed {
        Err(err.into())
    } else {
        Ok(())
    }
}

async fn subscribe(
    config: Config,
    client: moq_native::Client,
    room: bool,
    room_prefix: Option<String>,
) -> anyhow::Result<()> {
    let prefix = if room {
        room_prefix.unwrap_or_else(|| config.broadcast.clone())
    } else {
        config.broadcast.clone()
    };

    let origin = Origin::random().produce();
    let path: moq_net::Path<'_> = prefix.as_str().into();
    let scoped_origin = origin
        .scope(&[path])
        .context("not allowed to consume broadcast prefix")?;
    let mut announcements = scoped_origin.consume().announced();
    let session = client
        .with_subscriber(scoped_origin.clone())
        .connect(config.url)
        .await?;

    tracing::info!(
        prefix = %prefix,
        exact = !room,
        track = %config.track,
        "waiting for announced broadcasts"
    );

    loop {
        tokio::select! {
            Some(update) = announcements.next() => {
                let broadcast_path = update.path.to_string();
                if !room && broadcast_path != config.broadcast {
                    continue;
                }

                match update.broadcast {
                    Some(broadcast) => {
                        tracing::info!(broadcast = %broadcast_path, "broadcast online");
                        let track = broadcast.track(&config.track)?.subscribe(None).await?;
                        tokio::spawn(clock::Subscriber::new(broadcast_path, track).run());
                    }
                    None => {
                        tracing::warn!(broadcast = %broadcast_path, "broadcast offline");
                    }
                }
            }
            res = session.closed() => return Err(res.into()),
            _ = tokio::signal::ctrl_c() => return Ok(()),
        }
    }
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
