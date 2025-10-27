use anyhow::Result;
use std::sync::Arc;
use parking_lot::Mutex;

use moq_lite::{BroadcastProducer, Track, TrackProducer, GroupProducer, Group};

#[derive(Clone)]
pub struct BroadcastConfig {
    pub moq_track_name: String,
    pub priority: u8,
}

pub struct Producer {
    config: BroadcastConfig,
    broadcast_producer: BroadcastProducer,
    track_producer: Arc<Mutex<Option<TrackProducer>>>,
    group_producer: Arc<Mutex<Option<GroupProducer>>>,
    group_sequence: Arc<Mutex<u64>>,
}

impl Producer {
    pub fn new(config: BroadcastConfig, broadcast_producer: BroadcastProducer) -> Self {
        Self {
            config,
            broadcast_producer,
            track_producer: Arc::new(Mutex::new(None)),
            group_producer: Arc::new(Mutex::new(None)),
            group_sequence: Arc::new(Mutex::new(rand::random::<u64>() % 1000000)),
        }
    }

    pub fn initialize(&mut self) -> Result<()> {
        let track = Track {
            name: self.config.moq_track_name.clone(),
            priority: self.config.priority,
        };

        let track_producer = self.broadcast_producer.create_track(track);
        *self.track_producer.lock() = Some(track_producer);
        
        tracing::info!("Producer initialized for track: {}", self.config.moq_track_name);
        Ok(())
    }

    pub fn start_group(&self) -> Result<()> {
        let mut track_producer = self.track_producer.lock();
        let track_producer = track_producer
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Track producer not initialized"))?;

        let group_sequence = {
            let mut seq = self.group_sequence.lock();
            let current = *seq;
            *seq += 1;
            current
        };

        let group = Group {
            sequence: group_sequence,
        };

        let group_producer = track_producer
            .create_group(group)
            .ok_or_else(|| anyhow::anyhow!("Failed to create group"))?;
        *self.group_producer.lock() = Some(group_producer);
        
        Ok(())
    }

    pub fn write_frame(&self, data: &[u8]) -> Result<()> {
        let mut group_producer = self.group_producer.lock();
        let group_producer = group_producer
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Group not started"))?;

        group_producer.write_frame(data.to_vec());
        Ok(())
    }

    pub fn finish_group(&self) -> Result<()> {
        let mut group_producer = self.group_producer.lock();
        *group_producer = None; // Dropping the group finishes it
        Ok(())
    }

    pub fn write_object(&self, data: &[u8]) -> Result<()> {
        self.start_group()?;
        self.write_frame(data)?;
        self.finish_group()?;
        Ok(())
    }

    pub fn get_track_name(&self) -> &str {
        &self.config.moq_track_name
    }
}
