use anyhow::Result;
use std::sync::Arc;
use parking_lot::Mutex;
use tokio::task::JoinHandle;

use moq_lite::{BroadcastConsumer, Track, TrackConsumer};

pub type DataCallback = Arc<dyn Fn(&[u8]) + Send + Sync>;

#[derive(Clone)]
pub struct SubscriptionConfig {
    pub moq_track_name: String,
    pub data_callback: DataCallback,
}

pub struct Consumer {
    config: SubscriptionConfig,
    broadcast_consumer: Arc<BroadcastConsumer>,
    track_consumer: Arc<Mutex<Option<TrackConsumer>>>,
    worker_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    running: Arc<Mutex<bool>>,
}

impl Consumer {
    pub fn new(
        broadcast_consumer: Arc<BroadcastConsumer>,
        config: SubscriptionConfig,
    ) -> Result<Self> {
        let consumer = Self {
            config,
            broadcast_consumer,
            track_consumer: Arc::new(Mutex::new(None)),
            worker_handle: Arc::new(Mutex::new(None)),
            running: Arc::new(Mutex::new(false)),
        };

        // Automatically start the consumer
        consumer.start_internal()?;
        
        Ok(consumer)
    }

    fn start_internal(&self) -> Result<()> {
        if *self.running.lock() {
            return Ok(());
        }

        *self.running.lock() = true;

        let track = Track {
            name: self.config.moq_track_name.clone(),
            priority: 0,
        };

        let track_consumer = self
            .broadcast_consumer
            .subscribe_track(&track);
        
        *self.track_consumer.lock() = Some(track_consumer);

        // Spawn worker thread
        let track_consumer = self.track_consumer.clone();
        let running = self.running.clone();
        let callback = self.config.data_callback.clone();
        let track_name = self.config.moq_track_name.clone();

        let handle = tokio::spawn(async move {
            Self::consumer_loop(track_consumer, running, callback, track_name).await;
        });

        *self.worker_handle.lock() = Some(handle);
        
        tracing::info!("Consumer started for track: {}", self.config.moq_track_name);
        
        Ok(())
    }

    async fn consumer_loop(
        track_consumer: Arc<Mutex<Option<TrackConsumer>>>,
        running: Arc<Mutex<bool>>,
        callback: DataCallback,
        track_name: String,
    ) {
        while *running.lock() {
            let consumer_opt = {
                let mut guard = track_consumer.lock();
                guard.take()
            };
            
            if let Some(mut consumer) = consumer_opt {
                match consumer.next_group().await {
                    Ok(Some(mut group)) => {
                        // Read all frames from this group
                        loop {
                            if !*running.lock() {
                                *track_consumer.lock() = Some(consumer);
                                return;
                            }

                            match group.read_frame().await {
                                Ok(Some(frame_data)) => {
                                    // Call the data callback
                                    callback(&frame_data);
                                }
                                Ok(None) => {
                                    // No more frames in this group
                                    break;
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "Error reading frame from track {}: {}",
                                        track_name,
                                        e
                                    );
                                    break;
                                }
                            }
                        }
                        
                        // Put the consumer back
                        *track_consumer.lock() = Some(consumer);
                    }
                    Ok(None) => {
                        tracing::info!("Track {} ended", track_name);
                        break;
                    }
                    Err(e) => {
                        tracing::error!("Error getting next group for track {}: {}", track_name, e);
                        break;
                    }
                }
            } else {
                break;
            }
        }
        
        tracing::info!("Consumer loop ended for track: {}", track_name);
    }

    pub fn stop(&self) {
        *self.running.lock() = false;
        
        // Take the handle and abort it
        if let Some(handle) = self.worker_handle.lock().take() {
            handle.abort();
        }
        
        *self.track_consumer.lock() = None;
        
        tracing::info!("Consumer stopped for track: {}", self.config.moq_track_name);
    }

    pub fn get_track_name(&self) -> &str {
        &self.config.moq_track_name
    }

    pub fn is_running(&self) -> bool {
        *self.running.lock()
    }
}

impl Drop for Consumer {
    fn drop(&mut self) {
        self.stop();
    }
}
