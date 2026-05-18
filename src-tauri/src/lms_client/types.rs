use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LmsServer {
    pub name: String,
    pub host: String,
    pub json_port: u16,
    pub uuid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LmsPlayer {
    pub player_id: String,
    pub name: String,
    pub model: String,
    pub connected: bool,
    pub power: bool,
    pub is_playing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LmsPlayerStatus {
    pub player_id: String,
    pub player_name: String,
    pub mode: PlayMode,
    pub time: f64,
    pub duration: f64,
    pub volume: u8,
    pub repeat: RepeatMode,
    pub shuffle: ShuffleMode,
    pub playlist_tracks: usize,
    pub playlist_index: usize,
    pub current_track: Option<LmsTrack>,
    pub playlist: Vec<LmsTrack>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LmsTrack {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
    pub bitrate: Option<String>,
    pub format: Option<String>,
    pub artwork_url: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayMode {
    Playing,
    Paused,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepeatMode {
    Off,
    One,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShuffleMode {
    Off,
    Songs,
    Albums,
}

#[derive(Debug, thiserror::Error)]
pub enum LmsError {
    #[error("Network error: {0}")]
    Network(String),
    #[error("JSON parse error: {0}")]
    Parse(String),
    #[error("Not connected to LMS server")]
    NotConnected,
    #[error("Player not found: {0}")]
    PlayerNotFound(String),
}

impl From<reqwest::Error> for LmsError {
    fn from(e: reqwest::Error) -> Self {
        LmsError::Network(e.to_string())
    }
}

impl From<serde_json::Error> for LmsError {
    fn from(e: serde_json::Error) -> Self {
        LmsError::Parse(e.to_string())
    }
}

impl From<LmsError> for String {
    fn from(e: LmsError) -> Self {
        e.to_string()
    }
}
