use std::time::Duration;

use chrono::{SecondsFormat, Utc};
use moq_native::moq_net::{self, Timestamp};

pub struct Publisher {
    broadcast: String,
    track: moq_net::track::Producer,
    interval: Duration,
}

impl Publisher {
    pub fn new(
        broadcast: impl Into<String>,
        track: moq_net::track::Producer,
        interval: Duration,
    ) -> Self {
        Self {
            broadcast: broadcast.into(),
            track,
            interval,
        }
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        let mut ticker = tokio::time::interval(self.interval);

        loop {
            ticker.tick().await;

            let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
            let payload = format!("{} {}", self.broadcast, timestamp);
            self.track.write_frame(Timestamp::now(), payload.clone())?;
            tracing::info!(
                broadcast = %self.broadcast,
                track = "clock",
                payload = %payload,
                "published clock frame"
            );
        }
    }
}

pub struct Subscriber {
    broadcast: String,
    track: moq_net::track::Subscriber,
}

impl Subscriber {
    pub fn new(broadcast: impl Into<String>, track: moq_net::track::Subscriber) -> Self {
        Self {
            broadcast: broadcast.into(),
            track,
        }
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        let mut count = 0usize;

        while let Some(mut group) = self.track.next_group().await? {
            while let Some(frame) = group.read_frame().await? {
                count += 1;
                let payload = String::from_utf8_lossy(&frame.payload);
                println!("[{}] frame #{}: {}", self.broadcast, count, payload);
            }
        }

        Ok(())
    }
}
