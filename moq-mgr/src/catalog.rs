use anyhow::{Context, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogTrack {
    #[serde(rename = "trackName")]
    pub track_name: String,
    #[serde(rename = "type")]
    pub track_type: String,
    pub priority: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StandardCatalog {
    tracks: Vec<CatalogTrack>,
}

// HANG catalog format (used by moq-clock and some other examples)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct HangRendition {
    bitrate: Option<i64>,
    codec: Option<String>,
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HangTrackGroup {
    priority: Option<i32>,
    renditions: Option<HashMap<String, HangRendition>>,
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HangCatalog {
    video: Option<HangTrackGroup>,
    audio: Option<HangTrackGroup>,
    #[serde(flatten)]
    extra: HashMap<String, HangTrackGroup>,
}

#[derive(Clone)]
pub struct CatalogProcessor {
    available_tracks: Arc<RwLock<HashMap<String, CatalogTrack>>>,
}

impl CatalogProcessor {
    pub fn new() -> Self {
        Self {
            available_tracks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn process_catalog_data(&self, data: &[u8]) -> Result<()> {
        let json_str = std::str::from_utf8(data).context("Invalid UTF-8 in catalog")?;
        
        // Try to parse as standard catalog first
        if let Ok(catalog) = serde_json::from_str::<StandardCatalog>(json_str) {
            self.process_standard_catalog(catalog);
            return Ok(());
        }
        
        // Try HANG format
        if let Ok(catalog) = serde_json::from_str::<HangCatalog>(json_str) {
            self.process_hang_catalog(catalog);
            return Ok(());
        }
        
        anyhow::bail!("Unknown catalog format");
    }

    fn process_standard_catalog(&self, catalog: StandardCatalog) {
        let mut tracks = self.available_tracks.write();
        tracks.clear();
        
        tracing::info!("Processing standard catalog with {} tracks", catalog.tracks.len());
        
        for track in catalog.tracks {
            tracing::info!(
                "Available track: {} (type: {}, priority: {})",
                track.track_name,
                track.track_type,
                track.priority
            );
            tracks.insert(track.track_name.clone(), track);
        }
    }

    fn process_hang_catalog(&self, catalog: HangCatalog) {
        let mut tracks = self.available_tracks.write();
        tracks.clear();
        
        tracing::info!("Processing HANG catalog format");
        
        // Process video tracks
        if let Some(video_group) = &catalog.video {
            if let Some(renditions) = &video_group.renditions {
                for (track_name, _rendition) in renditions {
                    let priority = video_group.priority.unwrap_or(50);
                    tracing::info!(
                        "Available video track: {} (priority: {})",
                        track_name,
                        priority
                    );
                    tracks.insert(
                        track_name.clone(),
                        CatalogTrack {
                            track_name: track_name.clone(),
                            track_type: "video".to_string(),
                            priority,
                        },
                    );
                }
            }
        }
        
        // Process audio tracks
        if let Some(audio_group) = &catalog.audio {
            if let Some(renditions) = &audio_group.renditions {
                for (track_name, _rendition) in renditions {
                    let priority = audio_group.priority.unwrap_or(50);
                    tracing::info!(
                        "Available audio track: {} (priority: {})",
                        track_name,
                        priority
                    );
                    tracks.insert(
                        track_name.clone(),
                        CatalogTrack {
                            track_name: track_name.clone(),
                            track_type: "audio".to_string(),
                            priority,
                        },
                    );
                }
            }
        }
        
        // Process other track groups
        for (group_name, track_group) in &catalog.extra {
            if let Some(renditions) = &track_group.renditions {
                for (track_name, _rendition) in renditions {
                    let priority = track_group.priority.unwrap_or(50);
                    tracing::info!(
                        "Available {} track: {} (priority: {})",
                        group_name,
                        track_name,
                        priority
                    );
                    tracks.insert(
                        track_name.clone(),
                        CatalogTrack {
                            track_name: track_name.clone(),
                            track_type: group_name.clone(),
                            priority,
                        },
                    );
                }
            }
        }
    }

    pub fn get_available_tracks(&self) -> HashMap<String, CatalogTrack> {
        self.available_tracks.read().clone()
    }

    pub fn is_track_available(&self, track_name: &str) -> bool {
        self.available_tracks.read().contains_key(track_name)
    }
}

impl Default for CatalogProcessor {
    fn default() -> Self {
        Self::new()
    }
}
