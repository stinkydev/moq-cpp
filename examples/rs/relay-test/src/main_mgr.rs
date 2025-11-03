use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, atomic::{AtomicU64, Ordering}};
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use url::Url;

use moq_mgr::{Session, SessionConfig, SessionMode, SubscriptionConfig};

#[derive(Parser, Clone)]
pub struct Config {
    /// Connect to the given URL starting with https://
    #[arg(long, default_value = "https://relay.moq.sesame-streams.com:4433")]
    pub url: Url,

    /// The name of the broadcast to subscribe to.
    #[arg(long, default_value = "peter")]
    pub broadcast: String,

    /// Comma-separated list of tracks to subscribe to.
    #[arg(long, default_value = "video/hd,audio/data")]
    pub tracks: String,

    /// Local address to bind to (use 0.0.0.0:0 for IPv4)
    #[arg(long, default_value = "0.0.0.0:0")]
    pub bind: SocketAddr,

    /// The log configuration.
    #[command(flatten)]
    pub log: moq_native::Log,
}

/// Track statistics tracker
struct TrackStats {
    bytes_received: AtomicU64,
    messages_received: AtomicU64,
}

impl TrackStats {
    fn new() -> Self {
        Self {
            bytes_received: AtomicU64::new(0),
            messages_received: AtomicU64::new(0),
        }
    }

    fn add_data(&self, size: usize) {
        self.bytes_received.fetch_add(size as u64, Ordering::Relaxed);
        self.messages_received.fetch_add(1, Ordering::Relaxed);
    }

    fn get_bytes(&self) -> u64 {
        self.bytes_received.load(Ordering::Relaxed)
    }

    fn get_messages(&self) -> u64 {
        self.messages_received.load(Ordering::Relaxed)
    }
}

/// Main relay test application using MOQ Manager
struct RelayTestApp {
    config: Config,
    session: Option<Session>,
    track_stats: Arc<HashMap<String, Arc<TrackStats>>>,
    is_connected: bool,
}

impl RelayTestApp {
    fn new(config: Config) -> Self {
        Self {
            config,
            session: None,
            track_stats: Arc::new(HashMap::new()),
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

        // Create track stats map
        let mut stats_map = HashMap::new();
        for track_name in &track_names {
            stats_map.insert(track_name.clone(), Arc::new(TrackStats::new()));
        }
        self.track_stats = Arc::new(stats_map);

        // Add subscriptions with data callbacks
        for track_name in &track_names {
            let stats = self.track_stats.get(track_name).unwrap().clone();
            let track_name_clone = track_name.clone();
            
            let subscription = SubscriptionConfig {
                moq_track_name: track_name.clone(),
                data_callback: Arc::new(move |data: &[u8]| {
                    stats.add_data(data.len());
                    
                    // Print every 100 messages
                    let msg_count = stats.get_messages();
                    if msg_count % 100 == 0 {
                        println!(
                            "Track {}: {} messages, {} bytes",
                            track_name_clone,
                            msg_count,
                            stats.get_bytes()
                        );
                    }
                }),
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
        for (track_name, stats) in self.track_stats.iter() {
            println!(
                "  - {}: {} messages, {} bytes",
                track_name,
                stats.get_messages(),
                stats.get_bytes()
            );
        }
        println!("=============\n");
    }

    fn show_help(&self) {
        println!("\n=== Keyboard Controls ===");
        println!("c - Connect to relay");
        println!("d - Disconnect from relay");
        println!("s - Show status");
        println!("h - Show this help");
        println!("q - Quit application");
        println!("\nNote: With MOQ Manager, tracks are subscribed only when they appear in the catalog.");
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
    config.log.init();

    println!("MOQ Relay Test Application (using MOQ Manager)");
    println!("=============================================");
    println!("URL: {}", config.url);
    println!("Broadcast: {}", config.broadcast);
    println!("Tracks: {}", config.tracks);
    println!();

    let mut app = RelayTestApp::new(config);
    app.run().await
}
