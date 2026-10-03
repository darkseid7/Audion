//! Dedicated pathless controller wire contract; never a native command proxy.
use serde::{de::Error, Deserialize, Deserializer, Serialize};

pub const MAX_IDENTIFIER_BYTES: usize = 256;
pub const MAX_TEXT_BYTES: usize = 2048;
pub const MAX_PAGE_ITEMS: usize = 200;
pub const MAX_COMMAND_BYTES: usize = 1024 * 1024;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

fn bounded_string<'de, D: Deserializer<'de>>(d: D, max: usize) -> Result<String, D::Error> {
    let value = String::deserialize(d)?;
    if value.is_empty() || value.len() > max {
        return Err(D::Error::custom("invalid text length"));
    }
    Ok(value)
}
fn identifier<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    bounded_string(d, MAX_IDENTIFIER_BYTES)
}
fn display_text<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    bounded_string(d, MAX_TEXT_BYTES)
}
fn safe_revision<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let number = serde_json::Number::deserialize(d)?;
    safe_number(&number).ok_or_else(|| D::Error::custom("unsafe integer"))
}
fn safe_number(value: &serde_json::Number) -> Option<u64> {
    if let Some(integer) = value.as_u64() {
        return (integer <= MAX_SAFE_INTEGER).then_some(integer);
    }
    let float = value.as_f64()?;
    (float.is_finite() && float >= 0.0 && float <= MAX_SAFE_INTEGER as f64 && float.fract() == 0.0)
        .then_some(float as u64)
}
fn entity_id<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let value = safe_revision(d)?;
    if value == 0 {
        return Err(D::Error::custom("entity ID must be positive"));
    }
    Ok(value)
}
fn optional_id<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    entity_id(d).map(Some)
}
fn optional_revision<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    safe_revision(d).map(Some)
}
fn nullable_identifier<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let value = Option::<String>::deserialize(d)?;
    if value
        .as_ref()
        .is_some_and(|v| v.is_empty() || v.len() > MAX_IDENTIFIER_BYTES)
    {
        return Err(D::Error::custom("invalid identifier"));
    }
    Ok(value)
}
fn optional_identifier<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    identifier(d).map(Some)
}
fn nonnegative<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    let value = f64::deserialize(d)?;
    if !value.is_finite() || value < 0.0 {
        return Err(D::Error::custom("invalid nonnegative value"));
    }
    Ok(value)
}
fn volume<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    let value = nonnegative(d)?;
    if value > 1.0 {
        return Err(D::Error::custom("volume exceeds one"));
    }
    Ok(value)
}
fn track_ids<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u64>, D::Error> {
    let values = Vec::<serde_json::Number>::deserialize(d)?;
    if values.is_empty() || values.len() > MAX_PAGE_ITEMS {
        return Err(D::Error::custom("invalid track IDs"));
    }
    values
        .iter()
        .map(|value| {
            safe_number(value)
                .filter(|id| *id > 0)
                .ok_or_else(|| D::Error::custom("invalid track ID"))
        })
        .collect()
}
fn optional_limit<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    let value = safe_revision(d)?;
    if value == 0 || value > MAX_PAGE_ITEMS as u64 {
        return Err(D::Error::custom("invalid page limit"));
    }
    Ok(Some(value))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationMode {
    Desktop,
    Controller,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepeatMode {
    None,
    One,
    All,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlbumPlayMode {
    All,
    LikedOnly,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueuePlacement {
    Next,
    AfterUserQueue,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum OutputRef {
    Pc {},
    Squeeze {
        #[serde(deserialize_with = "identifier")]
        player_id: String,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SnapshotOutputRef {
    Pc {},
    Squeeze { player_id: String },
    DesktopOnly { reason: String },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandPreconditions {
    #[serde(deserialize_with = "identifier")]
    pub host_epoch: String,
    #[serde(
        default,
        deserialize_with = "optional_revision",
        skip_serializing_if = "Option::is_none"
    )]
    pub queue_revision: Option<u64>,
    #[serde(
        default,
        deserialize_with = "optional_revision",
        skip_serializing_if = "Option::is_none"
    )]
    pub library_revision: Option<u64>,
    #[serde(
        default,
        deserialize_with = "optional_revision",
        skip_serializing_if = "Option::is_none"
    )]
    pub output_revision: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ApplicationIntent {
    PlayAlbum {
        #[serde(deserialize_with = "entity_id")]
        album_id: u64,
        play_mode: AlbumPlayMode,
        #[serde(
            default,
            deserialize_with = "optional_id",
            skip_serializing_if = "Option::is_none"
        )]
        start_track_id: Option<u64>,
    },
    PlayPlaylist {
        #[serde(deserialize_with = "entity_id")]
        playlist_id: u64,
        #[serde(
            default,
            deserialize_with = "optional_id",
            skip_serializing_if = "Option::is_none"
        )]
        start_track_id: Option<u64>,
    },
    PlayArtist {
        #[serde(deserialize_with = "identifier")]
        artist_name: String,
        #[serde(
            default,
            deserialize_with = "optional_id",
            skip_serializing_if = "Option::is_none"
        )]
        start_track_id: Option<u64>,
    },
    PlayLiked {
        #[serde(
            default,
            deserialize_with = "optional_id",
            skip_serializing_if = "Option::is_none"
        )]
        start_track_id: Option<u64>,
    },
    PlayTrack {
        #[serde(deserialize_with = "entity_id")]
        track_id: u64,
    },
    SelectOutput {
        output: OutputRef,
    },
    Pause {},
    Resume {},
    Next {},
    Previous {},
    Seek {
        #[serde(deserialize_with = "nonnegative")]
        seconds: f64,
    },
    SetVolume {
        #[serde(deserialize_with = "volume")]
        volume: f64,
    },
    SetShuffle {
        enabled: bool,
    },
    SetRepeat {
        mode: RepeatMode,
    },
    QueueInsert {
        #[serde(deserialize_with = "track_ids")]
        track_ids: Vec<u64>,
        placement: QueuePlacement,
    },
    QueueAppend {
        #[serde(deserialize_with = "track_ids")]
        track_ids: Vec<u64>,
    },
    QueueRemove {
        #[serde(deserialize_with = "identifier")]
        entry_id: String,
    },
    QueueReorder {
        #[serde(deserialize_with = "identifier")]
        entry_id: String,
        #[serde(deserialize_with = "nullable_identifier")]
        before_entry_id: Option<String>,
    },
    QueueClearUpcoming {},
    QueuePlay {
        #[serde(deserialize_with = "identifier")]
        entry_id: String,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "RawCommandEnvelope")]
pub struct CommandEnvelope {
    pub protocol_version: u8,
    pub request_id: String,
    pub preconditions: CommandPreconditions,
    pub intent: ApplicationIntent,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawCommandEnvelope {
    protocol_version: u8,
    #[serde(deserialize_with = "identifier")]
    request_id: String,
    preconditions: CommandPreconditions,
    intent: ApplicationIntent,
}
impl TryFrom<RawCommandEnvelope> for CommandEnvelope {
    type Error = String;
    fn try_from(raw: RawCommandEnvelope) -> Result<Self, Self::Error> {
        use ApplicationIntent::*;
        if raw.protocol_version != 1 {
            return Err("unsupported protocol version".into());
        }
        let entity_playback = matches!(
            raw.intent,
            PlayAlbum { .. }
                | PlayPlaylist { .. }
                | PlayArtist { .. }
                | PlayLiked { .. }
                | PlayTrack { .. }
        );
        let queue = matches!(
            raw.intent,
            QueueInsert { .. }
                | QueueAppend { .. }
                | QueueRemove { .. }
                | QueueReorder { .. }
                | QueueClearUpcoming { .. }
                | QueuePlay { .. }
        );
        let library =
            entity_playback || matches!(raw.intent, QueueInsert { .. } | QueueAppend { .. });
        let output = entity_playback
            || matches!(
                raw.intent,
                QueuePlay { .. }
                    | SelectOutput { .. }
                    | Pause { .. }
                    | Resume { .. }
                    | Next { .. }
                    | Previous { .. }
                    | Seek { .. }
                    | SetVolume { .. }
            );
        if (queue && raw.preconditions.queue_revision.is_none())
            || (library && raw.preconditions.library_revision.is_none())
            || (output && raw.preconditions.output_revision.is_none())
        {
            return Err("missing required revision".into());
        }
        let envelope = Self {
            protocol_version: raw.protocol_version,
            request_id: raw.request_id,
            preconditions: raw.preconditions,
            intent: raw.intent,
        };
        if serde_json::to_vec(&envelope)
            .map_err(|e| e.to_string())?
            .len()
            > MAX_COMMAND_BYTES
        {
            return Err("command too large".into());
        }
        Ok(envelope)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlErrorCode {
    Unauthorized,
    PermissionRequired,
    Unsupported,
    NotFound,
    RevisionConflict,
    OutputUnavailable,
    HostNotReady,
    ExecutionFailed,
    OutcomeUnknown,
    ResyncRequired,
    InvalidRequest,
    RateLimited,
    TooLarge,
    Busy,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ControlError {
    pub code: ControlErrorCode,
    #[serde(deserialize_with = "display_text")]
    pub message: String,
    pub retryable: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ExecutionResult {
    Applied {
        revision: u64,
    },
    Accepted {
        job_id: String,
        revision: u64,
    },
    Failed {
        error: ControlError,
        revision: u64,
        partial_effects: Vec<String>,
    },
    Superseded {
        error: ControlError,
        revision: u64,
        partial_effects: Vec<String>,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CompletedExecutionResult {
    Applied {
        revision: u64,
    },
    Failed {
        error: ControlError,
        revision: u64,
        partial_effects: Vec<String>,
    },
    Superseded {
        error: ControlError,
        revision: u64,
        partial_effects: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtworkReference {
    pub resource_id: String,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualitySummary {
    pub format: Option<String>,
    pub bitrate: Option<u64>,
    pub badges: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisplayTrack {
    pub id: u64,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album_id: Option<u64>,
    pub album: Option<String>,
    pub duration: Option<f64>,
    pub track_number: Option<u64>,
    pub disc_number: Option<u64>,
    pub quality: QualitySummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artwork: Option<ArtworkReference>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AlbumSortSummary {
    pub artist: Option<String>,
    pub year: Option<i64>,
    pub date_added: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisplayAlbum {
    pub id: u64,
    pub name: String,
    pub artist: Option<String>,
    pub year: Option<i64>,
    pub quality_badges: Vec<String>,
    pub sort_summary: AlbumSortSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artwork: Option<ArtworkReference>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisplayArtist {
    pub name: String,
    pub track_count: u64,
    pub album_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artwork: Option<ArtworkReference>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueueEntry {
    pub entry_id: String,
    pub track: DisplayTrack,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AlbumSort {
    ArtistAsc,
    ArtistDesc,
    YearDesc,
    YearAsc,
    AddedDesc,
    AddedAsc,
    NameAsc,
    NameDesc,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ApplicationQuery {
    Snapshot {},
    Outputs {},
    Albums {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sort: Option<AlbumSort>,
        #[serde(
            default,
            deserialize_with = "optional_limit",
            skip_serializing_if = "Option::is_none"
        )]
        limit: Option<u64>,
        #[serde(
            default,
            deserialize_with = "optional_identifier",
            skip_serializing_if = "Option::is_none"
        )]
        cursor: Option<String>,
    },
    AlbumTracks {
        #[serde(deserialize_with = "entity_id")]
        album_id: u64,
        #[serde(
            default,
            deserialize_with = "optional_limit",
            skip_serializing_if = "Option::is_none"
        )]
        limit: Option<u64>,
        #[serde(
            default,
            deserialize_with = "optional_identifier",
            skip_serializing_if = "Option::is_none"
        )]
        cursor: Option<String>,
    },
    Tracks {
        #[serde(
            default,
            deserialize_with = "optional_limit",
            skip_serializing_if = "Option::is_none"
        )]
        limit: Option<u64>,
        #[serde(
            default,
            deserialize_with = "optional_identifier",
            skip_serializing_if = "Option::is_none"
        )]
        cursor: Option<String>,
    },
    Artists {
        #[serde(
            default,
            deserialize_with = "optional_limit",
            skip_serializing_if = "Option::is_none"
        )]
        limit: Option<u64>,
        #[serde(
            default,
            deserialize_with = "optional_identifier",
            skip_serializing_if = "Option::is_none"
        )]
        cursor: Option<String>,
    },
    ArtistAlbums {
        #[serde(deserialize_with = "identifier")]
        artist_name: String,
        #[serde(
            default,
            deserialize_with = "optional_limit",
            skip_serializing_if = "Option::is_none"
        )]
        limit: Option<u64>,
        #[serde(
            default,
            deserialize_with = "optional_identifier",
            skip_serializing_if = "Option::is_none"
        )]
        cursor: Option<String>,
    },
    Search {
        #[serde(deserialize_with = "display_text")]
        text: String,
        #[serde(
            default,
            deserialize_with = "optional_limit",
            skip_serializing_if = "Option::is_none"
        )]
        limit: Option<u64>,
        #[serde(
            default,
            deserialize_with = "optional_identifier",
            skip_serializing_if = "Option::is_none"
        )]
        cursor: Option<String>,
    },
    Queue {
        #[serde(
            default,
            deserialize_with = "optional_limit",
            skip_serializing_if = "Option::is_none"
        )]
        limit: Option<u64>,
        #[serde(
            default,
            deserialize_with = "optional_identifier",
            skip_serializing_if = "Option::is_none"
        )]
        cursor: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryType {
    Snapshot,
    Albums,
    AlbumTracks,
    Tracks,
    Artists,
    ArtistAlbums,
    Search,
    Queue,
    Outputs,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentType {
    PlayAlbum,
    PlayPlaylist,
    PlayArtist,
    PlayLiked,
    PlayTrack,
    SelectOutput,
    Pause,
    Resume,
    Next,
    Previous,
    Seek,
    SetVolume,
    SetShuffle,
    SetRepeat,
    QueueInsert,
    QueueAppend,
    QueueRemove,
    QueueReorder,
    QueueClearUpcoming,
    QueuePlay,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackStatus {
    Stopped,
    Playing,
    Paused,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlbumView {
    Grid,
    List,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OutputCapabilities {
    pub playback: bool,
    pub seek: bool,
    pub volume: bool,
    pub shuffle: bool,
    pub repeat: bool,
    pub equalizer: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AvailableOutput {
    pub output: OutputRef,
    pub name: String,
    pub available: bool,
    pub capabilities: OutputCapabilities,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostCapabilities {
    pub queries: Vec<QueryType>,
    pub intents: Vec<IntentType>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PlaybackContext {
    Album {
        album_id: u64,
        play_mode: AlbumPlayMode,
    },
    Playlist {
        playlist_id: u64,
    },
    Artist {
        artist_name: String,
    },
    Liked {},
    Track {
        track_id: u64,
    },
    Queue {},
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlaybackState {
    pub status: PlaybackStatus,
    pub track: Option<DisplayTrack>,
    pub context: Option<PlaybackContext>,
    pub position: f64,
    pub duration: Option<f64>,
    pub volume: f64,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ControlError>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueueSummary {
    pub count: u64,
    pub current_entry_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DomainRevisions {
    pub library_revision: u64,
    pub queue_revision: u64,
    pub output_revision: u64,
    pub settings_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsProjection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album_view: Option<AlbumView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reduced_motion: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum JobSummary {
    Pending {
        job_id: String,
        progress: Option<f64>,
    },
    Running {
        job_id: String,
        progress: Option<f64>,
    },
    Completed {
        job_id: String,
        result: CompletedExecutionResult,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostSnapshot {
    pub host_id: String,
    pub host_epoch: String,
    pub revision: u64,
    pub revisions: DomainRevisions,
    pub playback: PlaybackState,
    pub queue: QueueSummary,
    pub output: SnapshotOutputRef,
    pub outputs: Vec<AvailableOutput>,
    pub capabilities: HostCapabilities,
    pub settings: SettingsProjection,
    pub jobs: Vec<JobSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum HostEvent {
    Playback {
        revision: u64,
        playback: PlaybackState,
    },
    Queue {
        revision: u64,
        queue_revision: u64,
        queue: QueueSummary,
    },
    Library {
        revision: u64,
        library_revision: u64,
    },
    Output {
        revision: u64,
        output_revision: u64,
        output: SnapshotOutputRef,
        outputs: Vec<AvailableOutput>,
    },
    Settings {
        revision: u64,
        settings_revision: u64,
        settings: SettingsProjection,
    },
    Capabilities {
        revision: u64,
        capabilities: HostCapabilities,
    },
    Job {
        revision: u64,
        job: JobSummary,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ApplicationUpdate {
    Snapshot {
        snapshot: HostSnapshot,
    },
    Events {
        host_epoch: String,
        events: Vec<HostEvent>,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SearchMatch {
    Track { track: DisplayTrack },
    Album { album: DisplayAlbum },
    Artist { artist: DisplayArtist },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum QueryResult {
    Snapshot {
        snapshot: HostSnapshot,
    },
    Albums {
        page: Page<DisplayAlbum>,
    },
    AlbumTracks {
        page: Page<DisplayTrack>,
    },
    Tracks {
        page: Page<DisplayTrack>,
    },
    Artists {
        page: Page<DisplayArtist>,
    },
    ArtistAlbums {
        page: Page<DisplayAlbum>,
    },
    Search {
        page: Page<SearchMatch>,
    },
    Queue {
        page: Page<QueueEntry>,
    },
    Outputs {
        outputs: Vec<AvailableOutput>,
        revision: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    const VALID: &str = include_str!("../../../tests/fixtures/controller/valid.json");
    const INVALID: &str = include_str!("../../../tests/fixtures/controller/invalid.json");

    // JSON 1 and 1.0 are the same safe integer at the TypeScript boundary.
    // Compare literal fixture values, not serde's internal numeric representation.
    fn equivalent_json(actual: &serde_json::Value, expected: &serde_json::Value) -> bool {
        use serde_json::Value;
        match (actual, expected) {
            (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
            (Value::Array(a), Value::Array(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(a, b)| equivalent_json(a, b))
            }
            (Value::Object(a), Value::Object(b)) => {
                a.len() == b.len()
                    && a.iter()
                        .all(|(key, value)| b.get(key).is_some_and(|b| equivalent_json(value, b)))
            }
            _ => actual == expected,
        }
    }

    #[test]
    fn accepts_shared_intent_fixtures() {
        let cases: serde_json::Value = serde_json::from_str(VALID).unwrap();
        for case in cases.as_array().unwrap() {
            let envelope: CommandEnvelope = serde_json::from_value(case["envelope"].clone())
                .unwrap_or_else(|error| panic!("{}: {}", case["name"], error));
            assert!(
                equivalent_json(&serde_json::to_value(envelope).unwrap(), &case["envelope"]),
                "{} did not preserve the wire value",
                case["name"]
            );
        }
    }

    #[test]
    fn rejects_nonfinite_and_non_scalar_json_values() {
        for json in [
            r#"{"protocolVersion":1,"requestId":"\ud800","preconditions":{"hostEpoch":"e","outputRevision":0},"intent":{"type":"pause"}}"#,
            r#"{"protocolVersion":1,"requestId":"r","preconditions":{"hostEpoch":"e","outputRevision":0},"intent":{"type":"seek","seconds":1e309}}"#,
        ] {
            assert!(serde_json::from_str::<CommandEnvelope>(json).is_err());
        }
    }

    #[test]
    fn rejects_invalid_role_and_unsafe_wire_fields() {
        let cases: serde_json::Value = serde_json::from_str(INVALID).unwrap();
        for case in cases.as_array().unwrap() {
            assert!(
                serde_json::from_value::<CommandEnvelope>(case["envelope"].clone()).is_err(),
                "{} was accepted",
                case["name"]
            );
        }
    }
}
