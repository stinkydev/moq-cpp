use anyhow::Result;
use chrono::{Timelike, Utc};
use clap::{Parser, Subcommand};
use std::sync::Arc;
use tokio::time::sleep;
use tracing::{info, warn};

use moq_wrapper::{
    close_session, create_publisher, create_subscriber, set_log_level, write_frame, CatalogType,
    Level, MoqSession, SessionEvent, TrackDefinition, TrackType,
};

#[derive(Parser)]
#[command(author, version, about = "MoQ Clock example using moq-wrapper")]
struct Args {
    /// MoQ relay URL
    #[arg(long, default_value = "https://r1.moq.sesame-streams.com:4433")]
    url: String,

    /// Broadcast name
    #[arg(long, default_value = "peter2")]
    broadcast: String,

    /// Track name
    #[arg(long, default_value = "video")]
    track: String,

    /// Catalog type to use (none, sesame, hang)
    #[arg(long, default_value = "sesame", value_parser = parse_catalog_type)]
    catalog: CatalogType,

    #[command(subcommand)]
    command: Command,
}

fn parse_catalog_type(s: &str) -> Result<CatalogType, String> {
    match s.to_lowercase().as_str() {
        "none" => Ok(CatalogType::None),
        "sesame" => Ok(CatalogType::Sesame),
        "hang" => Ok(CatalogType::Hang),
        _ => Err(format!(
            "Invalid catalog type: {}. Valid options: none, sesame, hang",
            s
        )),
    }
}

#[derive(Subcommand)]
enum Command {
    /// Publish clock data
    Publish,
    /// Subscribe to clock data
    Subscribe,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging using the moq-wrapper set_log_level function
    set_log_level(Level::DEBUG);

    match args.command {
        Command::Publish => run_publisher(args).await,
        Command::Subscribe => run_subscriber(args).await,
    }
}

async fn run_publisher(args: Args) -> Result<()> {
    info!("🕐 Starting MoQ clock publisher");

    // Create tracks to publish
    let tracks = vec![TrackDefinition::data(args.track.clone(), 0)];

    // Create publisher session with tracks and specified catalog type
    let session =
        create_publisher(&args.url, &args.broadcast, tracks, args.catalog.clone()).await?;

    // Monitor events
    let session = Arc::new(session);
    let event_session = session.clone();
    tokio::spawn(async move {
        while let Some(event) = event_session.next_event().await {
            match event {
                SessionEvent::Connected => info!("✅ Connected to relay"),
                SessionEvent::Disconnected { reason } => warn!("❌ Disconnected: {}", reason),
                SessionEvent::Error { error } => warn!("🚨 Error: {}", error),
                _ => {}
            }
        }
    });

    info!(
        "📡 Publishing clock data on track: {} (catalog: {:?})",
        args.track, args.catalog
    );

    // Start clock publisher
    let mut clock_publisher = ClockPublisher::new(session, args.track.clone());
    clock_publisher.run().await?;

    Ok(())
}

async fn run_subscriber(args: Args) -> Result<()> {
    info!("🕐 Starting MoQ clock subscriber");

    // Create subscriber session with no specific tracks (will subscribe manually)
    let tracks = vec![]; // We'll subscribe manually to tracks
    let session =
        create_subscriber(&args.url, &args.broadcast, tracks, args.catalog.clone()).await?;
    let session = Arc::new(session);

    // Monitor events
    let session_clone = session.clone();
    tokio::spawn(async move {
        while let Some(event) = session_clone.next_event().await {
            match event {
                SessionEvent::Connected => info!("✅ Connected to relay"),
                SessionEvent::Disconnected { reason } => warn!("❌ Disconnected: {}", reason),
                SessionEvent::BroadcastAnnounced { path } => {
                    info!("📢 Broadcast announced: {}", path)
                }
                SessionEvent::Error { error } => warn!("🚨 Error: {}", error),
                _ => {}
            }
        }
    });

    info!(
        "🎯 Subscribing to track: {} (catalog: {:?})",
        args.track, args.catalog
    );

    // Set up a clock display callback using the new auto-subscription method
    let track_name = args.track.clone();
    let _ = session
        .set_auto_subscription_data_callback({
            use std::sync::Mutex;
            let state = Arc::new(Mutex::new(ClockState::new()));

            move |track: String, data: Vec<u8>| {
                if track == track_name {
                    let data_str = String::from_utf8_lossy(&data).to_string();
                    let mut clock_state = state.lock().unwrap();
                    clock_state.process_frame(data_str);
                }
            }
        })
        .await;

    // Enable auto-subscription with the new BroadcastSubscriptionManager
    let track_def = TrackDefinition {
        name: args.track.clone(),
        priority: 0,
        track_type: TrackType::Data,
    };

    session
        .enable_auto_subscription(args.broadcast.clone(), args.catalog, vec![track_def])
        .await?;
    info!("🎯 Enabled auto-subscription for track: {}", args.track);

    info!("📥 Listening for clock data... Press Ctrl+C to stop");

    // Keep the main thread alive
    tokio::signal::ctrl_c().await?;
    info!("⏹️ Received shutdown signal");

    // Close the session
    close_session(&session).await?;
    info!("📥 Clock subscriber finished");
    Ok(())
}

/// Clock publisher that sends time data similar to the original moq-clock example
struct ClockPublisher {
    session: Arc<MoqSession>,
    track_name: String,
}

impl ClockPublisher {
    fn new(session: Arc<MoqSession>, track_name: String) -> Self {
        Self {
            session,
            track_name,
        }
    }

    async fn run(&mut self) -> Result<()> {
        let start = Utc::now();
        let mut now = start;

        // Just for fun, don't start at zero (like original moq-clock)
        let mut sequence = start.minute() as u64;

        loop {
            info!("📤 Starting minute segment #{}", sequence);

            // Send the base timestamp (everything except seconds) as first frame of new group
            let base = now.format("%Y-%m-%d %H:%M:").to_string();
            match write_frame(
                &self.session,
                &self.track_name,
                base.clone().into_bytes(),
                true,
            )
            .await
            {
                Ok(_) => {
                    info!("📅 Sent base time: {}", base);
                }
                Err(e) => {
                    warn!(
                        "⏳ Failed to send base time (will retry next minute): {}",
                        e
                    );
                    // Continue anyway, we'll try again next minute
                }
            }

            let mut seconds_count = 0;

            // Send individual seconds for this minute (like original moq-clock)
            loop {
                let seconds = now.format("%S").to_string();

                // Try to write the frame, but handle all errors gracefully
                match write_frame(&self.session, &self.track_name, seconds.into_bytes(), false)
                    .await
                {
                    Ok(_) => {
                        seconds_count += 1;
                    }
                    Err(e) => {
                        warn!("⏳ Failed to write frame (skipping): {}", e);
                        // Don't increment seconds_count, just continue to next second
                    }
                }

                let next = now + chrono::Duration::try_seconds(1).unwrap();
                let next = next.with_nanosecond(0).unwrap();

                let delay = (next - now).to_std().unwrap();
                sleep(delay).await;

                // Get the current time again to check if we overslept
                let next = Utc::now();
                if next.minute() != now.minute() {
                    info!(
                        "📦 Minute changed, finishing group #{} with {} seconds",
                        sequence, seconds_count
                    );
                    break;
                }

                now = next;
            }

            // Close the group for this minute
            match self.session.close_group(&self.track_name).await {
                Ok(_) => {
                    info!(
                        "📦 Closed group #{} with {} seconds",
                        sequence, seconds_count
                    );
                }
                Err(e) => {
                    warn!(
                        "⏳ Failed to close group (will be cleaned up automatically): {}",
                        e
                    );
                    // Continue anyway, group will be cleaned up on reconnection or next group
                }
            }

            sequence += 1;
            now = Utc::now(); // just assume we didn't undersleep (like original)
        }
    }
}

/// Clock state for assembling time display from frames
struct ClockState {
    base_time: Option<String>,
    frame_count: usize,
}

impl ClockState {
    fn new() -> Self {
        Self {
            base_time: None,
            frame_count: 0,
        }
    }

    fn process_frame(&mut self, data: String) {
        self.frame_count += 1;

        if self.frame_count == 1 {
            // First frame is the base timestamp (everything except seconds)
            self.base_time = Some(data.clone());
            info!("📅 [ClockState] Base time: {}", data);
        } else if let Some(ref base) = self.base_time {
            // Subsequent frames are seconds
            if let Ok(seconds) = data.parse::<u32>() {
                // Create clock emoji display
                let clock_emojis = [
                    "🕛", "🕐", "🕑", "🕒", "🕓", "🕔", "🕕", "🕖", "🕗", "🕘", "🕙", "🕚",
                ];
                let clock_index = ((seconds as f64 / 60.0) * clock_emojis.len() as f64) as usize
                    % clock_emojis.len();
                let clock_emoji = clock_emojis[clock_index];

                println!("{} {}{:02}", clock_emoji, base, seconds);
            }
        }
    }
}
