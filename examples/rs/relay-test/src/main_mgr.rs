use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, atomic::{AtomicU64, Ordering}};
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use url::Url;

use moq_mgr::{Session, SessionConfig, SessionMode, SubscriptionConfig};

mod sesame_protocol;

#[derive(Parser, Clone)]
pub struct Config {
    /// Connect to the given URL starting with https://
    #[arg(long, default_value = "https://relay.moq.sesame-streams.com:4433")]
    pub url: Url,

    /// The name of the broadcast to subscribe to.
    #[arg(long, default_value = "peter")]
    pub broadcast: String,

    /// Comma-separated list of tracks to subscribe to.
    #[arg(long, default_value = "video,audio")]
    pub tracks: String,

    /// Local address to bind to (use 0.0.0.0:0 for IPv4)
    #[arg(long, default_value = "0.0.0.0:0")]
    pub bind: SocketAddr,

    /// Enable Sesame Binary Protocol parsing
    #[arg(long)]
    pub parse_protocol: bool,

    /// Include logs from moq-lite/moq-native libraries (verbose)
    #[arg(long)]
    pub verbose_logging: bool,

    /// The log configuration.
    #[command(flatten)]
    pub log: moq_native::Log,
}

/// Track data handler with detailed packet analysis
struct TrackDataHandler {
    track_name: String,
    bytes_received: AtomicU64,
    groups_received: AtomicU64,
    keyframes_received: AtomicU64,
    start_time: Instant,
    parse_protocol: bool,
}

impl TrackDataHandler {
    fn new(track_name: String, parse_protocol: bool) -> Self {
        Self {
            track_name,
            bytes_received: AtomicU64::new(0),
            groups_received: AtomicU64::new(0),
            keyframes_received: AtomicU64::new(0),
            start_time: Instant::now(),
            parse_protocol,
        }
    }

    fn handle_data(&self, data: &[u8]) {
        let size = data.len();
        self.bytes_received.fetch_add(size as u64, Ordering::Relaxed);
        self.groups_received.fetch_add(1, Ordering::Relaxed);

        let packet_info = if self.parse_protocol {
            // Parse packet using Sesame Binary Protocol
            let parsed = sesame_protocol::BinaryProtocol::parse_data(data);
            
            if parsed.valid {
                let is_keyframe = (parsed.header.flags & sesame_protocol::FLAG_IS_KEYFRAME) != 0;
                
                if is_keyframe {
                    self.keyframes_received.fetch_add(1, Ordering::Relaxed);
                }
                
                // Build detailed packet info
                let mut info = String::new();
                info.push_str(" [");
                
                // Packet type
                let packet_type = sesame_protocol::PacketType::from(parsed.header.packet_type);
                info.push_str(&packet_type.to_string());
                
                // Keyframe status
                if is_keyframe {
                    info.push_str(", key");
                }
                
                // PTS - copy field to avoid unaligned access
                let pts = parsed.header.pts;
                info.push_str(&format!(", PTS:{}", pts));
                
                // Codec info if available
                if let Some(codec_data) = &parsed.codec_data {
                    info.push_str(", ");
                    let codec_type = sesame_protocol::CodecType::from(codec_data.codec_type);
                    info.push_str(&codec_type.to_string());
                    
                    // Add resolution for video - copy fields to avoid unaligned access
                    if matches!(packet_type, sesame_protocol::PacketType::VideoFrame) {
                        let width = codec_data.width;
                        let height = codec_data.height;
                        info.push_str(&format!(" {}x{}", width, height));
                    }
                    
                    // Add sample rate for audio - copy field to avoid unaligned access
                    if matches!(packet_type, sesame_protocol::PacketType::AudioFrame) {
                        let sample_rate = codec_data.sample_rate;
                        info.push_str(&format!(" {} hz", sample_rate));
                    }
                }
                
                // Payload info with first and last bytes
                info.push_str(&format!(", payload:{}", parsed.payload.len()));
                if !parsed.payload.is_empty() {
                    info.push_str(&format!(" [0x{:02x}", parsed.payload[0]));
                    if parsed.payload.len() > 1 {
                        info.push_str(&format!("...0x{:02x}", parsed.payload[parsed.payload.len() - 1]));
                    }
                    info.push_str("]");
                }
                
                info.push_str("]");
                info
            } else {
                " [INVALID PACKET]".to_string()
            }
        } else {
            // Simple raw data logging when protocol parsing is disabled
            let mut info = String::from(" [RAW DATA");
            if !data.is_empty() {
                info.push_str(&format!(", first:0x{:02x}", data[0]));
                if data.len() > 1 {
                    info.push_str(&format!(", last:0x{:02x}", data[data.len() - 1]));
                }
            }
            info.push_str("]");
            info
        };
        
        // Log packet information
        println!("Track {}: Size {} bytes{}", self.track_name, size, packet_info);
        
        let groups = self.groups_received.load(Ordering::Relaxed);
        let bytes = self.bytes_received.load(Ordering::Relaxed);
        
        // Log every 100 groups or 1MB of data
        if groups % 100 == 0 || bytes % (1024 * 1024) == 0 {
            let duration = self.start_time.elapsed().as_secs().max(1);
            let keyframes = self.keyframes_received.load(Ordering::Relaxed);
            
            println!(
                "Track {}: {} groups, {} keyframes, {} bytes (avg {} B/s)",
                self.track_name, groups, keyframes, bytes, bytes / duration
            );
        }
    }

    fn get_bytes_received(&self) -> u64 {
        self.bytes_received.load(Ordering::Relaxed)
    }

    fn get_groups_received(&self) -> u64 {
        self.groups_received.load(Ordering::Relaxed)
    }

    fn get_keyframes_received(&self) -> u64 {
        self.keyframes_received.load(Ordering::Relaxed)
    }
}

/// Main relay test application using MOQ Manager
struct RelayTestApp {
    config: Config,
    session: Option<Session>,
    track_handlers: Arc<HashMap<String, Arc<TrackDataHandler>>>,
    is_connected: bool,
}

impl RelayTestApp {
    fn new(config: Config) -> Self {
        Self {
            config,
            session: None,
            track_handlers: Arc::new(HashMap::new()),
            is_connected: false,
        }
    }

    async fn connect(&mut self) -> Result<()> {
        if self.is_connected {
            println!("Already connected");
            return Ok(());
        }

        println!("Connecting to: {}", self.config.url);
        println!("Broadcast: {}", self.config.broadcast);

        // Parse track names
        let track_names: Vec<String> = self
            .config
            .tracks
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        println!("Tracks: {}", track_names.join(", "));

        // Create session config
        let mut client_config = moq_native::ClientConfig::default();
        client_config.bind = self.config.bind;

        let session_config = SessionConfig {
            moq_server_url: self.config.url.clone(),
            moq_namespace: self.config.broadcast.clone(),
            reconnect_on_failure: true,
            client_config,
        };

        let session = Session::new(session_config, SessionMode::SubscribeOnly);

        // Set up callbacks
        session.set_error_callback(|error| {
            eprintln!("Session error: {}", error);
        });

        session.set_status_callback(|status| {
            println!("Session status: {}", status);
        });

        // Create track handlers map
        let mut handlers_map = HashMap::new();
        for track_name in &track_names {
            handlers_map.insert(
                track_name.clone(), 
                Arc::new(TrackDataHandler::new(track_name.clone(), self.config.parse_protocol))
            );
        }
        self.track_handlers = Arc::new(handlers_map);

        // Add subscriptions with data callbacks
        for track_name in &track_names {
            let handler = self.track_handlers.get(track_name).unwrap().clone();
            
            let subscription = SubscriptionConfig {
                moq_track_name: track_name.clone(),
                data_callback: Arc::new(move |data: &[u8]| {
                    handler.handle_data(data);
                }),
                reconnect_callback: None, // The session will provide its own reconnect callback for track consumers
            };

            session.add_subscription(subscription);
            println!("Added subscription for track: {}", track_name);
        }

        // Start the session (non-blocking)
        session.start().await?;

        self.session = Some(session);
        self.is_connected = true;

        println!("Session started successfully");
        println!("Note: Subscriptions will activate when tracks appear in catalog");
        
        Ok(())
    }

    fn disconnect(&mut self) {
        if !self.is_connected {
            println!("Not connected");
            return;
        }

        println!("Disconnecting...");
        
        if let Some(session) = self.session.take() {
            session.stop();
        }

        self.is_connected = false;
        println!("Disconnected");
    }

    fn show_status(&self) {
        println!("\n=== Status ===");
        println!("Connected: {}", self.is_connected);
        if self.is_connected {
            println!("URL: {}", self.config.url);
            println!("Broadcast: {}", self.config.broadcast);
            
            if let Some(session) = &self.session {
                println!("Session Running: {}", session.is_running());
            }
        }
        
        println!("Track Statistics:");
        for (track_name, handler) in self.track_handlers.iter() {
            println!(
                "  - {}: {} groups, {} keyframes, {} bytes",
                track_name,
                handler.get_groups_received(),
                handler.get_keyframes_received(),
                handler.get_bytes_received()
            );
        }
        println!("=============\n");
    }

    fn show_help(&self) {
        println!("\n=== Keyboard Controls ===");
        println!("c - Connect to relay (automatically subscribes to all configured tracks)");
        println!("d - Disconnect from relay");
        println!("s - Show status");
        println!("h - Show this help");
        println!("q - Quit application");
        println!("\nNote: With MOQ Manager, tracks are subscribed only when they appear in the catalog.");
        
        let track_names: Vec<String> = self.config.tracks
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        
        println!("Requested track subscriptions: {}", track_names.join(", "));
        println!("========================\n");
    }

    async fn run(&mut self) -> Result<()> {
        self.show_help();

        // Enable raw mode for keyboard input
        enable_raw_mode()?;

        let result = self.run_loop().await;

        // Cleanup
        disable_raw_mode()?;
        self.disconnect();

        result
    }

    async fn run_loop(&mut self) -> Result<()> {
        loop {
            // Check for keyboard events with timeout
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key_event) = event::read()? {
                    match key_event.code {
                        KeyCode::Char('c') | KeyCode::Char('C') => {
                            if let Err(e) = self.connect().await {
                                eprintln!("Failed to connect: {}", e);
                            }
                        }
                        KeyCode::Char('d') | KeyCode::Char('D') => {
                            self.disconnect();
                        }
                        KeyCode::Char('s') | KeyCode::Char('S') => {
                            self.show_status();
                        }
                        KeyCode::Char('h') | KeyCode::Char('H') => {
                            self.show_help();
                        }
                        KeyCode::Char('q') | KeyCode::Char('Q') => {
                            println!("Quitting...");
                            break;
                        }
                        _ => {}
                    }
                }
            }

            // Small delay to prevent busy waiting
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::parse();
    
    // Initialize logging based on verbose flag
    if config.verbose_logging {
        // More verbose logging that includes moq-lite/moq-native logs
        std::env::set_var("RUST_LOG", "debug");
    }
    config.log.init();

    println!("MOQ Relay Test Application (using MOQ Manager)");
    println!("=============================================");
    println!("URL: {}", config.url);
    println!("Broadcast: {}", config.broadcast);
    println!("Tracks: {}", config.tracks);
    println!("Bind Address: {}", config.bind);
    println!("Protocol Parsing: {}", if config.parse_protocol { "ENABLED" } else { "DISABLED" });
    println!("Verbose Logging: {}", if config.verbose_logging { "ENABLED" } else { "DISABLED" });
    println!();

    let mut app = RelayTestApp::new(config);
    app.run().await
}
