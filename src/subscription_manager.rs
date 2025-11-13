use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};
use tokio::time::sleep;
use tracing::{debug, info, warn};

use moq_lite::TrackConsumer;

use crate::catalog::{Catalog, CatalogType, TrackDefinition};
use crate::session::MoqSession;

/// Type alias for track data callback to reduce complexity
pub type TrackDataCallback = Arc<dyn Fn(String, Vec<u8>) + Send + Sync>;

/// Manages catalog and track subscriptions for a broadcast
/// This class handles the complete flow: Wait for announce -> Subscribe to catalog -> Parse catalog -> Subscribe to tracks
pub struct BroadcastSubscriptionManager {
    session: MoqSession,
    broadcast_name: String,
    catalog_type: CatalogType,
    requested_tracks: Vec<TrackDefinition>,

    // State management
    catalog_consumer: Arc<RwLock<Option<TrackConsumer>>>,
    track_consumers: Arc<RwLock<HashMap<String, TrackConsumer>>>,
    current_catalog: Arc<RwLock<Option<Catalog>>>,

    // Communication channels
    catalog_update_tx: broadcast::Sender<String>,
    track_data_callback: Arc<RwLock<Option<TrackDataCallback>>>,

    // State tracking
    is_active: Arc<RwLock<bool>>,
    catalog_subscribed: Arc<RwLock<bool>>,
}

impl BroadcastSubscriptionManager {
    /// Create a new subscription manager for a specific broadcast
    pub async fn new(
        session: MoqSession,
        broadcast_name: String,
        catalog_type: CatalogType,
        requested_tracks: Vec<TrackDefinition>,
    ) -> Result<Self> {
        let (catalog_update_tx, _) = broadcast::channel(10);

        let manager = Self {
            session: session.clone(),
            broadcast_name: broadcast_name.clone(),
            catalog_type,
            requested_tracks,
            catalog_consumer: Arc::new(RwLock::new(None)),
            track_consumers: Arc::new(RwLock::new(HashMap::new())),
            current_catalog: Arc::new(RwLock::new(None)),
            catalog_update_tx,
            track_data_callback: Arc::new(RwLock::new(None)),
            is_active: Arc::new(RwLock::new(false)),
            catalog_subscribed: Arc::new(RwLock::new(false)),
        };

        // Start the subscription management flow
        manager.start_subscription_flow().await;

        Ok(manager)
    }

    /// Set a callback to receive data from all tracks
    pub async fn set_data_callback<F>(&self, callback: F)
    where
        F: Fn(String, Vec<u8>) + Send + Sync + 'static,
    {
        *self.track_data_callback.write().await = Some(Arc::new(callback));
    }

    /// Start the complete subscription flow
    async fn start_subscription_flow(&self) {
        let session = self.session.clone();
        let broadcast_name = self.broadcast_name.clone();
        let catalog_type = self.catalog_type.clone();
        let requested_tracks = self.requested_tracks.clone();
        let catalog_consumer = self.catalog_consumer.clone();
        let track_consumers = self.track_consumers.clone();
        let current_catalog = self.current_catalog.clone();
        let catalog_update_tx = self.catalog_update_tx.clone();
        let track_data_callback = self.track_data_callback.clone();
        let is_active = self.is_active.clone();
        let catalog_subscribed = self.catalog_subscribed.clone();

        tokio::spawn(async move {
            info!(
                "[BroadcastSubscriptionManager] Starting subscription flow for broadcast: {}",
                broadcast_name
            );

            // Step 1: Wait for broadcast announcement
            Self::wait_for_announcement(&session, &broadcast_name).await;

            // Step 2: Subscribe to catalog if needed (only once per session)
            if catalog_type != CatalogType::None {
                let mut already_subscribed = catalog_subscribed.write().await;
                if !*already_subscribed {
                    *already_subscribed = true;
                    info!("[BroadcastSubscriptionManager] First-time catalog subscription for broadcast: {}", broadcast_name);
                    Self::manage_catalog_subscription(
                        &session,
                        &broadcast_name,
                        catalog_consumer.clone(),
                        current_catalog.clone(),
                        catalog_update_tx.clone(),
                    )
                    .await;
                } else {
                    info!("[BroadcastSubscriptionManager] Catalog already subscribed for broadcast: {}", broadcast_name);
                }
            }

            // Step 3: Subscribe to all requested tracks
            Self::manage_track_subscriptions(
                &session,
                &broadcast_name,
                &requested_tracks,
                track_consumers.clone(),
                track_data_callback.clone(),
                is_active.clone(),
            )
            .await;
        });
    }

    /// Wait for the broadcast to be announced
    async fn wait_for_announcement(session: &MoqSession, broadcast_name: &str) {
        info!(
            "[BroadcastSubscriptionManager] Waiting for broadcast announcement: {}",
            broadcast_name
        );

        let mut announcement_rx = session.subscribe_announcements();

        while let Ok(announced_broadcast) = announcement_rx.recv().await {
            if announced_broadcast == *broadcast_name {
                info!(
                    "[BroadcastSubscriptionManager] ✅ Broadcast '{}' announced",
                    broadcast_name
                );
                break;
            } else {
                debug!(
                    "[BroadcastSubscriptionManager] Ignoring announcement for: {}",
                    announced_broadcast
                );
            }
        }
    }

    /// Manage catalog subscription and updates
    async fn manage_catalog_subscription(
        session: &MoqSession,
        broadcast_name: &str,
        catalog_consumer: Arc<RwLock<Option<TrackConsumer>>>,
        current_catalog: Arc<RwLock<Option<Catalog>>>,
        catalog_update_tx: broadcast::Sender<String>,
    ) {
        info!(
            "[BroadcastSubscriptionManager] Subscribing to catalog for broadcast: {}",
            broadcast_name
        );

        // Subscribe to catalog.json - only once
        info!("[BroadcastSubscriptionManager] 🔄 ATTEMPTING catalog.json subscription for broadcast: {}", broadcast_name);
        match session
            .subscribe_track_internal(broadcast_name, "catalog.json")
            .await
        {
            Ok(mut track_consumer) => {
                info!("[BroadcastSubscriptionManager] ✅ SUCCESS: catalog.json subscription created for broadcast: {}", broadcast_name);
                *catalog_consumer.write().await = Some(track_consumer.clone());

                // Monitor catalog for updates
                tokio::spawn(async move {
                    while let Ok(Some(mut group)) = track_consumer.next_group().await {
                        if let Ok(Some(frame)) = group.read_frame().await {
                            let catalog_json = String::from_utf8_lossy(&frame).to_string();
                            info!(
                                "[BroadcastSubscriptionManager] 📋 Catalog updated ({} bytes)",
                                catalog_json.len()
                            );

                            // Parse and store the catalog
                            match Catalog::parse_sesame(&catalog_json) {
                                Ok(sesame_catalog) => {
                                    let catalog = Catalog::Sesame(sesame_catalog);
                                    *current_catalog.write().await = Some(catalog);
                                    info!("[BroadcastSubscriptionManager] ✅ Catalog parsed successfully");
                                }
                                Err(e) => {
                                    warn!("[BroadcastSubscriptionManager] ⚠️ Failed to parse catalog: {}", e);
                                }
                            }

                            // Broadcast catalog update
                            if let Err(e) = catalog_update_tx.send(catalog_json) {
                                debug!("[BroadcastSubscriptionManager] No listeners for catalog update: {}", e);
                            }
                        }
                    }

                    warn!("[BroadcastSubscriptionManager] Catalog stream ended");
                    *catalog_consumer.write().await = None;
                });
            }
            Err(e) => {
                warn!(
                    "[BroadcastSubscriptionManager] ❌ FAILED to subscribe to catalog for broadcast {}: {}",
                    broadcast_name, e
                );
            }
        }
    }

    /// Manage subscriptions to all requested tracks
    async fn manage_track_subscriptions(
        session: &MoqSession,
        broadcast_name: &str,
        requested_tracks: &[TrackDefinition],
        track_consumers: Arc<RwLock<HashMap<String, TrackConsumer>>>,
        track_data_callback: Arc<RwLock<Option<TrackDataCallback>>>,
        is_active: Arc<RwLock<bool>>,
    ) {
        info!(
            "[BroadcastSubscriptionManager] Subscribing to {} tracks",
            requested_tracks.len()
        );

        *is_active.write().await = true;

        for track_def in requested_tracks {
            let track_name = track_def.name.clone();
            let session_clone = session.clone();
            let broadcast_name_clone = broadcast_name.to_string();
            let track_consumers_clone = track_consumers.clone();
            let callback_clone = track_data_callback.clone();
            let is_active_clone = is_active.clone();

            tokio::spawn(async move {
                // Subscribe to the track
                match session_clone
                    .subscribe_track_internal(&broadcast_name_clone, &track_name)
                    .await
                {
                    Ok(mut track_consumer) => {
                        info!("[BroadcastSubscriptionManager] ✅ Successfully subscribed to track: {}", track_name);

                        // Store the consumer
                        track_consumers_clone
                            .write()
                            .await
                            .insert(track_name.clone(), track_consumer.clone());

                        // Monitor track data
                        while *is_active_clone.read().await {
                            match track_consumer.next_group().await {
                                Ok(Some(mut group)) => {
                                    while let Ok(Some(frame)) = group.read_frame().await {
                                        // Call the data callback if set
                                        let callback_guard = callback_clone.read().await;
                                        if let Some(callback) = callback_guard.as_ref() {
                                            callback(track_name.clone(), frame.to_vec());
                                        }
                                    }
                                }
                                Ok(None) => {
                                    info!(
                                        "[BroadcastSubscriptionManager] Track '{}' stream ended",
                                        track_name
                                    );
                                    break;
                                }
                                Err(e) => {
                                    warn!(
                                        "[BroadcastSubscriptionManager] Track '{}' error: {}",
                                        track_name, e
                                    );
                                    break;
                                }
                            }
                        }

                        // Remove from active consumers
                        track_consumers_clone.write().await.remove(&track_name);
                        info!(
                            "[BroadcastSubscriptionManager] Track '{}' subscription ended",
                            track_name
                        );
                    }
                    Err(e) => {
                        warn!("[BroadcastSubscriptionManager] ⚠️ Failed to subscribe to track '{}': {}", track_name, e);
                    }
                }
            });

            // Small delay between track subscriptions
            sleep(Duration::from_millis(100)).await;
        }
    }

    /// Get the current catalog
    pub async fn get_catalog(&self) -> Option<Catalog> {
        self.current_catalog.read().await.clone()
    }

    /// Get list of active track subscriptions
    pub async fn get_active_tracks(&self) -> Vec<String> {
        self.track_consumers.read().await.keys().cloned().collect()
    }

    /// Stop all subscriptions
    pub async fn stop(&self) {
        info!(
            "[BroadcastSubscriptionManager] Stopping all subscriptions for broadcast: {}",
            self.broadcast_name
        );

        *self.is_active.write().await = false;
        *self.catalog_subscribed.write().await = false;
        *self.catalog_consumer.write().await = None;
        self.track_consumers.write().await.clear();
        *self.current_catalog.write().await = None;
    }

    /// Check if the manager is actively managing subscriptions
    pub async fn is_active(&self) -> bool {
        *self.is_active.read().await
    }
}
