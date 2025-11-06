use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum WrapperError {
    #[error("Connection failed: {0}")]
    Connection(#[from] anyhow::Error),

    #[error("Session error: {0}")]
    Session(String),

    #[error("Broadcast not found: {0}")]
    BroadcastNotFound(String),

    #[error("Track not found: {0}")]
    TrackNotFound(String),

    #[error("Reconnection failed after {attempts} attempts")]
    ReconnectionFailed { attempts: usize },

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectionConfig {
    /// The MoQ relay URL to connect to
    pub url: url::Url,

    /// Maximum number of reconnection attempts (0 = infinite)
    pub max_reconnect_attempts: usize,

    /// Initial delay between reconnection attempts
    pub reconnect_delay: Duration,

    /// Maximum delay between reconnection attempts (for exponential backoff)
    pub max_reconnect_delay: Duration,

    /// Force IPv4-only connections (Windows compatibility)
    pub ipv4_only: bool,

    /// Client configuration for the underlying moq-native client
    pub client_config: moq_native::ClientConfig,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        let mut client_config = moq_native::ClientConfig::default();
        
        // Force IPv4 binding on Windows to avoid IPv6 issues
        #[cfg(windows)]
        {
            client_config.bind = "0.0.0.0:0".parse().expect("Valid IPv4 bind address");
        }
        
        Self {
            url: url::Url::parse("https://relay.moq.dev/anon").unwrap(),
            max_reconnect_attempts: 0, // Infinite reconnection attempts
            reconnect_delay: Duration::from_millis(500), // Faster initial reconnection
            max_reconnect_delay: Duration::from_secs(10), // Shorter max delay for better responsiveness
            ipv4_only: cfg!(windows), // Default to IPv4-only on Windows
            client_config,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SessionConfig {
    /// Name/path of the broadcast
    pub broadcast_name: String,

    /// Connection configuration
    pub connection: ConnectionConfig,

    /// Enable automatic reconnection
    pub auto_reconnect: bool,
}

impl SessionConfig {
    pub fn new(broadcast_name: impl Into<String>, url: url::Url) -> Self {
        Self {
            broadcast_name: broadcast_name.into(),
            connection: ConnectionConfig {
                url,
                ..Default::default()
            },
            auto_reconnect: true,
        }
    }
}
