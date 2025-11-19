use anyhow::Result;
use bytes::Bytes;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::{interval, Instant};
use tracing::{debug, info, warn};

use moq_lite::{GroupProducer, Track, TrackConsumer, TrackProducer};

use crate::config::WrapperError;
use crate::session::MoqSession;

/// High-level wrapper for track management with automatic reconnection
pub struct TrackManager {
    session: Arc<MoqSession>,
    tracks: Arc<RwLock<HashMap<String, TrackHandle>>>,
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct TrackHandle {
    producer: Option<TrackProducer>,
    consumer: Option<TrackConsumer>,
    track_info: Track,
    last_activity: Instant,
}

impl TrackManager {
    pub fn new(session: Arc<MoqSession>) -> Self {
        Self {
            session,
            tracks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create and register a new track for publishing (deprecated - use session.add_track_definition instead)
    pub async fn create_publish_track(
        &self,
        _name: String,
        _priority: u32,
    ) -> Result<TrackProducer> {
        // This method is deprecated in favor of using MoqSession.add_track_definition
        // which properly manages tracks within the session's broadcast
        Err(anyhow::anyhow!(
            "create_publish_track is deprecated. Use MoqSession.add_track_definition instead"
        ))
    }

    /// Subscribe to a track
    pub async fn subscribe_track(
        &self,
        broadcast_name: &str,
        track_name: &str,
    ) -> Result<TrackConsumer> {
        let broadcast = self.session.get_broadcast_consumer().await?;
        let track_consumer = broadcast.subscribe_track(&Track {
            name: track_name.to_string(),
            priority: 0, // Priority doesn't matter for subscription
        });

        let handle = TrackHandle {
            producer: None,
            consumer: Some(track_consumer.clone()),
            track_info: Track {
                name: track_name.to_string(),
                priority: 0,
            },
            last_activity: Instant::now(),
        };

        self.tracks
            .write()
            .await
            .insert(track_name.to_string(), handle);

        info!(
            "Subscribed to track: {} in broadcast: {}",
            track_name, broadcast_name
        );
        Ok(track_consumer)
    }

    /// Get a track by name
    pub async fn get_track(&self, name: &str) -> Option<TrackHandle> {
        self.tracks.read().await.get(name).cloned()
    }

    /// Remove a track
    pub async fn remove_track(&self, name: &str) -> bool {
        self.tracks.write().await.remove(name).is_some()
    }

    /// Get all active tracks
    pub async fn list_tracks(&self) -> Vec<String> {
        self.tracks.read().await.keys().cloned().collect()
    }

    /// Start monitoring for track reconnection needs
    pub async fn start_monitoring(&self) {
        let tracks = self.tracks.clone();
        let session = self.session.clone();

        tokio::spawn(async move {
            let mut check_interval = interval(Duration::from_secs(30));

            loop {
                check_interval.tick().await;

                // Check if session is connected
                if !session.is_connected().await {
                    debug!("Session not connected, skipping track health check");
                    continue;
                }

                let track_handles: Vec<(String, TrackHandle)> =
                    { tracks.read().await.clone().into_iter().collect() };

                for (name, handle) in track_handles {
                    // Check if track needs reconnection (this is a simplified check)
                    let needs_reconnection =
                        handle.last_activity.elapsed() > Duration::from_secs(300);

                    if needs_reconnection {
                        warn!("Track {} may need reconnection", name);
                        // In a real implementation, you might implement more sophisticated
                        // health checking and automatic track recreation
                    }
                }
            }
        });
    }
}

/// A high-level helper for publishing data streams
pub struct StreamPublisher {
    track_producer: TrackProducer,
    current_group: Option<GroupProducer>,
    sequence_number: u64,
}

impl StreamPublisher {
    pub fn new(track_producer: TrackProducer) -> Self {
        Self {
            track_producer,
            current_group: None,
            sequence_number: 0,
        }
    }

    /// Start a new group (typically for keyframes or logical boundaries)
    pub fn start_group(&mut self) -> Result<()> {
        // Close the current group if it exists
        if let Some(group) = self.current_group.take() {
            group.close();
        }

        // Create a new group
        let group = self
            .track_producer
            .create_group(self.sequence_number.into())
            .ok_or_else(|| WrapperError::Session("Failed to create group".to_string()))?;

        self.current_group = Some(group);
        self.sequence_number += 1;

        debug!(
            "Started new group with sequence: {}",
            self.sequence_number - 1
        );
        Ok(())
    }

    /// Write a frame to the current group
    pub fn write_frame(&mut self, data: Bytes) -> Result<()> {
        let group = self.current_group.as_mut().ok_or_else(|| {
            WrapperError::Session("No active group, call start_group() first".to_string())
        })?;

        group.write_frame(data);
        Ok(())
    }

    /// Write a string frame (convenience method)
    pub fn write_string(&mut self, data: &str) -> Result<()> {
        self.write_frame(Bytes::from(data.to_string()))
    }

    /// Write a single frame and automatically manage the group
    pub fn write_single_frame(&mut self, data: Bytes) -> Result<()> {
        self.start_group()?;
        self.write_frame(data)?;
        self.close_group();
        Ok(())
    }

    /// Close the current group
    pub fn close_group(&mut self) {
        if let Some(group) = self.current_group.take() {
            group.close();
            debug!("Closed group");
        }
    }
}

impl Drop for StreamPublisher {
    fn drop(&mut self) {
        self.close_group();
    }
}
