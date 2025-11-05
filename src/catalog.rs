use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackType {
    #[serde(rename = "video")]
    Video,
    #[serde(rename = "audio")]
    Audio,
    #[serde(rename = "data")]
    Data,
}

impl std::fmt::Display for TrackType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrackType::Video => write!(f, "video"),
            TrackType::Audio => write!(f, "audio"),
            TrackType::Data => write!(f, "data"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrackDefinition {
    pub name: String,
    pub priority: u32,
    #[serde(rename = "type")]
    pub track_type: TrackType,
}

impl TrackDefinition {
    pub fn new(name: impl Into<String>, priority: u32, track_type: TrackType) -> Self {
        Self {
            name: name.into(),
            priority,
            track_type,
        }
    }

    pub fn video(name: impl Into<String>, priority: u32) -> Self {
        Self::new(name, priority, TrackType::Video)
    }

    pub fn audio(name: impl Into<String>, priority: u32) -> Self {
        Self::new(name, priority, TrackType::Audio)
    }

    pub fn data(name: impl Into<String>, priority: u32) -> Self {
        Self::new(name, priority, TrackType::Data)
    }
}

impl From<TrackDefinition> for moq_lite::Track {
    fn from(def: TrackDefinition) -> Self {
        moq_lite::Track {
            name: def.name,
            priority: def.priority.try_into().unwrap_or(0),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogType {
    None,
    Sesame,
    Hang,
}

/// Sesame format catalog (TypeScript-compatible)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SesameCatalog {
    pub tracks: Vec<SesameCatalogTrack>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SesameCatalogTrack {
    #[serde(rename = "type")]
    pub track_type: TrackType,
    #[serde(rename = "trackName")]
    pub track_name: String,
    pub priority: u32,
}

impl From<&TrackDefinition> for SesameCatalogTrack {
    fn from(def: &TrackDefinition) -> Self {
        Self {
            track_type: def.track_type.clone(),
            track_name: def.name.clone(),
            priority: def.priority,
        }
    }
}

impl SesameCatalog {
    pub fn from_tracks(tracks: &[TrackDefinition]) -> Self {
        Self {
            tracks: tracks.iter().map(|t| t.into()).collect(),
        }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    pub fn find_track(&self, name: &str) -> Option<&SesameCatalogTrack> {
        self.tracks.iter().find(|t| t.track_name == name)
    }
}

/// Hang format catalog (JSON-based, compatible with hang crate)
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HangCatalog {
    /// Video track information with multiple renditions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video: Option<HangVideo>,

    /// Audio track information with multiple renditions  
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<HangAudio>,

    /// Location track for spatial audio positioning
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<HangLocation>,

    /// User metadata for the broadcaster
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<HangUser>,

    /// Chat track metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat: Option<HangChat>,

    /// Preview information about the broadcast
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<HangTrack>,
}

/// Video track information in hang format
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HangVideo {
    /// Map of rendition name to video configuration
    pub renditions: HashMap<String, HangVideoConfig>,

    /// Priority of the video track relative to other tracks
    pub priority: u8,

    /// Display settings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<HangDisplay>,

    /// Rotation in degrees
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,

    /// Horizontal flip
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flip: Option<bool>,
}

/// Video decoder configuration based on WebCodecs
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HangVideoConfig {
    /// Codec string (e.g., "avc1.64001f", "vp09.00.10.08")
    pub codec: String,

    /// Codec-specific description data (hex-encoded)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Encoded dimensions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coded_width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coded_height: Option<u32>,

    /// Display aspect ratio
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_ratio_width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_ratio_height: Option<u32>,

    /// Bitrate in bits per second
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitrate: Option<u64>,

    /// Frame rate
    #[serde(skip_serializing_if = "Option::is_none")]
    pub framerate: Option<f64>,

    /// Optimize for latency
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optimize_for_latency: Option<bool>,
}

/// Audio track information in hang format
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HangAudio {
    /// Map of rendition name to audio configuration
    pub renditions: HashMap<String, HangAudioConfig>,

    /// Priority of the audio track relative to other tracks
    pub priority: u8,
}

/// Audio decoder configuration based on WebCodecs
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HangAudioConfig {
    /// Codec string (e.g., "opus", "mp4a.40.2")
    pub codec: String,

    /// Sample rate in Hz
    pub sample_rate: u32,

    /// Number of channels
    #[serde(rename = "numberOfChannels")]
    pub channel_count: u32,

    /// Bitrate in bits per second
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitrate: Option<u64>,

    /// Codec-specific description data (hex-encoded)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Display size configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HangDisplay {
    pub width: u32,
    pub height: u32,
}

/// Location track for spatial positioning
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HangLocation {
    /// Track name for location data
    pub track: String,
    /// Priority relative to other tracks
    pub priority: u8,
}

/// User metadata
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HangUser {
    /// Display name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// User avatar/profile picture
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
}

/// Chat track metadata
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HangChat {
    /// Track name for chat messages
    pub track: String,
    /// Priority relative to other tracks  
    pub priority: u8,
}

/// Generic track reference
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HangTrack {
    /// Track name
    pub name: String,
    /// Priority relative to other tracks
    pub priority: u8,
}

impl Default for HangCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl HangCatalog {
    /// Create a new empty hang catalog
    pub fn new() -> Self {
        Self {
            video: None,
            audio: None,
            location: None,
            user: None,
            chat: None,
            preview: None,
        }
    }

    /// Create a hang catalog from track definitions
    pub fn from_tracks(tracks: &[TrackDefinition]) -> Self {
        let mut catalog = Self::new();

        for track in tracks {
            match track.track_type {
                TrackType::Video => {
                    // Create a basic video rendition
                    let mut renditions = HashMap::new();
                    renditions.insert(
                        track.name.clone(),
                        HangVideoConfig {
                            codec: "avc1.42001e".to_string(), // H.264 baseline profile
                            description: None,
                            coded_width: Some(1280),
                            coded_height: Some(720),
                            display_ratio_width: None,
                            display_ratio_height: None,
                            bitrate: Some(2_000_000), // 2 Mbps default
                            framerate: Some(30.0),
                            optimize_for_latency: Some(true),
                        },
                    );

                    catalog.video = Some(HangVideo {
                        renditions,
                        priority: track.priority as u8,
                        display: None,
                        rotation: None,
                        flip: None,
                    });
                }
                TrackType::Audio => {
                    // Create a basic audio rendition
                    let mut renditions = HashMap::new();
                    renditions.insert(
                        track.name.clone(),
                        HangAudioConfig {
                            codec: "opus".to_string(),
                            sample_rate: 48000,
                            channel_count: 2,
                            bitrate: Some(128_000), // 128 kbps default
                            description: None,
                        },
                    );

                    catalog.audio = Some(HangAudio {
                        renditions,
                        priority: track.priority as u8,
                    });
                }
                TrackType::Data => {
                    // For data tracks, we can set them as preview or other metadata
                    if track.name == "catalog.json" {
                        // Skip catalog.json itself
                        continue;
                    }

                    catalog.preview = Some(HangTrack {
                        name: track.name.clone(),
                        priority: track.priority as u8,
                    });
                }
            }
        }

        catalog
    }

    /// Find if a track exists in this catalog
    pub fn find_track(&self, name: &str) -> bool {
        // Check video renditions
        if let Some(video) = &self.video {
            if video.renditions.contains_key(name) {
                return true;
            }
        }

        // Check audio renditions
        if let Some(audio) = &self.audio {
            if audio.renditions.contains_key(name) {
                return true;
            }
        }

        // Check other tracks
        if let Some(location) = &self.location {
            if location.track == name {
                return true;
            }
        }

        if let Some(chat) = &self.chat {
            if chat.track == name {
                return true;
            }
        }

        if let Some(preview) = &self.preview {
            if preview.name == name {
                return true;
            }
        }

        false
    }

    /// Serialize to JSON string
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Parse from JSON string
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Add a video track with configuration
    pub fn add_video_track(&mut self, name: String, config: HangVideoConfig, priority: u8) {
        let video = self.video.get_or_insert_with(|| HangVideo {
            renditions: HashMap::new(),
            priority,
            display: None,
            rotation: None,
            flip: None,
        });

        video.renditions.insert(name, config);
    }

    /// Add an audio track with configuration
    pub fn add_audio_track(&mut self, name: String, config: HangAudioConfig, priority: u8) {
        let audio = self.audio.get_or_insert_with(|| HangAudio {
            renditions: HashMap::new(),
            priority,
        });

        audio.renditions.insert(name, config);
    }
}

#[derive(Clone, Debug)]
pub enum Catalog {
    Sesame(SesameCatalog),
    Hang(Box<HangCatalog>),
}

impl Catalog {
    pub fn new(catalog_type: CatalogType, tracks: &[TrackDefinition]) -> Option<Self> {
        match catalog_type {
            CatalogType::None => None,
            CatalogType::Sesame => Some(Catalog::Sesame(SesameCatalog::from_tracks(tracks))),
            CatalogType::Hang => Some(Catalog::Hang(Box::new(HangCatalog::from_tracks(tracks)))),
        }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        match self {
            Catalog::Sesame(catalog) => catalog.to_json(),
            Catalog::Hang(catalog) => catalog.to_json(),
        }
    }

    pub fn find_track(&self, name: &str) -> bool {
        match self {
            Catalog::Sesame(catalog) => catalog.find_track(name).is_some(),
            Catalog::Hang(catalog) => catalog.find_track(name),
        }
    }

    pub fn parse_sesame(json: &str) -> Result<SesameCatalog, serde_json::Error> {
        SesameCatalog::from_json(json)
    }

    pub fn parse_hang(json: &str) -> Result<HangCatalog, serde_json::Error> {
        HangCatalog::from_json(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_track_definition() {
        let track = TrackDefinition::video("test-video", 1);
        assert_eq!(track.name, "test-video");
        assert_eq!(track.priority, 1);
        assert_eq!(track.track_type, TrackType::Video);
    }

    #[test]
    fn test_sesame_catalog() {
        let tracks = vec![
            TrackDefinition::video("video1", 1),
            TrackDefinition::audio("audio1", 0),
            TrackDefinition::data("data1", 2),
        ];

        let catalog = SesameCatalog::from_tracks(&tracks);
        assert_eq!(catalog.tracks.len(), 3);

        let json = catalog.to_json().unwrap();
        let parsed = SesameCatalog::from_json(&json).unwrap();
        assert_eq!(parsed.tracks.len(), 3);

        assert!(catalog.find_track("video1").is_some());
        assert!(catalog.find_track("nonexistent").is_none());
    }

    #[test]
    fn test_catalog_creation() {
        let tracks = vec![TrackDefinition::video("test", 1)];

        let none_catalog = Catalog::new(CatalogType::None, &tracks);
        assert!(none_catalog.is_none());

        let sesame_catalog = Catalog::new(CatalogType::Sesame, &tracks);
        assert!(sesame_catalog.is_some());

        if let Some(Catalog::Sesame(catalog)) = sesame_catalog {
            assert!(catalog.find_track("test").is_some());
        }
    }

    #[test]
    fn test_hang_catalog() {
        let tracks = vec![
            TrackDefinition::video("video1", 1),
            TrackDefinition::audio("audio1", 2),
        ];

        let catalog = HangCatalog::from_tracks(&tracks);

        // Test serialization
        let json = catalog.to_json().unwrap();
        println!("Hang catalog JSON: {}", json);

        // Test deserialization
        let _parsed = HangCatalog::from_json(&json).unwrap();

        // Test track finding
        assert!(catalog.find_track("video1"));
        assert!(catalog.find_track("audio1"));
        assert!(!catalog.find_track("nonexistent"));

        // Verify structure
        assert!(catalog.video.is_some());
        assert!(catalog.audio.is_some());

        if let Some(video) = &catalog.video {
            assert!(video.renditions.contains_key("video1"));
            assert_eq!(video.priority, 1);
        }

        if let Some(audio) = &catalog.audio {
            assert!(audio.renditions.contains_key("audio1"));
            assert_eq!(audio.priority, 2);
        }
    }

    #[test]
    fn test_hang_catalog_json_format() {
        let mut catalog = HangCatalog::new();

        // Add video configuration
        catalog.add_video_track(
            "video".to_string(),
            HangVideoConfig {
                codec: "avc1.64001f".to_string(),
                description: None,
                coded_width: Some(1280),
                coded_height: Some(720),
                display_ratio_width: None,
                display_ratio_height: None,
                bitrate: Some(6_000_000),
                framerate: Some(30.0),
                optimize_for_latency: Some(true),
            },
            1,
        );

        // Add audio configuration
        catalog.add_audio_track(
            "audio".to_string(),
            HangAudioConfig {
                codec: "opus".to_string(),
                sample_rate: 48000,
                channel_count: 2,
                bitrate: Some(128_000),
                description: None,
            },
            2,
        );

        let json = catalog.to_json().unwrap();

        // Verify it can be parsed back
        let parsed = HangCatalog::from_json(&json).unwrap();
        assert!(parsed.find_track("video"));
        assert!(parsed.find_track("audio"));

        // Verify JSON structure matches expected hang format
        assert!(json.contains("\"video\""));
        assert!(json.contains("\"audio\""));
        assert!(json.contains("\"renditions\""));
        assert!(json.contains("\"codec\""));
        assert!(json.contains("\"priority\""));
    }
}
