use anyhow::{Context, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use url::Url;

use moq_lite::*;
use moq_native::{Client, ClientConfig};

use crate::catalog::CatalogProcessor;
use crate::consumer::{Consumer, SubscriptionConfig};
use crate::producer::{BroadcastConfig, Producer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionMode {
    PublishOnly,
    SubscribeOnly,
    PublishAndSubscribe,
}

#[derive(Clone)]
pub struct SessionConfig {
    pub moq_server_url: Url,
    pub moq_namespace: String,
    pub reconnect_on_failure: bool,
    pub client_config: ClientConfig,
}

pub struct Session {
    config: SessionConfig,
    mode: SessionMode,
    client: Arc<RwLock<Option<Client>>>,
    session: Arc<RwLock<Option<Arc<moq_lite::Session<moq_native::web_transport_quinn::Session>>>>>,
    broadcast_consumer: Arc<RwLock<Option<Arc<BroadcastConsumer>>>>,
    broadcast_producer: Arc<RwLock<Option<Arc<BroadcastProducer>>>>,
    
    // For consumers
    catalog_processor: Arc<RwLock<Option<CatalogProcessor>>>,
    catalog_consumer: Arc<RwLock<Option<Consumer>>>,
    active_consumers: Arc<RwLock<HashMap<String, Consumer>>>,
    requested_subscriptions: Arc<RwLock<HashMap<String, SubscriptionConfig>>>,
    
    // For producers
    active_producers: Arc<RwLock<HashMap<String, Producer>>>,
    broadcast_configs: Arc<RwLock<Vec<BroadcastConfig>>>,
    
    // Control
    shutdown_tx: broadcast::Sender<()>,
    is_connected: Arc<RwLock<bool>>,
    
    // Callbacks
    error_callback: Arc<RwLock<Option<Box<dyn Fn(&str) + Send + Sync>>>>,
    status_callback: Arc<RwLock<Option<Box<dyn Fn(&str) + Send + Sync>>>>,
}

impl Clone for Session {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            mode: self.mode,
            client: self.client.clone(),
            session: self.session.clone(),
            broadcast_consumer: self.broadcast_consumer.clone(),
            broadcast_producer: self.broadcast_producer.clone(),
            catalog_processor: self.catalog_processor.clone(),
            catalog_consumer: self.catalog_consumer.clone(),
            active_consumers: self.active_consumers.clone(),
            requested_subscriptions: self.requested_subscriptions.clone(),
            active_producers: self.active_producers.clone(),
            broadcast_configs: self.broadcast_configs.clone(),
            shutdown_tx: self.shutdown_tx.clone(),
            is_connected: self.is_connected.clone(),
            error_callback: self.error_callback.clone(),
            status_callback: self.status_callback.clone(),
        }
    }
}

impl Session {
    pub fn new(config: SessionConfig, mode: SessionMode) -> Self {
        let (shutdown_tx, _) = broadcast::channel(16);
        
        Self {
            config,
            mode,
            client: Arc::new(RwLock::new(None)),
            session: Arc::new(RwLock::new(None)),
            broadcast_consumer: Arc::new(RwLock::new(None)),
            broadcast_producer: Arc::new(RwLock::new(None)),
            catalog_processor: Arc::new(RwLock::new(None)),
            catalog_consumer: Arc::new(RwLock::new(None)),
            active_consumers: Arc::new(RwLock::new(HashMap::new())),
            requested_subscriptions: Arc::new(RwLock::new(HashMap::new())),
            active_producers: Arc::new(RwLock::new(HashMap::new())),
            broadcast_configs: Arc::new(RwLock::new(Vec::new())),
            shutdown_tx,
            is_connected: Arc::new(RwLock::new(false)),
            error_callback: Arc::new(RwLock::new(None)),
            status_callback: Arc::new(RwLock::new(None)),
        }
    }

    pub fn set_error_callback<F>(&self, callback: F)
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        *self.error_callback.write() = Some(Box::new(callback));
    }

    pub fn set_status_callback<F>(&self, callback: F)
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        *self.status_callback.write() = Some(Box::new(callback));
    }

    pub fn add_subscription(&self, subscription: SubscriptionConfig) {
        self.requested_subscriptions
            .write()
            .insert(subscription.moq_track_name.clone(), subscription);
    }

    pub fn add_broadcast(&self, broadcast: BroadcastConfig) {
        self.broadcast_configs.write().push(broadcast);
    }

        /// Notifies the error callback
    #[allow(dead_code)]
    fn notify_error(&self, error: &str) {
        if let Some(callback) = self.error_callback.read().as_ref() {
            callback(error);
        }
        tracing::error!("{}", error);
    }

    fn notify_status(&self, status: &str) {
        if let Some(callback) = self.status_callback.read().as_ref() {
            callback(status);
        }
        tracing::info!("{}", status);
    }

    pub async fn start(&self) -> Result<()> {
        if *self.is_connected.read() {
            return Ok(());
        }

        // Initialize client
        let client = self
            .config
            .client_config
            .clone()
            .init()
            .context("Failed to initialize MoQ client")?;
        
        self.notify_status("MoQ client initialized");
        
        // Connect to server
        self.notify_status(&format!("Connecting to {}", self.config.moq_server_url));
        let session = client
            .connect(self.config.moq_server_url.clone())
            .await
            .context("Failed to connect to MoQ server")?;

        // Setup based on mode
        let moq_session = match self.mode {
            SessionMode::PublishOnly => {
                let origin = Origin::produce();
                
                // Create broadcast producer
                let broadcast = moq_lite::Broadcast::produce();
                
                // Setup producers for each configured broadcast
                let broadcast_configs = self.broadcast_configs.read().clone();
                for broadcast_config in &broadcast_configs {
                    let producer = Producer::new(
                        broadcast_config.clone(),
                        broadcast.producer.clone(),
                    );
                    self.active_producers
                        .write()
                        .insert(broadcast_config.moq_track_name.clone(), producer);
                }
                
                origin
                    .producer
                    .publish_broadcast(&self.config.moq_namespace, broadcast.consumer);
                
                *self.broadcast_producer.write() = Some(Arc::new(broadcast.producer));
                
                moq_lite::Session::connect(session, origin.consumer, None).await?
            }
            SessionMode::SubscribeOnly => {
                let origin = Origin::produce();
                let moq_session = 
                    moq_lite::Session::connect(session, None, Some(origin.producer)).await?;
                
                // Setup broadcast consumer with retry logic
                let broadcast_consumer = self.consume_broadcast_with_retry(&origin.consumer).await?;
                
                *self.broadcast_consumer.write() = Some(Arc::new(broadcast_consumer));
                
                // Setup catalog processor
                let catalog_processor = CatalogProcessor::new();
                *self.catalog_processor.write() = Some(catalog_processor);
                
                // Start catalog consumer
                self.start_catalog_consumer()?;
                
                moq_session
            }
            SessionMode::PublishAndSubscribe => {
                let origin = Origin::produce();
                let moq_session = 
                    moq_lite::Session::connect(session, origin.consumer, Some(origin.producer))
                        .await?;
                
                // Setup both producer and consumer
                // TODO: Implement dual mode
                
                moq_session
            }
        };

        let moq_session_arc = Arc::new(moq_session);
        *self.session.write() = Some(moq_session_arc.clone());
        *self.client.write() = Some(client);
        *self.is_connected.write() = true;
        
        self.notify_status("Connected to MoQ server");
        
        // Start connection monitoring task for reconnection
        if self.config.reconnect_on_failure {
            self.start_connection_monitor(moq_session_arc);
        }
        
        Ok(())
    }
    
    async fn consume_broadcast_with_retry(&self, consumer: &OriginConsumer) -> Result<BroadcastConsumer> {
        let mut retry_count = 0;
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        
        loop {
            match consumer.consume_broadcast(&self.config.moq_namespace) {
                Some(broadcast_consumer) => {
                    if retry_count > 0 {
                        self.notify_status(&format!("Successfully connected to broadcast '{}' after {} retries", 
                                                   self.config.moq_namespace, retry_count));
                    }
                    return Ok(broadcast_consumer);
                }
                None => {
                    retry_count += 1;
                    
                    self.notify_status(&format!("Broadcast '{}' not available, retrying in 2 seconds... (attempt {})", 
                                               self.config.moq_namespace, retry_count));
                    
                    // Wait 2 seconds but allow cancellation via shutdown signal
                    tokio::select! {
                        _ = tokio::time::sleep(tokio::time::Duration::from_secs(2)) => {
                            // Continue retrying
                        }
                        _ = shutdown_rx.recv() => {
                            return Err(anyhow::anyhow!("Broadcast retry cancelled due to session shutdown"));
                        }
                    }
                }
            }
        }
    }
    
    fn start_connection_monitor(&self, session: Arc<moq_lite::Session<moq_native::web_transport_quinn::Session>>) {
        let is_connected = self.is_connected.clone();
        let status_callback = self.status_callback.clone();
        let self_clone = self.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    result = session.closed() => {
                        tracing::warn!("MoQ session closed: {:?}", result);
                        *is_connected.write() = false;
                        
                        if let Some(cb) = status_callback.read().as_ref() {
                            cb("Connection lost, attempting to reconnect...");
                        }
                        
                        // Wait a bit before reconnecting
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                        
                        // Attempt to reconnect
                        match self_clone.reconnect().await {
                            Ok(_) => {
                                tracing::info!("Successfully reconnected to MoQ server");
                                if let Some(cb) = status_callback.read().as_ref() {
                                    cb("Reconnected to MoQ server");
                                }
                                // After successful reconnect, exit this monitor (new one will be created)
                                break;
                            }
                            Err(e) => {
                                tracing::error!("Failed to reconnect: {}", e);
                                if let Some(cb) = status_callback.read().as_ref() {
                                    cb(&format!("Reconnection failed: {}", e));
                                }
                                // Wait before trying again
                                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                                // Loop will retry
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        tracing::info!("Connection monitor shutting down");
                        break;
                    }
                }
            }
        });
    }
    
    async fn reconnect(&self) -> Result<()> {
        tracing::info!("Attempting to reconnect...");
        
        // Stop all active consumers
        {
            let mut consumers = self.active_consumers.write();
            for (_name, consumer) in consumers.drain() {
                consumer.stop();
            }
        }
        
        // Stop catalog consumer
        if let Some(consumer) = self.catalog_consumer.write().take() {
            consumer.stop();
        }
        
        // Clear session and client
        *self.session.write() = None;
        *self.client.write() = None;
        *self.is_connected.write() = false;
        
        // Wait a moment
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        
        // Reconnect - start() already has broadcast retry logic
        self.start().await
    }
    
    fn start_catalog_consumer(&self) -> Result<()> {
        let broadcast_consumer = self
            .broadcast_consumer
            .read()
            .as_ref()
            .context("No broadcast consumer available")?
            .clone();

        let catalog_config = SubscriptionConfig {
            moq_track_name: "catalog.json".to_string(),
            data_callback: {
                let catalog_processor = self.catalog_processor.clone();
                let requested_subs = self.requested_subscriptions.clone();
                let active_consumers = self.active_consumers.clone();
                let broadcast_consumer = broadcast_consumer.clone();
                let status_callback = self.status_callback.clone();
                let session_for_reconnect = self.clone(); // Clone self for reconnection callbacks
                
                Arc::new(move |data: &[u8]| {
                    if let Some(processor) = catalog_processor.read().as_ref() {
                        if let Err(e) = processor.process_catalog_data(data) {
                            tracing::error!("Failed to process catalog: {}", e);
                            return;
                        }
                        
                        // Check and update subscriptions based on catalog
                        let available_tracks = processor.get_available_tracks();
                        let requested = requested_subs.read();
                        let mut active = active_consumers.write();
                        
                        // Remove subscriptions for tracks no longer available
                        active.retain(|track_name, _consumer| {
                            available_tracks.contains_key(track_name)
                        });
                        
                        // Add new subscriptions for available tracks
                        for (track_name, sub_config) in requested.iter() {
                            if available_tracks.contains_key(track_name) && !active.contains_key(track_name) {
                                if let Some(callback) = status_callback.read().as_ref() {
                                    callback(&format!("Starting subscription to: {}", track_name));
                                }
                                
                                // Create a modified subscription config with reconnect callback
                                let session_for_track = session_for_reconnect.clone();
                                let track_name_for_log = track_name.clone();
                                let modified_config = SubscriptionConfig {
                                    moq_track_name: sub_config.moq_track_name.clone(),
                                    data_callback: sub_config.data_callback.clone(),
                                    reconnect_callback: Some(Arc::new(move || {
                                        let session = session_for_track.clone();
                                        let track = track_name_for_log.clone();
                                        tokio::spawn(async move {
                                            tracing::info!("Consumer-triggered reconnection starting for track {}...", track);
                                            if let Err(e) = session.reconnect().await {
                                                tracing::error!("Consumer-triggered reconnection failed: {}", e);
                                            }
                                        });
                                    })),
                                };
                                
                                match Consumer::new(
                                    broadcast_consumer.clone(),
                                    modified_config,
                                ) {
                                    Ok(consumer) => {
                                        active.insert(track_name.clone(), consumer);
                                    }
                                    Err(e) => {
                                        tracing::error!("Failed to create consumer for {}: {}", track_name, e);
                                    }
                                }
                            }
                        }
                    }
                })
            },
            reconnect_callback: {
                let session = self.clone();
                Some(Arc::new(move || {
                    let session = session.clone();
                    tokio::spawn(async move {
                        tracing::info!("Catalog consumer-triggered reconnection starting...");
                        if let Err(e) = session.reconnect().await {
                            tracing::error!("Catalog consumer-triggered reconnection failed: {}", e);
                        }
                    });
                }))
            },
        };

        let catalog_consumer = Consumer::new(broadcast_consumer, catalog_config)?;
        *self.catalog_consumer.write() = Some(catalog_consumer);
        
        self.notify_status("Catalog consumer started");
        
        Ok(())
    }

    pub fn stop(&self) {
        *self.is_connected.write() = false;
        let _ = self.shutdown_tx.send(());
        
        // Stop all consumers
        self.active_consumers.write().clear();
        *self.catalog_consumer.write() = None;
        
        // Stop all producers
        self.active_producers.write().clear();
        
        *self.session.write() = None;
        *self.client.write() = None;
        
        self.notify_status("Session stopped");
    }

    pub fn is_running(&self) -> bool {
        *self.is_connected.read()
    }
}
