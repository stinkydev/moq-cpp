use anyhow::Result;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};
use tokio::time::sleep;
use tracing::{debug, info, warn};

use moq_native::moq_net::track::Subscriber as TrackConsumer;

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
    subscribe_all_catalog_tracks: bool,

    // State management
    catalog_consumer: Arc<RwLock<Option<TrackConsumer>>>,
    active_tracks: Arc<RwLock<HashSet<String>>>,
    current_catalog: Arc<RwLock<Option<Catalog>>>,

    // Communication channels
    catalog_update_tx: broadcast::Sender<String>,
    track_data_callback: Arc<RwLock<Option<TrackDataCallback>>>,

    // State tracking
    is_active: Arc<RwLock<bool>>,
    catalog_subscribed: Arc<RwLock<bool>>,
}

struct CatalogSubscriptionContext {
    catalog_type: CatalogType,
    subscribe_all_catalog_tracks: bool,
    current_catalog: Arc<RwLock<Option<Catalog>>>,
    catalog_update_tx: broadcast::Sender<String>,
    active_tracks: Arc<RwLock<HashSet<String>>>,
    track_data_callback: Arc<RwLock<Option<TrackDataCallback>>>,
    is_active: Arc<RwLock<bool>>,
}

impl BroadcastSubscriptionManager {
    /// Create a new subscription manager for a specific broadcast
    pub async fn new(
        session: MoqSession,
        broadcast_name: String,
        catalog_type: CatalogType,
        requested_tracks: Vec<TrackDefinition>,
        subscribe_all_catalog_tracks: bool,
    ) -> Result<Self> {
        let (catalog_update_tx, _) = broadcast::channel(10);

        let manager = Self {
            session: session.clone(),
            broadcast_name: broadcast_name.clone(),
            catalog_type,
            requested_tracks,
            subscribe_all_catalog_tracks,
            catalog_consumer: Arc::new(RwLock::new(None)),
            active_tracks: Arc::new(RwLock::new(HashSet::new())),
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
        info!("✅ Data callback set on BroadcastSubscriptionManager");
    }

    /// Get the current data callback (used for preserving callback during recreation)
    pub async fn get_data_callback(&self) -> Option<TrackDataCallback> {
        self.track_data_callback.read().await.clone()
    }

    /// Set data callback from Arc (used when recreating manager with existing callback)
    pub async fn set_data_callback_from_arc(&self, callback: TrackDataCallback) {
        *self.track_data_callback.write().await = Some(callback);
    }

    /// Get the catalog type (used for preserving configuration during recreation)
    pub fn get_catalog_type(&self) -> CatalogType {
        self.catalog_type.clone()
    }

    /// Get the requested tracks (used for preserving configuration during recreation)
    pub fn get_requested_tracks(&self) -> Vec<TrackDefinition> {
        self.requested_tracks.clone()
    }

    pub fn subscribe_all_catalog_tracks(&self) -> bool {
        self.subscribe_all_catalog_tracks
    }

    /// Start the complete subscription flow
    async fn start_subscription_flow(&self) {
        let session = self.session.clone();
        let broadcast_name = self.broadcast_name.clone();
        let catalog_type = self.catalog_type.clone();
        let requested_tracks = self.requested_tracks.clone();
        let subscribe_all_catalog_tracks = self.subscribe_all_catalog_tracks;
        let active_tracks = self.active_tracks.clone();
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
            *is_active.write().await = true;

            // Step 1: Subscribe to catalog if needed (broadcast is already announced and subscribed by session)
            if catalog_type != CatalogType::None {
                let mut already_subscribed = catalog_subscribed.write().await;
                if !*already_subscribed {
                    *already_subscribed = true;
                    info!("[BroadcastSubscriptionManager] First-time catalog subscription for broadcast: {}", broadcast_name);
                    let catalog_context = CatalogSubscriptionContext {
                        catalog_type: catalog_type.clone(),
                        subscribe_all_catalog_tracks,
                        current_catalog: current_catalog.clone(),
                        catalog_update_tx: catalog_update_tx.clone(),
                        active_tracks: active_tracks.clone(),
                        track_data_callback: track_data_callback.clone(),
                        is_active: is_active.clone(),
                    };
                    Self::manage_catalog_subscription(&session, &broadcast_name, catalog_context)
                        .await;
                } else {
                    info!("[BroadcastSubscriptionManager] Catalog already subscribed for broadcast: {}", broadcast_name);
                }
            }

            // Step 2: Subscribe to all requested tracks
            Self::manage_track_subscriptions(
                &session,
                &broadcast_name,
                &requested_tracks,
                active_tracks.clone(),
                track_data_callback.clone(),
                is_active.clone(),
            )
            .await;
        });
    }

    /// Manage catalog subscription and updates
    async fn manage_catalog_subscription(
        session: &MoqSession,
        broadcast_name: &str,
        context: CatalogSubscriptionContext,
    ) {
        info!(
            "[BroadcastSubscriptionManager] Subscribing to catalog for broadcast: {}",
            broadcast_name
        );

        // Subscribe to catalog.json - only once
        match session
            .subscribe_track_internal(broadcast_name, "catalog.json")
            .await
        {
            Ok(mut track_consumer) => {
                let session = session.clone();
                let broadcast_name = broadcast_name.to_string();
                let CatalogSubscriptionContext {
                    catalog_type,
                    subscribe_all_catalog_tracks,
                    current_catalog,
                    catalog_update_tx,
                    active_tracks,
                    track_data_callback,
                    is_active,
                } = context;

                // Monitor catalog for updates
                tokio::spawn(async move {
                    while *is_active.read().await {
                        let mut group = match track_consumer.next_group().await {
                            Ok(Some(group)) => group,
                            Ok(None) => break,
                            Err(e) => {
                                warn!(
                                    "[BroadcastSubscriptionManager] Catalog track error for broadcast {}: {}",
                                    broadcast_name, e
                                );
                                break;
                            }
                        };

                        if let Ok(Some(frame)) = group.read_frame().await {
                            let catalog_json = String::from_utf8_lossy(&frame.payload).to_string();
                            debug!(
                                "[BroadcastSubscriptionManager] 📋 Catalog updated ({} bytes)",
                                catalog_json.len()
                            );

                            // Parse and store the catalog
                            match Catalog::parse(&catalog_type, &catalog_json) {
                                Ok(Some(catalog)) => {
                                    let catalog_tracks = catalog.track_definitions();
                                    *current_catalog.write().await = Some(catalog);

                                    if subscribe_all_catalog_tracks {
                                        Self::manage_track_subscriptions(
                                            &session,
                                            &broadcast_name,
                                            &catalog_tracks,
                                            active_tracks.clone(),
                                            track_data_callback.clone(),
                                            is_active.clone(),
                                        )
                                        .await;
                                    }
                                }
                                Ok(None) => {
                                    debug!(
                                        "[BroadcastSubscriptionManager] Catalog parsing skipped for CatalogType::None"
                                    );
                                }
                                Err(e) => {
                                    warn!("[BroadcastSubscriptionManager] ⚠️ Failed to parse catalog: {}", e);
                                }
                            }

                            // Broadcast catalog update
                            if let Err(_e) = catalog_update_tx.send(catalog_json) {}
                        }
                    }
                });
            }
            Err(e) => {
                warn!(
                    "[BroadcastSubscriptionManager] FAILED to subscribe to catalog for broadcast {}: {}",
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
        active_tracks: Arc<RwLock<HashSet<String>>>,
        track_data_callback: Arc<RwLock<Option<TrackDataCallback>>>,
        is_active: Arc<RwLock<bool>>,
    ) {
        info!(
            "[BroadcastSubscriptionManager] Subscribing to {} tracks",
            requested_tracks.len()
        );

        for track_def in requested_tracks {
            let track_name = track_def.name.clone();

            if track_name == "catalog.json" {
                debug!(
                    "[BroadcastSubscriptionManager] Skipping catalog track in auto-subscribe set"
                );
                continue;
            }

            {
                let mut active_guard = active_tracks.write().await;
                if !active_guard.insert(track_name.clone()) {
                    debug!(
                        "[BroadcastSubscriptionManager] Track '{}' already subscribed or pending",
                        track_name
                    );
                    continue;
                }
            }

            let session_clone = session.clone();
            let broadcast_name_clone = broadcast_name.to_string();
            let active_tracks_clone = active_tracks.clone();
            let callback_clone = track_data_callback.clone();
            let is_active_clone = is_active.clone();

            tokio::spawn(async move {
                // Subscribe to the track
                match session_clone
                    .subscribe_track_internal(&broadcast_name_clone, &track_name)
                    .await
                {
                    Ok(mut track_consumer) => {
                        while *is_active_clone.read().await {
                            match track_consumer.next_group().await {
                                Ok(Some(mut group)) => {
                                    while let Ok(Some(frame)) = group.read_frame().await {
                                        // Call the data callback if set
                                        let callback_guard = callback_clone.read().await;
                                        if let Some(callback) = callback_guard.as_ref() {
                                            callback(track_name.clone(), frame.payload.to_vec());
                                        }
                                    }
                                }
                                Ok(None) => {
                                    info!(
                                        "[BroadcastSubscriptionManager] Track '{}' stream ended (no more groups)",
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
                        active_tracks_clone.write().await.remove(&track_name);
                        info!(
                            "[BroadcastSubscriptionManager] Track '{}' subscription ended",
                            track_name
                        );
                    }
                    Err(e) => {
                        active_tracks_clone.write().await.remove(&track_name);
                        warn!(
                            "[BroadcastSubscriptionManager] Failed to subscribe to track '{}': {}",
                            track_name, e
                        );
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
        self.active_tracks.read().await.iter().cloned().collect()
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
        self.active_tracks.write().await.clear();
        *self.current_catalog.write().await = None;
    }

    /// Check if the manager is actively managing subscriptions
    pub async fn is_active(&self) -> bool {
        *self.is_active.read().await
    }
}
