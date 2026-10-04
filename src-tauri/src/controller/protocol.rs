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
// Display metadata is not a library command identity: providers may use
// negative IDs, but the JSON number must still be an exact nonzero integer.
fn display_id<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    let number = serde_json::Number::deserialize(d)?;
    let value = number
        .as_f64()
        .ok_or_else(|| D::Error::custom("Invalid display ID"))?;
    if !value.is_finite() || value.fract() != 0.0 || value.abs() > MAX_SAFE_INTEGER as f64 {
        return Err(D::Error::custom("Invalid display ID"));
    }
    let id = value as i64;
    if id == 0 || !(-9_007_199_254_740_991..=9_007_199_254_740_991).contains(&id) {
        return Err(D::Error::custom("Invalid display ID"));
    }
    Ok(id)
}
fn nullable_display_id<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(d)?;
    value
        .map(|v| display_id(v).map_err(D::Error::custom))
        .transpose()
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

// A custom field deserializer without `default` makes the key required even for Option.
fn required_nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}
// Use with `default` only for truly optional TS fields: absent is valid, null is not.
fn optional_value<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}
fn nullable_revision<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    Option::<serde_json::Number>::deserialize(d)?
        .map(|value| safe_number(&value).ok_or_else(|| D::Error::custom("unsafe integer")))
        .transpose()
}
fn nullable_signed_integer<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    Option::<serde_json::Number>::deserialize(d)?
        .map(|value| {
            let number = value
                .as_f64()
                .ok_or_else(|| D::Error::custom("invalid signed integer"))?;
            if !number.is_finite()
                || number.abs() > MAX_SAFE_INTEGER as f64
                || number.fract() != 0.0
            {
                return Err(D::Error::custom("unsafe signed integer"));
            }
            Ok(number as i64)
        })
        .transpose()
}
fn finite<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    let value = f64::deserialize(d)?;
    if !value.is_finite() {
        return Err(D::Error::custom("nonfinite number"));
    }
    Ok(value)
}
fn nullable_finite<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    let value = Option::<f64>::deserialize(d)?;
    if value.is_some_and(|number| !number.is_finite()) {
        return Err(D::Error::custom("nonfinite number"));
    }
    Ok(value)
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
    Squeeze {
        #[serde(deserialize_with = "identifier")]
        player_id: String,
    },
    DesktopOnly {
        reason: String,
    },
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
    QueueEntity { entity: QueueEntity, placement: EntityQueuePlacement },
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
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum QueueEntity {
    Album { #[serde(deserialize_with = "entity_id")] album_id: u64, play_mode: AlbumPlayMode },
    Playlist { #[serde(deserialize_with = "entity_id")] playlist_id: u64 },
    Artist { #[serde(deserialize_with = "identifier")] artist_name: String },
    Liked {},
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityQueuePlacement { Next, AfterUserQueue, End }
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
    #[serde(deserialize_with = "safe_revision")]
    protocol_version: u64,
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
                | QueueEntity { .. }
                | QueueAppend { .. }
                | QueueRemove { .. }
                | QueueReorder { .. }
                | QueueClearUpcoming { .. }
                | QueuePlay { .. }
        );
        let library =
            entity_playback || matches!(raw.intent, QueueInsert { .. } | QueueAppend { .. } | QueueEntity { .. });
        let output = entity_playback
            || matches!(
                raw.intent,
                QueueEntity { .. }
                    | QueuePlay { .. }
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
            protocol_version: 1,
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
        #[serde(deserialize_with = "safe_revision")]
        revision: u64,
    },
    Accepted {
        #[serde(deserialize_with = "identifier")]
        job_id: String,
        #[serde(deserialize_with = "safe_revision")]
        revision: u64,
    },
    Failed {
        error: ControlError,
        #[serde(deserialize_with = "safe_revision")]
        revision: u64,
        partial_effects: Vec<String>,
    },
    Superseded {
        error: ControlError,
        #[serde(deserialize_with = "safe_revision")]
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
        #[serde(deserialize_with = "safe_revision")]
        revision: u64,
    },
    Failed {
        error: ControlError,
        #[serde(deserialize_with = "safe_revision")]
        revision: u64,
        partial_effects: Vec<String>,
    },
    Superseded {
        error: ControlError,
        #[serde(deserialize_with = "safe_revision")]
        revision: u64,
        partial_effects: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtworkReference {
    #[serde(deserialize_with = "identifier")]
    pub resource_id: String,
    #[serde(deserialize_with = "safe_revision")]
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualitySummary {
    #[serde(deserialize_with = "required_nullable")]
    pub format: Option<String>,
    #[serde(deserialize_with = "nullable_revision")]
    pub bitrate: Option<u64>,
    pub badges: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisplayTrack {
    #[serde(
        default,
        deserialize_with = "optional_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub liked: Option<bool>,
    #[serde(deserialize_with = "display_id")]
    pub id: i64,
    #[serde(deserialize_with = "required_nullable")]
    pub title: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    pub artist: Option<String>,
    #[serde(deserialize_with = "nullable_display_id")]
    pub album_id: Option<i64>,
    #[serde(deserialize_with = "required_nullable")]
    pub album: Option<String>,
    #[serde(deserialize_with = "nullable_finite")]
    pub duration: Option<f64>,
    #[serde(deserialize_with = "nullable_revision")]
    pub track_number: Option<u64>,
    #[serde(deserialize_with = "nullable_revision")]
    pub disc_number: Option<u64>,
    pub quality: QualitySummary,
    #[serde(
        default,
        deserialize_with = "optional_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub artwork: Option<ArtworkReference>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AlbumSortSummary {
    #[serde(deserialize_with = "required_nullable")]
    pub artist: Option<String>,
    #[serde(deserialize_with = "nullable_signed_integer")]
    pub year: Option<i64>,
    #[serde(deserialize_with = "required_nullable")]
    pub date_added: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisplayAlbum {
    #[serde(deserialize_with = "entity_id")]
    pub id: u64,
    pub name: String,
    #[serde(deserialize_with = "required_nullable")]
    pub artist: Option<String>,
    #[serde(deserialize_with = "nullable_signed_integer")]
    pub year: Option<i64>,
    pub quality_badges: Vec<String>,
    pub sort_summary: AlbumSortSummary,
    #[serde(
        default,
        deserialize_with = "optional_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub artwork: Option<ArtworkReference>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisplayPlaylist {
    #[serde(deserialize_with = "entity_id")]
    pub id: u64,
    pub name: String,
    #[serde(deserialize_with = "safe_revision")]
    pub track_count: u64,
    #[serde(
        default,
        deserialize_with = "optional_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub artwork: Option<ArtworkReference>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AlbumDetail {
    pub album: DisplayAlbum,
    #[serde(deserialize_with = "nullable_signed_integer")]
    pub original_year: Option<i64>,
    #[serde(deserialize_with = "safe_revision")]
    pub track_count: u64,
    pub liked: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisplayArtist {
    #[serde(deserialize_with = "identifier")]
    pub name: String,
    #[serde(deserialize_with = "safe_revision")]
    pub track_count: u64,
    #[serde(deserialize_with = "safe_revision")]
    pub album_count: u64,
    #[serde(
        default,
        deserialize_with = "optional_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub artwork: Option<ArtworkReference>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueueEntry {
    #[serde(deserialize_with = "identifier")]
    pub entry_id: String,
    pub track: DisplayTrack,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Page<T> {
    pub items: Vec<T>,
    #[serde(deserialize_with = "nullable_identifier")]
    pub next_cursor: Option<String>,
    #[serde(deserialize_with = "safe_revision")]
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
    AlbumDetail {
        #[serde(deserialize_with = "entity_id")]
        album_id: u64,
    },
    Playlists {
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
    PlaylistTracks {
        #[serde(deserialize_with = "entity_id")]
        playlist_id: u64,
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
    ArtistTracks {
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
    LikedTracks {
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

    Snapshot {},
    Outputs {},
    Albums {
        #[serde(
            default,
            deserialize_with = "optional_value",
            skip_serializing_if = "Option::is_none"
        )]
        liked_only: Option<bool>,
        #[serde(
            default,
            deserialize_with = "optional_identifier",
            skip_serializing_if = "Option::is_none"
        )]
        text: Option<String>,
        #[serde(
            default,
            deserialize_with = "optional_value",
            skip_serializing_if = "Option::is_none"
        )]
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
        #[serde(default, deserialize_with = "optional_value", skip_serializing_if = "Option::is_none")]
        liked_only: Option<bool>,
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
    AlbumDetail,
    Playlists,
    PlaylistTracks,
    ArtistTracks,
    LikedTracks,
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
    QueueEntity,
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
        #[serde(deserialize_with = "display_id")]
        album_id: i64,
        play_mode: AlbumPlayMode,
    },
    Playlist {
        #[serde(deserialize_with = "display_id")]
        playlist_id: i64,
    },
    Artist {
        #[serde(deserialize_with = "identifier")]
        artist_name: String,
    },
    Liked {},
    Track {
        #[serde(deserialize_with = "display_id")]
        track_id: i64,
    },
    Queue {},
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlaybackState {
    pub status: PlaybackStatus,
    #[serde(deserialize_with = "required_nullable")]
    pub track: Option<DisplayTrack>,
    #[serde(deserialize_with = "required_nullable")]
    pub context: Option<PlaybackContext>,
    #[serde(deserialize_with = "finite")]
    pub position: f64,
    #[serde(deserialize_with = "nullable_finite")]
    pub duration: Option<f64>,
    #[serde(deserialize_with = "volume")]
    pub volume: f64,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    #[serde(
        default,
        deserialize_with = "optional_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub error: Option<ControlError>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueueSummary {
    #[serde(deserialize_with = "safe_revision")]
    pub count: u64,
    #[serde(deserialize_with = "nullable_identifier")]
    pub current_entry_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DomainRevisions {
    #[serde(deserialize_with = "safe_revision")]
    pub library_revision: u64,
    #[serde(deserialize_with = "safe_revision")]
    pub queue_revision: u64,
    #[serde(deserialize_with = "safe_revision")]
    pub output_revision: u64,
    #[serde(deserialize_with = "safe_revision")]
    pub settings_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsProjection {
    #[serde(
        default,
        deserialize_with = "optional_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub album_view: Option<AlbumView>,
    #[serde(
        default,
        deserialize_with = "optional_value",
        skip_serializing_if = "Option::is_none"
    )]
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
        #[serde(deserialize_with = "identifier")]
        job_id: String,
        #[serde(deserialize_with = "nullable_finite")]
        progress: Option<f64>,
    },
    Running {
        #[serde(deserialize_with = "identifier")]
        job_id: String,
        #[serde(deserialize_with = "nullable_finite")]
        progress: Option<f64>,
    },
    Completed {
        #[serde(deserialize_with = "identifier")]
        job_id: String,
        result: CompletedExecutionResult,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostSnapshot {
    #[serde(deserialize_with = "identifier")]
    pub host_id: String,
    #[serde(deserialize_with = "identifier")]
    pub host_epoch: String,
    #[serde(deserialize_with = "safe_revision")]
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
        #[serde(deserialize_with = "safe_revision")]
        revision: u64,
        playback: PlaybackState,
    },
    Queue {
        #[serde(deserialize_with = "safe_revision")]
        revision: u64,
        #[serde(deserialize_with = "safe_revision")]
        queue_revision: u64,
        queue: QueueSummary,
    },
    Library {
        #[serde(deserialize_with = "safe_revision")]
        revision: u64,
        #[serde(deserialize_with = "safe_revision")]
        library_revision: u64,
    },
    Output {
        #[serde(deserialize_with = "safe_revision")]
        revision: u64,
        #[serde(deserialize_with = "safe_revision")]
        output_revision: u64,
        output: SnapshotOutputRef,
        outputs: Vec<AvailableOutput>,
    },
    Settings {
        #[serde(deserialize_with = "safe_revision")]
        revision: u64,
        #[serde(deserialize_with = "safe_revision")]
        settings_revision: u64,
        settings: SettingsProjection,
    },
    Capabilities {
        #[serde(deserialize_with = "safe_revision")]
        revision: u64,
        capabilities: HostCapabilities,
    },
    Job {
        #[serde(deserialize_with = "safe_revision")]
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
        #[serde(deserialize_with = "identifier")]
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
    AlbumDetail {
        detail: AlbumDetail,
        #[serde(deserialize_with = "safe_revision")]
        revision: u64,
    },
    Playlists {
        page: Page<DisplayPlaylist>,
    },
    PlaylistTracks {
        page: Page<DisplayTrack>,
    },
    ArtistTracks {
        page: Page<DisplayTrack>,
    },
    LikedTracks {
        page: Page<DisplayTrack>,
    },
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
        #[serde(deserialize_with = "safe_revision")]
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
    fn accepts_protocol_version_numeric_representations() {
        for version in ["1", "1.0", "1e0"] {
            let raw = format!(
                r#"{{"protocolVersion":{version},"requestId":"r","preconditions":{{"hostEpoch":"e","outputRevision":0}},"intent":{{"type":"pause"}}}}"#
            );
            let envelope: CommandEnvelope =
                serde_json::from_str(&raw).unwrap_or_else(|error| panic!("{version}: {error}"));
            assert_eq!(envelope.protocol_version, 1);
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

    #[test]
    fn provider_publication_accepts_signed_metadata_without_widening_commands() {
        let raw: serde_json::Value = serde_json::from_str(include_str!("../../../tests/fixtures/controller/provider-publication.json")).unwrap();
        let received = serde_json::from_value::<HostUpdate>(raw.clone());
        assert!(received.is_ok(), "actual TypeScript host projection must cross native publication");
        let HostUpdate::Projection { snapshot, presentation: _ } = received.unwrap();
        assert_eq!(serde_json::to_value(snapshot).unwrap()["playback"]["track"]["id"], -8);
        let presentation: HostPresentation = serde_json::from_value(raw["presentation"].clone()).unwrap();
        assert_eq!(presentation.queue.len(), 2);
        assert_ne!(presentation.queue[0].entry_id, presentation.queue[1].entry_id);
        for field in ["trackId", "albumId", "playlistId"] {
            let kind = match field { "trackId" => "play_track", "albumId" => "play_album", _ => "play_playlist" };
            let mut intent = serde_json::json!({"type":kind,field:-8});
            if field == "albumId" { intent["playMode"] = serde_json::json!("all"); }
            let envelope = serde_json::json!({"protocolVersion":1,"requestId":"fixture","preconditions":{"hostEpoch":"fixture-epoch","libraryRevision":0,"outputRevision":0},"intent":intent});
            assert!(serde_json::from_value::<CommandEnvelope>(envelope).is_err());
        }
    }
    fn track_fixture() -> serde_json::Value {
        serde_json::json!({"id":1,"title":"","artist":null,"albumId":null,"album":null,"duration":null,"trackNumber":null,"discNumber":null,"quality":{"format":null,"bitrate":null,"badges":[]}})
    }
    fn playback_fixture() -> serde_json::Value {
        serde_json::json!({"status":"stopped","track":null,"context":null,"position":0,"duration":null,"volume":0,"shuffle":false,"repeat":"none"})
    }
    fn snapshot_fixture() -> serde_json::Value {
        serde_json::json!({"hostId":"h","hostEpoch":"e","revision":0,"revisions":{"libraryRevision":0,"queueRevision":0,"outputRevision":0,"settingsRevision":0},"playback":playback_fixture(),"queue":{"count":0,"currentEntryId":null},"output":{"kind":"pc"},"outputs":[],"capabilities":{"queries":[],"intents":[]},"settings":{},"jobs":[]})
    }
    fn rejects_field_values<T: serde::de::DeserializeOwned>(
        fixture: &serde_json::Value,
        field: &str,
        invalid: &[serde_json::Value],
    ) {
        assert!(
            serde_json::from_value::<T>(fixture.clone()).is_ok(),
            "valid {} fixture",
            std::any::type_name::<T>()
        );
        for value in invalid {
            let mut changed = fixture.clone();
            changed[field] = value.clone();
            assert!(
                serde_json::from_value::<T>(changed).is_err(),
                "{} accepted {field}={value}",
                std::any::type_name::<T>()
            );
        }
    }
    fn requires_nullable_fields<T: serde::de::DeserializeOwned>(
        fixture: &serde_json::Value,
        fields: &[&str],
    ) {
        assert!(
            serde_json::from_value::<T>(fixture.clone()).is_ok(),
            "explicit nullable fields should be accepted"
        );
        for field in fields {
            let mut missing = fixture.clone();
            missing.as_object_mut().unwrap().remove(*field);
            assert!(
                serde_json::from_value::<T>(missing).is_err(),
                "{} accepted missing {field}",
                std::any::type_name::<T>()
            );
        }
    }
    fn invalid_identifiers() -> Vec<serde_json::Value> {
        vec![serde_json::json!(""), serde_json::json!("é".repeat(129))]
    }
    fn invalid_revisions() -> Vec<serde_json::Value> {
        vec![
            serde_json::json!(-1),
            serde_json::json!(0.5),
            serde_json::json!(9_007_199_254_740_992_u64),
        ]
    }
    fn invalid_display_ids() -> Vec<serde_json::Value> {
        let mut values = invalid_entity_ids();
        values.retain(|v| v.as_i64() != Some(-1));
        values.push(serde_json::json!(-9_007_199_254_740_992i64));
        values
    }
    fn invalid_entity_ids() -> Vec<serde_json::Value> {
        let mut values = invalid_revisions();
        values.push(serde_json::json!(0));
        values
    }

    #[test]
    fn artwork_reference_enforces_identifier_and_revision_bounds() {
        let fixture = serde_json::json!({"resourceId":"a","revision":0});
        rejects_field_values::<ArtworkReference>(&fixture, "resourceId", &invalid_identifiers());
        rejects_field_values::<ArtworkReference>(&fixture, "revision", &invalid_revisions());
        let at_limit =
            serde_json::json!({"resourceId":"é".repeat(128),"revision":9_007_199_254_740_991_u64});
        assert!(serde_json::from_value::<ArtworkReference>(at_limit).is_ok());
    }

    #[test]
    fn display_track_requires_nullable_metadata_without_fabricating_it() {
        let fixture = track_fixture();
        requires_nullable_fields::<DisplayTrack>(
            &fixture,
            &[
                "title",
                "artist",
                "albumId",
                "album",
                "duration",
                "trackNumber",
                "discNumber",
            ],
        );
        requires_nullable_fields::<QualitySummary>(&fixture["quality"], &["format", "bitrate"]);
        let track: DisplayTrack = serde_json::from_value(fixture.clone()).unwrap();
        assert_eq!(serde_json::to_value(track).unwrap(), fixture);
        let mut empty = fixture.clone();
        empty["artist"] = serde_json::json!("");
        empty["album"] = serde_json::json!("");
        empty["quality"]["format"] = serde_json::json!("");
        assert!(serde_json::from_value::<DisplayTrack>(empty).is_ok());
    }

    #[test]
    fn display_entities_validate_ids_counts_and_required_nullable_fields() {
        let track = track_fixture();
        rejects_field_values::<DisplayTrack>(&track, "id", &invalid_display_ids());
        rejects_field_values::<DisplayTrack>(&track, "albumId", &invalid_display_ids());
        for field in ["trackNumber", "discNumber"] {
            rejects_field_values::<DisplayTrack>(&track, field, &invalid_revisions());
        }
        rejects_field_values::<QualitySummary>(&track["quality"], "bitrate", &invalid_revisions());
        let album = serde_json::json!({"id":1,"name":"Album","artist":null,"year":null,"qualityBadges":[],"sortSummary":{"artist":null,"year":null,"dateAdded":null,"name":"Album"}});
        rejects_field_values::<DisplayAlbum>(&album, "id", &invalid_entity_ids());
        requires_nullable_fields::<DisplayAlbum>(&album, &["artist", "year"]);
        requires_nullable_fields::<AlbumSortSummary>(
            &album["sortSummary"],
            &["artist", "year", "dateAdded"],
        );
        let artist = serde_json::json!({"name":"Artist","trackCount":0,"albumCount":0});
        rejects_field_values::<DisplayArtist>(&artist, "name", &invalid_identifiers());
        for field in ["trackCount", "albumCount"] {
            rejects_field_values::<DisplayArtist>(&artist, field, &invalid_revisions());
        }
        for context in [
            serde_json::json!({"type":"album","albumId":1,"playMode":"all"}),
            serde_json::json!({"type":"playlist","playlistId":1}),
            serde_json::json!({"type":"track","trackId":1}),
        ] {
            let field = match context["type"].as_str().unwrap() {
                "album" => "albumId",
                "playlist" => "playlistId",
                _ => "trackId",
            };
            rejects_field_values::<PlaybackContext>(&context, field, &invalid_display_ids());
        }
        rejects_field_values::<PlaybackContext>(
            &serde_json::json!({"type":"artist","artistName":"Artist"}),
            "artistName",
            &invalid_identifiers(),
        );
    }

    #[test]
    fn pages_require_nullable_bounded_cursor_and_safe_revision() {
        let fixture = serde_json::json!({"items":[],"nextCursor":null,"revision":0});
        requires_nullable_fields::<Page<DisplayTrack>>(&fixture, &["nextCursor"]);
        rejects_field_values::<Page<DisplayTrack>>(&fixture, "nextCursor", &invalid_identifiers());
        rejects_field_values::<Page<DisplayTrack>>(&fixture, "revision", &invalid_revisions());
        let mut encoded_integer = fixture.clone();
        encoded_integer["revision"] = serde_json::json!(1.0);
        assert!(serde_json::from_value::<Page<DisplayTrack>>(encoded_integer).is_ok());
    }

    #[test]
    fn execution_and_job_results_validate_identifiers_revisions_and_required_progress() {
        let error = serde_json::json!({"code":"execution_failed","message":"Output failed","retryable":false});
        for fixture in [
            serde_json::json!({"status":"applied","revision":0}),
            serde_json::json!({"status":"accepted","jobId":"j","revision":0}),
            serde_json::json!({"status":"failed","revision":0,"error":error,"partialEffects":[]}),
            serde_json::json!({"status":"superseded","revision":0,"error":error,"partialEffects":[]}),
        ] {
            rejects_field_values::<ExecutionResult>(&fixture, "revision", &invalid_revisions());
            if fixture["status"] != "accepted" {
                rejects_field_values::<CompletedExecutionResult>(
                    &fixture,
                    "revision",
                    &invalid_revisions(),
                );
            } else {
                rejects_field_values::<ExecutionResult>(&fixture, "jobId", &invalid_identifiers());
            }
        }
        for status in ["pending", "running"] {
            let fixture = serde_json::json!({"status":status,"jobId":"j","progress":null});
            requires_nullable_fields::<JobSummary>(&fixture, &["progress"]);
            rejects_field_values::<JobSummary>(&fixture, "jobId", &invalid_identifiers());
            let concrete = serde_json::json!({"status":status,"jobId":"j","progress":100});
            assert!(
                serde_json::from_value::<JobSummary>(concrete).is_ok(),
                "no unapproved unit-interval progress restriction"
            );
        }
        rejects_field_values::<JobSummary>(
            &serde_json::json!({"status":"completed","jobId":"j","result":{"status":"applied","revision":0}}),
            "jobId",
            &invalid_identifiers(),
        );
    }

    #[test]
    fn snapshot_queue_and_output_contracts_enforce_primitive_invariants() {
        let snapshot = snapshot_fixture();
        for field in ["hostId", "hostEpoch"] {
            rejects_field_values::<HostSnapshot>(&snapshot, field, &invalid_identifiers());
        }
        rejects_field_values::<HostSnapshot>(&snapshot, "revision", &invalid_revisions());
        for field in [
            "libraryRevision",
            "queueRevision",
            "outputRevision",
            "settingsRevision",
        ] {
            rejects_field_values::<DomainRevisions>(
                &snapshot["revisions"],
                field,
                &invalid_revisions(),
            );
        }
        requires_nullable_fields::<PlaybackState>(
            &snapshot["playback"],
            &["track", "context", "duration"],
        );
        let queue = &snapshot["queue"];
        requires_nullable_fields::<QueueSummary>(queue, &["currentEntryId"]);
        rejects_field_values::<QueueSummary>(queue, "currentEntryId", &invalid_identifiers());
        rejects_field_values::<QueueSummary>(queue, "count", &invalid_revisions());
        rejects_field_values::<QueueEntry>(
            &serde_json::json!({"entryId":"entry","track":track_fixture()}),
            "entryId",
            &invalid_identifiers(),
        );
        rejects_field_values::<SnapshotOutputRef>(
            &serde_json::json!({"kind":"squeeze","playerId":"player"}),
            "playerId",
            &invalid_identifiers(),
        );
    }

    #[test]
    fn event_and_query_results_validate_all_revision_fields_and_epochs() {
        for fixture in [
            serde_json::json!({"type":"playback","revision":0,"playback":playback_fixture()}),
            serde_json::json!({"type":"queue","revision":0,"queueRevision":0,"queue":{"count":0,"currentEntryId":null}}),
            serde_json::json!({"type":"library","revision":0,"libraryRevision":0}),
            serde_json::json!({"type":"output","revision":0,"outputRevision":0,"output":{"kind":"pc"},"outputs":[]}),
            serde_json::json!({"type":"settings","revision":0,"settingsRevision":0,"settings":{}}),
            serde_json::json!({"type":"capabilities","revision":0,"capabilities":{"queries":[],"intents":[]}}),
            serde_json::json!({"type":"job","revision":0,"job":{"status":"pending","jobId":"j","progress":null}}),
        ] {
            for field in [
                "revision",
                "queueRevision",
                "libraryRevision",
                "outputRevision",
                "settingsRevision",
            ] {
                if fixture.get(field).is_some() {
                    rejects_field_values::<HostEvent>(&fixture, field, &invalid_revisions());
                }
            }
        }
        rejects_field_values::<ApplicationUpdate>(
            &serde_json::json!({"type":"events","hostEpoch":"e","events":[]}),
            "hostEpoch",
            &invalid_identifiers(),
        );
        rejects_field_values::<QueryResult>(
            &serde_json::json!({"type":"outputs","outputs":[],"revision":0}),
            "revision",
            &invalid_revisions(),
        );
    }

    #[test]
    fn optional_fields_are_omittable_but_not_explicitly_null() {
        let null = [serde_json::Value::Null];
        rejects_field_values::<DisplayTrack>(&track_fixture(), "artwork", &null);
        rejects_field_values::<PlaybackState>(&playback_fixture(), "error", &null);
        rejects_field_values::<SettingsProjection>(&serde_json::json!({}), "albumView", &null);
        rejects_field_values::<SettingsProjection>(&serde_json::json!({}), "reducedMotion", &null);
        rejects_field_values::<ApplicationQuery>(
            &serde_json::json!({"type":"albums"}),
            "sort",
            &null,
        );
    }

    #[test]
    fn nullable_metadata_numbers_preserve_signed_years_and_safe_numeric_representations() {
        let album = serde_json::json!({"id":1,"name":"Album","artist":null,"year":null,"qualityBadges":[],"sortSummary":{"artist":null,"year":null,"dateAdded":null,"name":"Album"}});
        let invalid = [
            serde_json::json!(9_007_199_254_740_992_i64),
            serde_json::json!(-9_007_199_254_740_992_i64),
            serde_json::json!(0.5),
        ];
        rejects_field_values::<DisplayAlbum>(&album, "year", &invalid);
        rejects_field_values::<AlbumSortSummary>(&album["sortSummary"], "year", &invalid);
        let mut signed = album.clone();
        signed["year"] = serde_json::json!(-1.0);
        assert!(serde_json::from_value::<DisplayAlbum>(signed).is_ok());
        let mut numbered = track_fixture();
        numbered["trackNumber"] = serde_json::json!(0.0);
        numbered["discNumber"] = serde_json::json!(1.0);
        numbered["quality"]["bitrate"] = serde_json::json!(320000.0);
        assert!(serde_json::from_value::<DisplayTrack>(numbered).is_ok());
    }

    #[test]
    fn playback_projection_rejects_volume_outside_the_slider_range() {
        rejects_field_values::<PlaybackState>(
            &playback_fixture(),
            "volume",
            &[serde_json::json!(-0.1), serde_json::json!(1.01)],
        );
    }

    #[test]
    fn metadata_labels_are_required_strings_not_bounded_identifiers() {
        for label in [String::new(), "é".repeat(1025)] {
            let album = serde_json::json!({"id":1,"name":label,"artist":null,"year":null,"qualityBadges":[],"sortSummary":{"artist":null,"year":null,"dateAdded":null,"name":label}});
            let typed: DisplayAlbum = serde_json::from_value(album.clone()).unwrap();
            assert_eq!(serde_json::to_value(typed).unwrap(), album);
            let output = serde_json::json!({"output":{"kind":"pc"},"name":label,"available":true,"capabilities":{"playback":false,"seek":false,"volume":false,"shuffle":false,"repeat":false,"equalizer":false}});
            assert!(serde_json::from_value::<AvailableOutput>(output).is_ok());
        }
    }
}

/// Only projections cross from the authoritative WebView. Replay ordering is native.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum HostUpdate {
    Projection {
        snapshot: HostSnapshot,
        #[serde(
            default,
            deserialize_with = "optional_value",
            skip_serializing_if = "Option::is_none"
        )]
        presentation: Option<HostPresentation>,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventCursor {
    #[serde(deserialize_with = "identifier")]
    pub host_epoch: String,
    #[serde(deserialize_with = "safe_revision")]
    pub revision: u64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventBatch {
    pub host_epoch: String,
    pub revision: u64,
    pub events: Vec<HostEvent>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostPresentation {
    pub queue: Vec<QueueEntry>,
    pub pinned_album_ids: Vec<u64>,
}
