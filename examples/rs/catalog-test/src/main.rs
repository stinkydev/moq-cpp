use anyhow::Result;
use clap::Parser;
use moq_lite::*;
use std::time::Duration;
use tokio::time::timeout;
use url::Url;

#[derive(Parser, Debug)]
#[command(author, version, about = "Test MOQ client that waits for 'peter' broadcast", long_about = None)]
struct Args {
    /// MoQ relay URL
    #[arg(short, long, default_value = "https://relay1.moq.sesame-streams.com:4433")]
    relay: Url,

    /// Broadcast namespace to monitor
    #[arg(short, long, default_value = "peter")]
    broadcast: String,

    /// Track name to subscribe to
    #[arg(short, long, default_value = "catalog.json")]
    track_name: String,

    /// The MoQ client configuration
    #[command(flatten)]
    client_config: moq_native::ClientConfig,

    /// The log configuration
    #[command(flatten)]
    log: moq_native::Log,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize tracing
    args.log.init();

    tracing::info!("Connecting to relay: {}", args.relay);
    tracing::info!("Waiting for broadcast: {}", args.broadcast);
    tracing::info!("Will subscribe to track: {}", args.track_name);

    // Create and connect client
    let client = args.client_config.init()?;

    tracing::info!("Connecting to MOQ server...");

    // Connect with timeout
    let session_result = timeout(Duration::from_secs(10), client.connect(args.relay.clone())).await;
    let session = match session_result {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            tracing::error!("Failed to connect to MOQ server: {}", e);
            return Err(e.into());
        }
        Err(_) => {
            tracing::error!("Connection to MOQ relay timed out");
            return Err(anyhow::anyhow!("Connection timeout"));
        }
    };

    tracing::info!("Successfully connected to MOQ server!");

    // Create origin for consuming
    let mut origin = Origin::produce();
    let _session = Session::connect(session, None, Some(origin.producer)).await?;

    tracing::info!("Waiting for '{}' broadcast to be announced...", args.broadcast);

    // Wait for the broadcast to be announced as active
    let broadcast_consumer = loop {
        let announcement = origin.consumer.announced().await;

        if let Some((path, consumer_opt)) = announcement {
            tracing::info!(
                "Received announcement: path='{}', consumer={}",
                path,
                if consumer_opt.is_some() { "active" } else { "inactive" }
            );

            if path.as_str() == args.broadcast {
                if let Some(consumer) = consumer_opt {
                    tracing::info!("Broadcast '{}' is now active!", args.broadcast);
                    break consumer;
                } else {
                    tracing::info!("Broadcast '{}' was deactivated", args.broadcast);
                }
            }
        }
    };

    // Subscribe to the catalog track
    tracing::info!("Subscribing to track '{}'", args.track_name);
    
    let track = Track {
        name: args.track_name.clone(),
        priority: 0,
    };
    
    let mut track_consumer = broadcast_consumer.subscribe_track(&track);

    tracing::info!("Subscribed to track, waiting for data...");

    // Wait for the first group
    let group_result = timeout(Duration::from_secs(10), track_consumer.next_group()).await;

    match group_result {
        Ok(Ok(Some(mut group))) => {
            tracing::info!("Received group, reading first frame...");

            // Read the first frame with timeout
            let frame_result = timeout(Duration::from_secs(5), group.read_frame()).await;

            match frame_result {
                Ok(Ok(Some(frame))) => {
                    tracing::info!("Successfully read frame! Size: {} bytes", frame.len());
                    
                    // Try to display as UTF-8 if possible
                    if let Ok(text) = String::from_utf8(frame.to_vec()) {
                        tracing::info!("Frame payload (as text): {}", &text[..text.len().min(500)]);
                    } else {
                        tracing::info!("Frame payload (first 100 bytes): {:?}", &frame[..frame.len().min(100)]);
                    }
                }
                Ok(Ok(None)) => {
                    tracing::warn!("No frame data available");
                }
                Ok(Err(e)) => {
                    tracing::error!("Error reading frame: {}", e);
                    return Err(e.into());
                }
                Err(_) => {
                    tracing::error!("Timeout waiting for frame");
                    return Err(anyhow::anyhow!("Frame read timeout"));
                }
            }
        }
        Ok(Ok(None)) => {
            tracing::warn!("No group available");
        }
        Ok(Err(e)) => {
            tracing::error!("Error getting group: {}", e);
            return Err(e.into());
        }
        Err(_) => {
            tracing::error!("Timeout waiting for group");
            return Err(anyhow::anyhow!("Group timeout"));
        }
    }

    tracing::info!("Test completed successfully, exiting");

    Ok(())
}
