use anyhow::Result;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tracing::{debug, info, warn};

use moq_lite::{GroupConsumer, TrackConsumer};

use crate::session::MoqSession;

/// Type alias for data callback function
pub type DataCallback = Arc<dyn Fn(String, Vec<u8>) + Send + Sync>;

/// A resilient track consumer that automatically handles reconnections and broadcast announcements
#[derive(Clone)]
pub struct ResilientTrackConsumer {
    session: MoqSession,
    broadcast_name: String,
    track_name: String,
    current_consumer: Arc<RwLock<Option<TrackConsumer>>>,
}

impl ResilientTrackConsumer {
    pub async fn new(
        session: MoqSession,
        broadcast_name: String,
        track_name: String,
    ) -> Result<Self> {
        let resilient = Self {
            session: session.clone(),
            broadcast_name: broadcast_name.clone(),
            track_name: track_name.clone(),
            current_consumer: Arc::new(RwLock::new(None)),
        };

        // Start the simple subscription manager
        resilient.start_subscription_manager().await;

        Ok(resilient)
    }

    /// Start the simple subscription manager that follows: Connect -> Announce -> Subscribe -> Unannounce/Error -> Close -> Repeat
    /// Also listens for BroadcastAnnounced events to immediately reset on broadcaster reconnections
    async fn start_subscription_manager(&self) {
        let session = self.session.clone();
        let broadcast_name = self.broadcast_name.clone();
        let track_name = self.track_name.clone();
        let current_consumer = self.current_consumer.clone();

        // Start the subscription management task
        let subscription_task = {
            let session = session.clone();
            let broadcast_name = broadcast_name.clone();
            let track_name = track_name.clone();
            let current_consumer = current_consumer.clone();

            tokio::spawn(async move {
                info!(
                    "[ResilientTrackConsumer] Starting subscription manager for broadcast: {}",
                    broadcast_name
                );

                loop {
                    // Step 1: Wait for session to be connected
                    while !session.is_connected().await {
                        debug!("[ResilientTrackConsumer] Waiting for session connection...");
                        sleep(Duration::from_millis(500)).await;
                    }

                    // Step 2: Check if we have a consumer
                    let has_consumer = {
                        let consumer_guard = current_consumer.read().await;
                        consumer_guard.is_some()
                    };

                    if !has_consumer {
                        // Step 3: Try to subscribe when we don't have a consumer
                        match session
                            .subscribe_track_internal(&broadcast_name, &track_name)
                            .await
                        {
                            Ok(consumer) => {
                                info!(
                                    "[ResilientTrackConsumer] Successfully subscribed to track: {}",
                                    track_name
                                );
                                *current_consumer.write().await = Some(consumer);
                            }
                            Err(e) => {
                                debug!(
                                    "[ResilientTrackConsumer] Subscription failed (will retry): {}",
                                    e
                                );
                            }
                        }
                    }

                    // Step 4: Sleep before checking again
                    sleep(Duration::from_millis(1000)).await;
                }
            })
        };

        // Start the broadcast announcement listener task
        let announcement_task = {
            let session = session.clone();
            let broadcast_name = broadcast_name.clone();
            let current_consumer = current_consumer.clone();

            tokio::spawn(async move {
                info!(
                    "[ResilientTrackConsumer] Starting broadcast announcement listener for: {}",
                    broadcast_name
                );

                let mut announcement_rx = session.subscribe_announcements();

                loop {
                    match announcement_rx.recv().await {
                        Ok(announced_broadcast) => {
                            if announced_broadcast == broadcast_name {
                                info!("[ResilientTrackConsumer] Broadcast '{}' announced - resetting consumer for fresh connection", broadcast_name);

                                // Immediately clear the current consumer to force a new subscription
                                *current_consumer.write().await = None;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(skipped)) => {
                            warn!("[ResilientTrackConsumer] Announcement listener lagged, skipped {} messages", skipped);
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            debug!("[ResilientTrackConsumer] Announcement channel closed, exiting listener");
                            break;
                        }
                    }
                }
            })
        };

        // Don't wait for tasks to complete - they run indefinitely
        // Just spawn them and return
        std::mem::drop(subscription_task);
        std::mem::drop(announcement_task);
    }

    /// Get the next group, with automatic error handling that clears the consumer on errors
    pub async fn next_group(&self) -> Result<Option<GroupConsumer>> {
        loop {
            // Check if we have a consumer and call next_group directly without cloning
            let mut consumer_guard = self.current_consumer.write().await;

            if let Some(ref mut consumer) = *consumer_guard {
                match consumer.next_group().await {
                    Ok(Some(group)) => {
                        // Successfully got a group - consumer has advanced internally
                        return Ok(Some(group));
                    }
                    Ok(None) => {
                        // Stream ended normally - clear consumer and wait for reconnection
                        warn!("[ResilientTrackConsumer] Track stream ended (Ok(None)), clearing consumer");
                        *consumer_guard = None;
                        drop(consumer_guard); // Release lock before sleeping
                                              // Sleep briefly before retrying
                        sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                    Err(e) => {
                        // Error occurred - clear consumer and wait for reconnection
                        warn!(
                            "[ResilientTrackConsumer] Track stream error: {}, clearing consumer",
                            e
                        );
                        *consumer_guard = None;
                        drop(consumer_guard); // Release lock before sleeping
                                              // Sleep briefly before retrying
                        sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                }
            } else {
                drop(consumer_guard); // Release lock before sleeping
                                      // No consumer available, wait for the subscription manager to create one
                sleep(Duration::from_millis(100)).await;
                continue;
            }
        }
    }
}

/// Manages track subscriptions with callback-based data handling
pub struct SubscriptionManager {
    session: MoqSession,
    active_subscriptions: Arc<RwLock<HashSet<String>>>,
    data_callback: Arc<RwLock<Option<DataCallback>>>,
    background_tasks: Arc<RwLock<Vec<JoinHandle<()>>>>,
}

impl SubscriptionManager {
    pub fn new(session: MoqSession) -> Self {
        Self {
            session,
            active_subscriptions: Arc::new(RwLock::new(HashSet::new())),
            data_callback: Arc::new(RwLock::new(None)),
            background_tasks: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Set the data callback for receiving frame data
    pub async fn set_data_callback(&self, callback: DataCallback) {
        *self.data_callback.write().await = Some(callback);
    }

    /// Subscribe to a track with callback-based data handling
    pub async fn subscribe_track_with_callback(
        &self,
        broadcast_name: &str,
        track_name: &str,
    ) -> Result<ResilientTrackConsumer> {
        let subscription_key = format!("{}:{}", broadcast_name, track_name);

        // Check if already subscribed
        {
            let subscriptions = self.active_subscriptions.read().await;
            if subscriptions.contains(&subscription_key) {
                warn!("Already subscribed to track: {}", subscription_key);
                // Skip creating a new subscription - the existing one will handle reconnection
                info!("Skipping duplicate subscription for: {}", subscription_key);
                return Err(anyhow::anyhow!(
                    "Already subscribed to track: {}",
                    subscription_key
                ));
            }
        }

        // Add to active subscriptions
        {
            let mut subscriptions = self.active_subscriptions.write().await;
            subscriptions.insert(subscription_key.clone());
            info!(
                "📝 Added subscription: {} (total active: {})",
                subscription_key,
                subscriptions.len()
            );
        }

        // Create resilient consumer
        let resilient_consumer = ResilientTrackConsumer::new(
            self.session.clone(),
            broadcast_name.to_string(),
            track_name.to_string(),
        )
        .await?;

        // Start the callback processing task
        self.start_callback_task(track_name.to_string(), resilient_consumer.clone())
            .await;

        Ok(resilient_consumer)
    }

    /// Start a task to process frames and call the data callback
    async fn start_callback_task(
        &self,
        track_name: String,
        resilient_consumer: ResilientTrackConsumer,
    ) {
        let data_callback = self.data_callback.clone();

        let task_handle = tokio::spawn(async move {
            info!("Starting callback subscription for track: {}", track_name);

            let mut _frame_count = 0;

            loop {
                match resilient_consumer.next_group().await {
                    Ok(Some(mut group)) => {
                        loop {
                            match group.read_frame().await {
                                Ok(Some(frame_data)) => {
                                    _frame_count += 1;

                                    // Call the data callback if available
                                    let callback_guard = data_callback.read().await;
                                    if let Some(callback) = callback_guard.as_ref() {
                                        callback(track_name.clone(), frame_data.to_vec());
                                    }
                                }
                                Ok(None) => {
                                    // No more frames in this group
                                    break;
                                }
                                Err(e) => {
                                    warn!(
                                        "⚠️ Error reading frame from track {}: {}",
                                        track_name, e
                                    );
                                    break;
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        // This should not happen with ResilientTrackConsumer as it should handle reconnections
                        warn!(
                            "ResilientTrackConsumer returned None for track: {}",
                            track_name
                        );
                        sleep(Duration::from_millis(1000)).await;
                    }
                    Err(e) => {
                        // This should not happen with ResilientTrackConsumer as it should handle errors internally
                        warn!(
                            "ResilientTrackConsumer returned error for track {}: {}",
                            track_name, e
                        );
                        sleep(Duration::from_millis(1000)).await;
                    }
                }
            }
        });

        // Store the task handle for later cleanup
        self.background_tasks.write().await.push(task_handle);
    }

    /// Shutdown all background tasks and clean up resources
    pub async fn shutdown(&self) -> Result<()> {
        info!("🛑 Shutting down SubscriptionManager");

        // Clear active subscriptions
        self.active_subscriptions.write().await.clear();

        // Clear data callback
        *self.data_callback.write().await = None;

        // Cancel all background tasks
        let mut tasks = self.background_tasks.write().await;
        for task in tasks.drain(..) {
            task.abort();
        }

        info!("SubscriptionManager shutdown complete");
        Ok(())
    }

    /// Remove a subscription
    pub async fn remove_subscription(&self, broadcast_name: &str, track_name: &str) -> bool {
        let subscription_key = format!("{}:{}", broadcast_name, track_name);
        let mut subscriptions = self.active_subscriptions.write().await;
        let removed = subscriptions.remove(&subscription_key);
        if removed {
            info!(
                "🗑️ Removed subscription: {} (total active: {})",
                subscription_key,
                subscriptions.len()
            );
        }
        removed
    }

    /// Get active subscription count
    pub async fn active_subscription_count(&self) -> usize {
        self.active_subscriptions.read().await.len()
    }
}
