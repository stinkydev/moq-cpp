use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use tokio::sync::broadcast;
use tokio::time::timeout;
use url::Url;

use moq_lite::*;

#[derive(Parser, Clone)]
pub struct Config {
    /// Connect to the given URL starting with https://
    #[arg(long, default_value = "https://relay1.moq.sesame-streams.com:4433")]
    pub url: Url,

    /// The name of the broadcast to subscribe to.
    #[arg(long, default_value = "peter")]
    pub broadcast: String,

    /// Comma-separated list of tracks to subscribe to.
    #[arg(long, default_value = "video,audio")]
    pub tracks: String,

    /// The MoQ client configuration.
    #[command(flatten)]
    pub client: moq_native::ClientConfig,

    /// The log configuration.
    #[command(flatten)]
    pub log: moq_native::Log,
}

/// Track subscriber that handles receiving data from a specific track
struct TrackSubscriber {
    track_name: String,
    track_consumer: TrackConsumer,
    bytes_received: u64,
    shutdown_rx: broadcast::Receiver<()>,
}

impl TrackSubscriber {
    fn new(
        track_name: String,
        track_consumer: TrackConsumer,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> Self {
        Self {
            track_name,
            track_consumer,
            bytes_received: 0,
            shutdown_rx,
        }
    }

    async fn run(&mut self) -> Result<u64> {
        println!("Starting subscriber thread for track: {}", self.track_name);
        let mut group_count = 0u64;

        loop {
            tokio::select! {
                _ = self.shutdown_rx.recv() => {
                    println!("Track {} subscriber shutting down", self.track_name);
                    break;
                }
                group_result = timeout(Duration::from_millis(200), self.track_consumer.next_group()) => {
                    match group_result {
                        Ok(Ok(Some(mut group))) => {
                            group_count += 1;
                            let mut group_bytes = 0u64;
                            let mut frame_count = 0;

                            // Read all frames in the group
                            loop {
                                tokio::select! {
                                    _ = self.shutdown_rx.recv() => {
                                        println!("Track {} cancelled during frame reading", self.track_name);
                                        return Ok(self.bytes_received);
                                    }
                                    frame_result = timeout(Duration::from_millis(100), group.read_frame()) => {
                                        match frame_result {
                                            Ok(Ok(Some(frame_data))) => {
                                                let frame_size = frame_data.len() as u64;
                                                group_bytes += frame_size;
                                                frame_count += 1;
                                                self.bytes_received += frame_size;
                                            }
                                            Ok(Ok(None)) => {
                                                // No more frames in this group
                                                break;
                                            }
                                            Ok(Err(e)) => {
                                                tracing::error!("Error reading frame from track {}: {:?}", self.track_name, e);
                                                break;
                                            }
                                            Err(_) => {
                                                // Timeout - check for shutdown
                                                continue;
                                            }
                                        }
                                    }
                                }
                            }

                            println!(
                                "Track {}: Group {} - {} frames, {} bytes (total: {} bytes)",
                                self.track_name, group_count, frame_count, group_bytes, self.bytes_received
                            );
                        }
                        Ok(Ok(None)) => {
                            println!(
                                "Track {}: No more groups available (received {} groups total)",
                                self.track_name, group_count
                            );
                            break;
                        }
                        Ok(Err(e)) => {
                            tracing::error!("Error getting next group for track {}: {:?}", self.track_name, e);
                            if group_count == 0 {
                                // If we haven't received any data, assume no data available
                                println!("Track {}: No data available", self.track_name);
                                break;
                            }
                            // Sleep a bit before retrying
                            tokio::time::sleep(Duration::from_secs(1)).await;
                        }
                        Err(_) => {
                            // Timeout - check for shutdown and continue
                            continue;
                        }
                    }
                }
            }
        }

        println!(
            "Track {} subscriber finished. Groups: {}, Total bytes: {}",
            self.track_name, group_count, self.bytes_received
        );
        Ok(self.bytes_received)
    }
}

/// Main relay test application
struct RelayTestApp {
    config: Config,
    client: Option<moq_native::Client>,
    session: Option<Session<moq_native::web_transport_quinn::Session>>,
    broadcast_consumer: Option<BroadcastConsumer>,
    active_subscribers: HashMap<String, tokio::task::JoinHandle<Result<u64>>>,
    track_stats: HashMap<String, u64>, // Track name -> bytes received
    subscribed_tracks: Vec<String>,    // List of tracks we should be subscribed to
    shutdown_tx: broadcast::Sender<()>,
    is_connected: bool,
    auto_reconnect: bool,
}

impl RelayTestApp {
    fn new(config: Config) -> Self {
        let (shutdown_tx, _) = broadcast::channel(16);
        Self {
            config,
            client: None,
            session: None,
            broadcast_consumer: None,
            active_subscribers: HashMap::new(),
            track_stats: HashMap::new(),
            subscribed_tracks: Vec::new(),
            shutdown_tx,
            is_connected: false,
            auto_reconnect: true,
        }
    }

    async fn initialize(&mut self) -> Result<()> {
        self.config.log.init();

        // Override bind address to use IPv4 if not explicitly set
        let mut client_config = self.config.client.clone();
        if client_config.bind.to_string() == "[::]:0" {
            client_config.bind = "0.0.0.0:0".parse().unwrap();
        }

        let client = client_config.init()?;
        self.client = Some(client);
        println!("MOQ library initialized successfully");
        Ok(())
    }

    async fn connect_to_relay(&mut self) -> Result<()> {
        if self.is_connected {
            println!("Already connected to relay");
            return Ok(());
        }

        let client = self
            .client
            .as_ref()
            .context("Client not initialized")?
            .clone();

        println!("Connecting to: {}", self.config.url);

        // Timeout for connect (10 seconds)
        let session_result = timeout(
            Duration::from_secs(10),
            client.connect(self.config.url.clone()),
        )
        .await;
        let session = match session_result {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                println!("Failed to connect to MOQ server: {e}");
                return Ok(());
            }
            Err(_) => {
                println!("Connection to MOQ relay timed out");
                return Ok(());
            }
        };

        println!("Successfully connected to MOQ server!");

        // Create origin for consuming
        let origin = Origin::produce();
        let session = Session::connect(session, None, Some(origin.producer)).await?;

        // Give some time for the broadcast to be available
        println!("Waiting for broadcast to be available...");
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Consume the broadcast
        println!("Consuming broadcast: {}", self.config.broadcast);
        let broadcast_consumer = match origin.consumer.consume_broadcast(&self.config.broadcast) {
            Some(bc) => bc,
            None => {
                println!("Failed to consume broadcast (maybe no publisher available?)");
                return Ok(());
            }
        };

        println!("Successfully consuming broadcast!");

        self.session = Some(session);
        self.broadcast_consumer = Some(broadcast_consumer);
        self.is_connected = true;
        Ok(())
    }

    async fn disconnect_from_relay(&mut self) -> Result<()> {
        if !self.is_connected {
            println!("Not connected to relay");
            return Ok(());
        }

        println!("Disconnecting from relay...");

        // Stop all active subscribers
        self.unsubscribe_from_all_tracks().await?;

        // Close session and reset state
        self.session = None;
        self.broadcast_consumer = None;
        self.is_connected = false;

        println!("Disconnected from relay");
        Ok(())
    }

    async fn reconnect_with_subscriptions(&mut self) -> Result<()> {
        if self.is_connected {
            // First disconnect
            self.disconnect_from_relay().await?;
        }

        println!("Attempting to reconnect and restore subscriptions...");

        // Store the tracks we need to resubscribe to
        let tracks_to_restore = self.subscribed_tracks.clone();

        // Reconnect
        if let Err(e) = self.connect_to_relay().await {
            tracing::error!("Failed to reconnect: {:?}", e);
            return Ok(());
        }

        // Restore subscriptions
        for track_name in tracks_to_restore {
            println!("Restoring subscription to track: {}", track_name);
            if let Err(e) = self.subscribe_to_track(&track_name).await {
                tracing::error!("Failed to restore subscription to {}: {:?}", track_name, e);
            }
        }

        Ok(())
    }

    async fn monitor_session(&mut self) -> Result<()> {
        if let Some(session) = &self.session {
            let session_closed = session.closed();

            tokio::select! {
                result = session_closed => {
                    match result {
                        Ok(_) => {
                            println!("Session closed normally");
                        }
                        Err(e) => {
                            println!("Session terminated with error: {:?}", e);
                        }
                    }

                    if self.auto_reconnect && !self.subscribed_tracks.is_empty() {
                        println!("Auto-reconnecting...");
                        self.reconnect_with_subscriptions().await?;
                    } else {
                        self.is_connected = false;
                        self.session = None;
                        self.broadcast_consumer = None;
                        println!("Session lost - use 'c' to reconnect");
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    // Continue monitoring
                }
            }
        }
        Ok(())
    }

    async fn subscribe_to_track(&mut self, track_name: &str) -> Result<()> {
        if !self.is_connected {
            println!(
                "Not connected to relay. Cannot subscribe to track: {}",
                track_name
            );
            return Ok(());
        }

        if self.active_subscribers.contains_key(track_name) {
            println!("Already subscribed to track: {}", track_name);
            return Ok(());
        }

        let broadcast_consumer = self
            .broadcast_consumer
            .as_ref()
            .context("Broadcast consumer not available")?;

        println!("Subscribing to track: {}", track_name);

        let track = Track {
            name: track_name.to_string(),
            priority: 0,
        };

        let track_consumer = broadcast_consumer.subscribe_track(&track);
        println!("Successfully subscribed to track: {}", track_name);

        // Add to subscribed tracks list for auto-reconnect
        if !self.subscribed_tracks.contains(&track_name.to_string()) {
            self.subscribed_tracks.push(track_name.to_string());
        }

        // Create subscriber for this track
        let shutdown_rx = self.shutdown_tx.subscribe();
        let mut subscriber =
            TrackSubscriber::new(track_name.to_string(), track_consumer, shutdown_rx);

        // Start subscriber task
        let handle = tokio::spawn(async move { subscriber.run().await });

        // Store handle
        self.active_subscribers
            .insert(track_name.to_string(), handle);
        self.track_stats.insert(track_name.to_string(), 0);

        Ok(())
    }

    async fn unsubscribe_from_track(&mut self, track_name: &str) -> Result<()> {
        if let Some(handle) = self.active_subscribers.remove(track_name) {
            println!("Unsubscribing from track: {}", track_name);

            // Remove from subscribed tracks list
            self.subscribed_tracks.retain(|t| t != track_name);

            // Send shutdown signal
            let _ = self.shutdown_tx.send(());

            // Wait for task to complete and get final stats
            match handle.await {
                Ok(Ok(bytes)) => {
                    self.track_stats.insert(track_name.to_string(), bytes);
                    println!(
                        "Unsubscribed from track: {} (final: {} bytes)",
                        track_name, bytes
                    );
                }
                Ok(Err(e)) => {
                    tracing::error!("Error in subscriber for {}: {:?}", track_name, e);
                }
                Err(e) => {
                    tracing::error!("Task join error for {}: {:?}", track_name, e);
                }
            }

            self.track_stats.remove(track_name);
        } else {
            println!("Not subscribed to track: {}", track_name);
        }
        Ok(())
    }

    async fn unsubscribe_from_all_tracks(&mut self) -> Result<()> {
        if self.active_subscribers.is_empty() {
            return Ok(());
        }

        println!("Unsubscribing from all tracks...");

        // Clear subscribed tracks list
        self.subscribed_tracks.clear();

        // Send shutdown signal to all subscribers
        let _ = self.shutdown_tx.send(());

        // Wait for all tasks to complete
        let handles: Vec<_> = self.active_subscribers.drain().collect();
        for (track_name, handle) in handles {
            match handle.await {
                Ok(Ok(bytes)) => {
                    println!("Track {} finished with {} bytes", track_name, bytes);
                }
                Ok(Err(e)) => {
                    tracing::error!("Error in subscriber for {}: {:?}", track_name, e);
                }
                Err(e) => {
                    tracing::error!("Task join error for {}: {:?}", track_name, e);
                }
            }
        }

        self.track_stats.clear();
        println!("Unsubscribed from all tracks");
        Ok(())
    }

    fn show_status(&self) {
        println!("\n=== Status ===");
        println!(
            "Connected: {}",
            if self.is_connected { "YES" } else { "NO" }
        );
        println!(
            "Auto-reconnect: {}",
            if self.auto_reconnect { "ON" } else { "OFF" }
        );
        if self.is_connected {
            println!("URL: {}", self.config.url);
            println!("Broadcast: {}", self.config.broadcast);
        }
        println!("Active subscriptions: {}", self.active_subscribers.len());
        for track_name in self.active_subscribers.keys() {
            let bytes = self.track_stats.get(track_name).unwrap_or(&0);
            println!("  - {}: {} bytes", track_name, bytes);
        }
        if !self.subscribed_tracks.is_empty() {
            println!("Configured tracks: {}", self.subscribed_tracks.join(", "));
        }
        println!("=============\n");
    }

    fn show_help(&self) {
        println!("\n=== Keyboard Controls ===");
        println!("c - Connect to relay");
        println!("d - Disconnect from relay");
        println!("r - Reconnect and restore subscriptions");
        println!(
            "t - Toggle auto-reconnect (currently: {})",
            if self.auto_reconnect { "ON" } else { "OFF" }
        );
        println!("v - Subscribe to video track");
        println!("a - Subscribe to audio track");
        println!("V - Unsubscribe from video track");
        println!("A - Unsubscribe from audio track");
        println!("u - Unsubscribe from all tracks");
        println!("s - Show status");
        println!("h - Show this help");
        println!("q - Quit application");
        println!("Esc - Quit application");
        println!("Ctrl+C - Force quit");
        println!("========================\n");
    }

    async fn handle_keyboard_input(&mut self) -> Result<()> {
        self.show_help();

        enable_raw_mode()?;

        let mut running = true;
        while running {
            tokio::select! {
                // Monitor session for disconnects
                _ = self.monitor_session() => {
                    // Session monitoring will handle reconnects
                }

                // Handle keyboard input
                input_result = async {
                    if event::poll(Duration::from_millis(50))? {
                        if let Event::Key(key_event) = event::read()? {
                            return Ok::<Option<crossterm::event::KeyEvent>, std::io::Error>(Some(key_event));
                        }
                    }
                    Ok::<Option<crossterm::event::KeyEvent>, std::io::Error>(None)
                } => {
                    match input_result {
                        Ok(Some(key_event)) => {
                            // Handle Ctrl+C explicitly
                            if key_event.modifiers.contains(KeyModifiers::CONTROL) && key_event.code == KeyCode::Char('c') {
                                println!("Received Ctrl+C, quitting...");
                                running = false;
                                break;
                            }

                            // Only process key press events, not releases
                            if key_event.kind != crossterm::event::KeyEventKind::Press {
                                continue;
                            }

                            match key_event.code {
                                KeyCode::Char('c') | KeyCode::Char('C') => {
                                    if let Err(e) = self.connect_to_relay().await {
                                        tracing::error!("Failed to connect: {:?}", e);
                                    }
                                }
                                KeyCode::Char('d') | KeyCode::Char('D') => {
                                    if let Err(e) = self.disconnect_from_relay().await {
                                        tracing::error!("Failed to disconnect: {:?}", e);
                                    }
                                }
                                KeyCode::Char('r') | KeyCode::Char('R') => {
                                    if let Err(e) = self.reconnect_with_subscriptions().await {
                                        tracing::error!("Failed to reconnect: {:?}", e);
                                    }
                                }
                                KeyCode::Char('t') | KeyCode::Char('T') => {
                                    self.auto_reconnect = !self.auto_reconnect;
                                    println!("Auto-reconnect: {}", if self.auto_reconnect { "ON" } else { "OFF" });
                                }
                                KeyCode::Char('v') => {
                                    if let Err(e) = self.subscribe_to_track("video/hd").await {
                                        tracing::error!("Failed to subscribe to video: {:?}", e);
                                    }
                                }
                                KeyCode::Char('a') => {
                                    if let Err(e) = self.subscribe_to_track("audio/data").await {
                                        tracing::error!("Failed to subscribe to audio: {:?}", e);
                                    }
                                }
                                KeyCode::Char('V') => {
                                    if let Err(e) = self.unsubscribe_from_track("video/hd").await {
                                        tracing::error!("Failed to unsubscribe from video: {:?}", e);
                                    }
                                }
                                KeyCode::Char('A') => {
                                    if let Err(e) = self.unsubscribe_from_track("audio/data").await {
                                        tracing::error!("Failed to unsubscribe from audio: {:?}", e);
                                    }
                                }
                                KeyCode::Char('u') | KeyCode::Char('U') => {
                                    if let Err(e) = self.unsubscribe_from_all_tracks().await {
                                        tracing::error!("Failed to unsubscribe from all tracks: {:?}", e);
                                    }
                                }
                                KeyCode::Char('s') | KeyCode::Char('S') => {
                                    self.show_status();
                                }
                                KeyCode::Char('h') | KeyCode::Char('H') => {
                                    self.show_help();
                                }
                                KeyCode::Char('q') | KeyCode::Char('Q') => {
                                    println!("Quitting...");
                                    running = false;
                                }
                                KeyCode::Esc => {
                                    println!("Received Escape, quitting...");
                                    running = false;
                                }
                                _ => {
                                    // Ignore other keys
                                }
                            }
                        }
                        Ok(None) => {
                            // No input, continue
                        }
                        Err(e) => {
                            tracing::error!("Input error: {:?}", e);
                        }
                    }
                }
            }
        }

        disable_raw_mode()?;
        Ok(())
    }

    async fn run(&mut self) -> Result<()> {
        // Initialize the application
        self.initialize().await?;

        // Ensure we restore terminal state on any exit
        let _guard = TerminalGuard::new();

        // Start keyboard input handling
        let result = self.handle_keyboard_input().await;

        // Cleanup
        self.disconnect_from_relay().await?;

        result
    }
}

/// Guard to ensure terminal raw mode is disabled on drop
struct TerminalGuard;

impl TerminalGuard {
    fn new() -> Self {
        Self
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}

fn parse_tracks(tracks_str: &str) -> Vec<String> {
    tracks_str
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::parse();

    // Validate inputs
    if config.broadcast.is_empty() {
        anyhow::bail!("Broadcast name cannot be empty");
    }

    let track_names = parse_tracks(&config.tracks);
    if track_names.is_empty() {
        anyhow::bail!("At least one track must be specified");
    }

    println!("MOQ Relay Test Application (Rust Native)");
    println!("========================================");
    println!("URL: {}", config.url);
    println!("Broadcast: {}", config.broadcast);
    print!("Tracks: ");
    for (i, track) in track_names.iter().enumerate() {
        if i > 0 {
            print!(", ");
        }
        print!("{}", track);
    }
    println!("\n");

    // Create and run the test app
    let mut app = RelayTestApp::new(config);
    app.run().await?;

    Ok(())
}
