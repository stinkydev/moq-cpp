use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use tracing::{info, warn};

use moq_wrapper::{
    close_session, create_subscriber, set_data_callback, set_log_level, CatalogType, Level,
    SessionEvent, TrackDefinition, TrackType,
};

#[derive(Parser)]
#[command(author, version, about = "Simple MoQ hang subscriber example")]
struct Args {
    /// MoQ relay URL
    #[arg(long, default_value = "https://relay1.moq.sesame-streams.com:4433")]
    url: String,

    /// Broadcast name
    #[arg(long, default_value = "me")]
    broadcast: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging using the moq-wrapper set_log_level function
    set_log_level(Level::INFO);

    info!(
        "🎥 Starting hang subscriber for broadcast: {}",
        args.broadcast
    );

    // Create subscriber session with the tracks we want to subscribe to
    let tracks = vec![
        TrackDefinition::new("video/hd", 0, TrackType::Video),
        TrackDefinition::new("audio/data", 1, TrackType::Audio),
    ];
    let session = create_subscriber(&args.url, &args.broadcast, tracks, CatalogType::Hang).await?;
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

    // Set up data callback for every frame with sequence tracking
    let _ = set_data_callback(&session, {
        use std::sync::Mutex;
        let frame_counter = Arc::new(Mutex::new(std::collections::HashMap::<String, usize>::new()));

        move |track: String, data: Vec<u8>| {
            let mut counters = frame_counter.lock().unwrap();
            let count = counters.entry(track.clone()).or_insert(0);
            *count += 1;

            info!(
                "🎬 [{}] Frame #{} - size: {} bytes",
                track,
                count,
                data.len()
            );
        }
    })
    .await;

    info!("📥 Listening for data... Press Ctrl+C to stop");

    // Keep the main thread alive
    tokio::signal::ctrl_c().await?;
    info!("⏹️ Received shutdown signal");

    // Close the session
    close_session(&session).await?;
    info!("📥 Hang subscriber finished");
    Ok(())
}
