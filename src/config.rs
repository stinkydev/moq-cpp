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

    #[error("Reconnection gave up: {0}")]
    ReconnectionFailed(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectionConfig {
    /// The MoQ relay URL to connect to
    pub url: url::Url,

    /// How long to keep retrying before giving up (0 = retry for as long as the session lives)
    pub reconnect_timeout: Duration,

    /// Initial delay between reconnection attempts
    pub reconnect_delay: Duration,

    /// Maximum delay between reconnection attempts (for exponential backoff)
    pub max_reconnect_delay: Duration,

    /// How long a subscribed broadcast stays announced after the connection drops.
    /// Zero unannounces it at once, and it is announced again once the reconnected
    /// relay has it. A longer window lets a reconnect splice in without consumers
    /// seeing a gap, but also keeps a closed session's broadcasts alive that long.
    pub broadcast_linger: Duration,

    /// Force IPv4-only connections (Windows compatibility)
    pub ipv4_only: bool,

    /// Timeout for each connection attempt (0 = the moq-native default)
    pub connect_timeout: Duration,

    /// Client configuration for the underlying moq-native client
    pub client_config: moq_native::ClientConfig,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        #[cfg(windows)]
        let client_config = {
            let mut client_config = moq_native::ClientConfig::default();
            client_config.bind = "0.0.0.0:0".parse().expect("Valid IPv4 bind address");
            client_config
        };

        #[cfg(not(windows))]
        let client_config = moq_native::ClientConfig::default();

        Self {
            url: url::Url::parse("https://relay.moq.dev/anon").unwrap(),
            reconnect_timeout: Duration::ZERO,
            reconnect_delay: Duration::from_millis(500),
            max_reconnect_delay: Duration::from_secs(10),
            broadcast_linger: Duration::ZERO,
            ipv4_only: cfg!(windows), // Default to IPv4-only on Windows
            connect_timeout: Duration::from_secs(5),
            client_config,
        }
    }
}

impl ConnectionConfig {
    /// The moq-native client configuration with this connection's dial timeout and
    /// reconnect backoff applied.
    pub fn resolved_client_config(&self) -> Result<moq_native::ClientConfig, WrapperError> {
        if self.reconnect_delay.is_zero() || self.max_reconnect_delay.is_zero() {
            return Err(WrapperError::InvalidConfig(
                "reconnect_delay and max_reconnect_delay must be non-zero".to_string(),
            ));
        }
        let mut client_config = self.client_config.clone();
        if !self.connect_timeout.is_zero() {
            client_config.timeout = Some(self.connect_timeout);
        }
        let mut backoff = moq_native::Backoff::default();
        backoff.initial = self.reconnect_delay;
        backoff.max = self.max_reconnect_delay.max(self.reconnect_delay);
        backoff.timeout = self.reconnect_timeout;
        client_config.backoff = backoff;
        Ok(client_config)
    }
}

#[derive(Clone, Debug)]
pub struct SessionConfig {
    /// Name/path of the broadcast
    pub broadcast_name: String,

    /// Connection configuration
    pub connection: ConnectionConfig,
}

impl SessionConfig {
    pub fn new(broadcast_name: impl Into<String>, url: url::Url) -> Self {
        Self {
            broadcast_name: broadcast_name.into(),
            connection: ConnectionConfig {
                url,
                ..Default::default()
            },
        }
    }
}
