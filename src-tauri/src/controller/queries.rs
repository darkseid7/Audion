//! Revision-consistent, pathless PC library projections. No controller cache owns order.
use super::{commands::error, protocol::*, query_input::CapturedLibrary};
use crate::db::{queries as db, Database};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};
#[derive(Clone, Default)]
pub struct QueryContext {
    pub host_epoch: String,
    pub revision: u64,
    pub stamp: u64,
    pub queue_revision: u64,
    pub queue: Vec<QueueEntry>,
    pub pinned_albums: Vec<u64>,
}
/// Constructed only by the origin-checked local-main IPC, never from a LAN credential.
pub(crate) struct DesktopLibraryAuthority(());
impl DesktopLibraryAuthority {
    pub(crate) fn new() -> Self {
        Self(())
    }
}
#[derive(Default)]
struct LibraryRevision {
    stamp: Option<u64>,
    revision: u64,
    pins: Vec<u64>,
    owner: Option<uuid::Uuid>,
    generation: u64,
}
#[derive(Clone)]
pub struct LibraryQueries {
    pub db: Database,
    pub resources: Arc<super::resources::ManagedResources>,
    work: Arc<tokio::sync::Semaphore>,
    clock: Arc<Mutex<LibraryRevision>>,
    local_epoch: uuid::Uuid,
    #[cfg(test)]
    projection_probe: Option<Arc<dyn Fn(&str, usize) + Send + Sync>>,
}
impl LibraryQueries {
    pub fn new(db: Database) -> Self {
        Self::with_root(db, std::path::PathBuf::new())
    }
    pub fn with_root(db: Database, root: std::path::PathBuf) -> Self {
        Self {
            resources: Arc::new(super::resources::ManagedResources::new(db.clone(), root)),
            db,
            work: Arc::new(tokio::sync::Semaphore::new(4)),
            clock: Arc::new(Mutex::new(LibraryRevision::default())),
            local_epoch: uuid::Uuid::new_v4(),
            #[cfg(test)]
            projection_probe: None,
        }
    }
    pub(crate) fn set_host_owner(&self, owner: Option<uuid::Uuid>) -> Result<(), ControlError> {
        // Ownership transitions cannot be dropped on contention. This mutex never acquires DB/state locks or spans I/O/await.
        let mut clock = self
            .clock
            .lock()
            .map_err(|_| error(ControlErrorCode::HostNotReady))?;
        if clock.owner != owner {
            clock.owner = owner;
            clock.generation = clock
                .generation
                .checked_add(1)
                .ok_or_else(|| error(ControlErrorCode::ResyncRequired))?;
        }
        Ok(())
    }
    fn clock_context(&self, clock: &LibraryRevision) -> QueryContext {
        QueryContext {
            host_epoch: format!("local:{}:{}", self.local_epoch, clock.generation),
            revision: clock.revision,
            stamp: clock.stamp.unwrap_or(0),
            pinned_albums: clock.pins.clone(),
            ..Default::default()
        }
    }
    fn advance(clock: &mut LibraryRevision) -> Result<(), ControlError> {
        if clock.revision >= 9_007_199_254_740_991 {
            return Err(error(ControlErrorCode::ResyncRequired));
        }
        clock.revision += 1;
        Ok(())
    }
    pub(crate) fn observe_stamp(&self, stamp: u64) -> Result<QueryContext, ControlError> {
        let mut clock = self
            .clock
            .try_lock()
            .map_err(|_| error(ControlErrorCode::Busy))?;
        if clock.stamp.is_some_and(|old| stamp < old) {
            return Err(error(ControlErrorCode::RevisionConflict));
        }
        if clock.stamp.is_some_and(|old| old != stamp) {
            Self::advance(&mut clock)?;
        }
        clock.stamp = Some(stamp);
        Ok(self.clock_context(&clock))
    }
    pub(crate) fn publish_pins(
        &self,
        owner: uuid::Uuid,
        pins: Vec<u64>,
    ) -> Result<QueryContext, ControlError> {
        validate_pins(&pins)?;
        let mut clock = self
            .clock
            .lock()
            .map_err(|_| error(ControlErrorCode::HostNotReady))?;
        if clock.owner != Some(owner) {
            return Err(error(ControlErrorCode::ResyncRequired));
        }
        if clock.pins != pins {
            Self::advance(&mut clock)?;
            clock.pins = pins;
        }
        Ok(self.clock_context(&clock))
    }
    pub(crate) async fn desktop_context(
        &self,
        _authority: &DesktopLibraryAuthority,
        pins: Vec<u64>,
    ) -> Result<QueryContext, ControlError> {
        validate_pins(&pins)?;
        let stamp = self.stamp_async().await?;
        self.observe_stamp(stamp)?;
        let mut clock = self
            .clock
            .try_lock()
            .map_err(|_| error(ControlErrorCode::Busy))?;
        // Only the ordered hosted publication owns presentation while attached.
        if clock.owner.is_none() && clock.pins != pins {
            Self::advance(&mut clock)?;
            clock.pins = pins;
        }
        Ok(self.clock_context(&clock))
    }
    pub(crate) async fn desktop_current(
        &self,
        authority: &DesktopLibraryAuthority,
    ) -> Result<QueryContext, ControlError> {
        let _ = authority;
        let stamp = self.stamp_async().await?;
        self.observe_stamp(stamp)
    }
    pub(crate) fn check_context(
        before: &QueryContext,
        after: &QueryContext,
    ) -> Result<(), ControlError> {
        if before.host_epoch != after.host_epoch
            || before.revision != after.revision
            || before.stamp != after.stamp
        {
            return Err(error(ControlErrorCode::RevisionConflict));
        }
        Ok(())
    }
    pub fn stamp(&self) -> Result<u64, ControlError> {
        let conn = self
            .db
            .conn
            .try_lock()
            .map_err(|_| error(ControlErrorCode::Busy))?;
        db::controller_library_stamp(&conn).map_err(db_error)
    }
    pub async fn stamp_async(&self) -> Result<u64, ControlError> {
        let permit = self
            .work
            .clone()
            .try_acquire_owned()
            .map_err(|_| error(ControlErrorCode::Busy))?;
        let library = self.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            library.stamp()
        })
        .await
        .map_err(|_| error(ControlErrorCode::HostNotReady))?
    }
    pub async fn query_library(
        &self,
        query: ApplicationQuery,
        context: QueryContext,
    ) -> Result<QueryResult, ControlError> {
        let permit = self
            .work
            .clone()
            .try_acquire_owned()
            .map_err(|_| error(ControlErrorCode::Busy))?;
        let db = self.db.clone();
        let resources = self.resources.clone();
        #[cfg(test)]
        let probe = self.projection_probe.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let window = PageWindow::new(&query, &context)?;
            let input = {
                let conn = db
                    .conn
                    .try_lock()
                    .map_err(|_| error(ControlErrorCode::Busy))?;
                if db::controller_library_stamp(&conn).map_err(db_error)? != context.stamp {
                    return Err(error(ControlErrorCode::RevisionConflict));
                }
                CapturedLibrary::read(&conn, &query, &context, window.offset, window.limit)?
            };
            #[cfg(test)]
            {
                MEMBERSHIP_VISITS.with(|v| v.set(0));
                if let Some(p) = &probe {
                    p("captured", input.tracks.len());
                }
            }
            let mut result = project(input, query, &context, &window)?;
            #[cfg(test)]
            if let Some(p) = &probe {
                p("projected", MEMBERSHIP_VISITS.with(|v| v.get()));
            }
            attach_artwork(&mut result, &resources, &db, &context)?;
            let conn = db
                .conn
                .try_lock()
                .map_err(|_| error(ControlErrorCode::Busy))?;
            if db::controller_library_stamp(&conn).map_err(db_error)? != context.stamp {
                return Err(error(ControlErrorCode::RevisionConflict));
            }
            Ok(result)
        })
        .await
        .map_err(|_| error(ControlErrorCode::HostNotReady))?
    }
}
fn validate_pins(pins: &[u64]) -> Result<(), ControlError> {
    if pins.len() > 10000
        || pins
            .iter()
            .any(|id| *id == 0 || *id > 9_007_199_254_740_991)
    {
        return Err(error(ControlErrorCode::InvalidRequest));
    }
    Ok(())
}
fn db_error(_: rusqlite::Error) -> ControlError {
    error(ControlErrorCode::ExecutionFailed)
}
fn normalize(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .filter_map(|c| {
            Some(match c {
                '\u{300}'..='\u{36f}' => return None,
                'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => 'a',
                'é' | 'è' | 'ê' | 'ë' => 'e',
                'í' | 'ì' | 'î' | 'ï' => 'i',
                'ó' | 'ò' | 'ô' | 'ö' | 'õ' => 'o',
                'ú' | 'ù' | 'û' | 'ü' => 'u',
                'ñ' => 'n',
                'ç' => 'c',
                x => x,
            })
        })
        .collect()
}
/// Deterministic natural base/case ordering, not browser-locale collation.
fn compare_text(a: &str, b: &str) -> Ordering {
    let a = normalize(a);
    let b = normalize(b);
    let mut a = a.chars().peekable();
    let mut b = b.chars().peekable();
    loop {
        match (a.peek(), b.peek()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut x = String::new();
                let mut y = String::new();
                while a.peek().is_some_and(char::is_ascii_digit) {
                    x.push(a.next().unwrap());
                }
                while b.peek().is_some_and(char::is_ascii_digit) {
                    y.push(b.next().unwrap());
                }
                let x = x.trim_start_matches('0');
                let y = y.trim_start_matches('0');
                let c = x.len().cmp(&y.len()).then_with(|| x.cmp(y));
                if c != Ordering::Equal {
                    return c;
                }
            }
            _ => {
                let c = a.next().cmp(&b.next());
                if c != Ordering::Equal {
                    return c;
                }
            }
        }
    }
}
fn optional_number<T: PartialOrd>(a: Option<T>, b: Option<T>) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, _) => Ordering::Greater,
        (_, None) => Ordering::Less,
        (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(Ordering::Equal),
    }
}
fn metadata(track: &db::Track) -> serde_json::Value {
    track
        .metadata_json
        .as_ref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(serde_json::Value::Null)
}
fn normalized_format(s: &str) -> String {
    let s = s.to_uppercase();
    if s.contains("MPEG") || s.contains("MP3") {
        "MP3".into()
    } else if s.contains("HI_RES") || s.contains("HIRES") {
        "HI-RES".into()
    } else if s.contains("LOSSLESS") {
        "LOSSLESS".into()
    } else {
        s
    }
}

/// serde_json's default Map sorts keys, whereas PC Object.values preserves insertion order.
fn ordered_values(track: &db::Track) -> Vec<serde_json::Value> {
    struct Values;
    impl<'de> serde::de::Visitor<'de> for Values {
        type Value = Vec<serde_json::Value>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("metadata object")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> Result<Self::Value, M::Error> {
            let mut fields: Vec<(String, serde_json::Value)> = Vec::new();
            while let Some((k, v)) = map.next_entry()? {
                if let Some((_, old)) = fields.iter_mut().find(|(key, _)| key == &k) {
                    *old = v;
                } else {
                    fields.push((k, v));
                }
            }
            let index = |s: &str| {
                s.parse::<u32>()
                    .ok()
                    .filter(|n| *n < u32::MAX && n.to_string() == s)
            };
            fields.sort_by(|a, b| match (index(&a.0), index(&b.0)) {
                (Some(a), Some(b)) => a.cmp(&b),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                _ => Ordering::Equal,
            });
            Ok(fields.into_iter().map(|(_, v)| v).collect())
        }
        fn visit_seq<S: serde::de::SeqAccess<'de>>(
            self,
            mut seq: S,
        ) -> Result<Self::Value, S::Error> {
            let mut values = Vec::new();
            while let Some(v) = seq.next_element()? {
                values.push(v);
            }
            Ok(values)
        }
    }
    use serde::Deserializer;
    track
        .metadata_json
        .as_deref()
        .and_then(|text| {
            serde_json::Deserializer::from_str(text)
                .deserialize_any(Values)
                .ok()
        })
        .unwrap_or_default()
}
fn metadata_number(text: &str, kind: &str) -> Option<f64> {
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        let integer_end = i;
        if kind == "rate"
            && i + 1 < bytes.len()
            && bytes[i] == b'.'
            && bytes[i + 1].is_ascii_digit()
        {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
        }
        let number = lower[start..i].parse::<f64>().ok()?;
        let left = start == 0 || !word(bytes[start - 1]);
        let right = i == bytes.len() || !word(bytes[i]);
        if kind == "year" && left && right && i - start == 4 && (1900.0..=2100.0).contains(&number)
        {
            return Some(number);
        }
        let mut suffix = i;
        while suffix < bytes.len() && bytes[suffix].is_ascii_whitespace() {
            suffix += 1;
        }
        if kind == "bits"
            && left
            && [16.0, 24.0, 32.0].contains(&number)
            && lower[suffix..].starts_with("bit")
            && (suffix + 3 == bytes.len() || !word(bytes[suffix + 3]))
        {
            return Some(number);
        }
        if kind == "rate" {
            if bytes.get(suffix) == Some(&b'k') {
                suffix += 1;
                while suffix < bytes.len() && bytes[suffix].is_ascii_whitespace() {
                    suffix += 1;
                }
                if lower[suffix..].starts_with("hz") {
                    return Some((number * 1000.0).round());
                }
            } else if left
                && (4..=6).contains(&(integer_end - start))
                && integer_end == i
                && lower[suffix..].starts_with("hz")
                && (suffix + 2 == bytes.len() || !word(bytes[suffix + 2]))
            {
                return Some(number);
            }
        }
    }
    None
}
fn audio(track: &db::Track) -> (Option<f64>, Option<f64>, Option<i64>) {
    let meta = metadata(track);
    let values = ordered_values(track);
    let mut rate = meta.get("__sample_rate_hz").and_then(|v| v.as_f64());
    let mut bits = meta.get("__bit_depth").and_then(|v| v.as_f64());
    let mut year = None;
    for v in values {
        if let Some(n) = v.as_f64() {
            if rate.is_none() && (4000.0..=768000.0).contains(&n) {
                rate = Some(n);
            }
            if year.is_none() && (1000.0..=3000.0).contains(&n) && n.fract() == 0.0 {
                year = Some(n as i64);
            }
        }
        if let Some(text) = v.as_str() {
            if rate.is_none() {
                rate = metadata_number(text, "rate");
            }
            if bits.is_none() {
                bits = metadata_number(text, "bits");
            }
            if year.is_none() {
                year = metadata_number(text, "year").map(|n| n as i64);
            }
        }
    }
    (
        rate.filter(|v| v.is_finite() && *v > 0.0),
        bits.filter(|v| v.is_finite() && *v > 0.0),
        year,
    )
}
fn badges(
    format: Option<&str>,
    bitrate: Option<i32>,
    rate: Option<f64>,
    bits: Option<f64>,
) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(f) = format {
        v.push(f.into());
    }
    if let Some(r) = rate {
        v.push(if r % 1000.0 == 0.0 {
            format!("{}kHz", r / 1000.0)
        } else {
            format!("{:.1}kHz", r / 1000.0)
        });
    }
    if let Some(b) = bits {
        v.push(format!("{b}bit"));
    } else if let Some(b) = bitrate.filter(|v| *v > 0) {
        v.push(format!("{b}kbps"));
    }
    v
}
pub(super) fn display_track(t: &db::Track, liked: bool) -> DisplayTrack {
    let format = t
        .format
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(normalized_format);
    let (rate, bits, _) = audio(t);
    DisplayTrack {
        id: t.id as u64,
        title: t.title.clone(),
        artist: t.artist.clone(),
        album_id: t.album_id.filter(|id| *id > 0).map(|id| id as u64),
        album: t.album.clone(),
        duration: t.duration.map(|d| d.max(0) as f64),
        track_number: t.track_number.filter(|n| *n >= 0).map(|n| n as u64),
        disc_number: t.disc_number.filter(|n| *n >= 0).map(|n| n as u64),
        quality: QualitySummary {
            badges: badges(format.as_deref(), t.bitrate, rate, bits),
            format,
            bitrate: t.bitrate.filter(|n| *n >= 0).map(|n| n as u64),
        },
        artwork: None,
        liked: Some(liked),
    }
}
fn date(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.timestamp_millis())
        .ok()
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|d| d.and_utc().timestamp_millis())
        })
        .or_else(|| {
            chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .ok()
                .and_then(|d| d.and_hms_opt(0, 0, 0))
                .map(|d| d.and_utc().timestamp_millis())
        })
}
struct AlbumProjection {
    display: DisplayAlbum,
    original_year: Option<i64>,
    count: u64,
    oldest: Option<i64>,
    newest: Option<i64>,
}
#[cfg(test)]
thread_local! { static MEMBERSHIP_VISITS:std::cell::Cell<usize>=const {std::cell::Cell::new(0)}; }
fn membership_visit() {
    #[cfg(test)]
    MEMBERSHIP_VISITS.with(|v| v.set(v.get() + 1));
}
fn album_projection(a: db::Album, members: &[&db::Track]) -> AlbumProjection {
    let mut formats: Vec<(String, usize)> = Vec::new();
    let mut format_positions: HashMap<String, usize> = HashMap::new();
    let (mut bitrate, mut rate, mut bits, mut year, mut newest, mut oldest) =
        (None, None, None, None, None, None);
    for t in members {
        if let Some(f) = t
            .format
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(normalized_format)
        {
            if let Some(index) = format_positions.get(&f) {
                formats[*index].1 += 1;
            } else {
                format_positions.insert(f.clone(), formats.len());
                formats.push((f, 1));
            }
        }
        bitrate = bitrate.max(t.bitrate);
        let (r, b, y) = audio(t);
        rate = match (rate, r) {
            (Some(a), Some(b)) => Some(f64::max(a, b)),
            (a, b) => a.or(b),
        };
        bits = match (bits, b) {
            (Some(a), Some(b)) => Some(f64::max(a, b)),
            (a, b) => a.or(b),
        };
        year = year.max(y);
        if let Some(d) = t.date_added.as_deref().and_then(date) {
            newest = Some(newest.map_or(d, |n: i64| n.max(d)));
            oldest = Some(oldest.map_or(d, |n: i64| n.min(d)));
        }
    }
    let mut primary = None;
    let mut max = 0;
    for (f, n) in formats {
        if n > max {
            max = n;
            primary = Some(f);
        }
    }
    let sort_year = a.original_year.or(a.year).map(i64::from).or(year);
    AlbumProjection {
        original_year: a.original_year.map(i64::from),
        count: members.len() as u64,
        oldest,
        newest,
        display: DisplayAlbum {
            id: a.id as u64,
            name: a.name.clone(),
            artist: a.artist.clone(),
            year: a.original_year.or(a.year).map(i64::from),
            quality_badges: badges(primary.as_deref(), bitrate, rate, bits),
            sort_summary: AlbumSortSummary {
                artist: a.artist,
                year: sort_year,
                date_added: newest
                    .and_then(chrono::DateTime::from_timestamp_millis)
                    .map(|d| d.to_rfc3339()),
                name: a.name,
            },
            artwork: None,
        },
    }
}
struct PageWindow {
    offset: usize,
    limit: usize,
    revision: u64,
    fingerprint: String,
}
impl PageWindow {
    fn new(query: &ApplicationQuery, c: &QueryContext) -> Result<Self, ControlError> {
        let mut fingerprint =
            serde_json::to_value(query).map_err(|_| error(ControlErrorCode::InvalidRequest))?;
        let object = fingerprint.as_object_mut().unwrap();
        if matches!(query, ApplicationQuery::AlbumTracks { .. }) {
            let liked = object.get("likedOnly").and_then(|v| v.as_bool()).unwrap_or(false);
            object.insert("likedOnly".into(), serde_json::json!(liked));
        }
        let cursor = object
            .remove("cursor")
            .and_then(|v| v.as_str().map(str::to_owned));
        let limit = object.get("limit").and_then(|v| v.as_u64()).unwrap_or(200) as usize;
        if !(1..=200).contains(&limit) {
            return Err(error(ControlErrorCode::InvalidRequest));
        }
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&(fingerprint, &c.pinned_albums)).unwrap())
        );
        let revision = if matches!(query, ApplicationQuery::Queue { .. }) {
            c.queue_revision
        } else {
            c.revision
        };
        let mut offset = 0usize;
        if let Some(cursor) = cursor {
            let bytes = URL_SAFE_NO_PAD
                .decode(cursor)
                .map_err(|_| error(ControlErrorCode::InvalidRequest))?;
            let (epoch, rev, stamp, hash, at): (String, u64, u64, String, usize) =
                serde_json::from_slice(&bytes)
                    .map_err(|_| error(ControlErrorCode::InvalidRequest))?;
            if epoch != c.host_epoch || rev != revision || stamp != c.stamp || hash != fingerprint {
                return Err(error(ControlErrorCode::RevisionConflict));
            }
            offset = at;
        }
        Ok(Self {
            offset,
            limit,
            revision,
            fingerprint,
        })
    }
    fn next(&self, c: &QueryContext, len: usize) -> Result<Option<String>, ControlError> {
        if self.offset > len {
            return Err(error(ControlErrorCode::InvalidRequest));
        }
        Ok((self.offset.saturating_add(self.limit) < len).then(|| {
            URL_SAFE_NO_PAD.encode(
                serde_json::to_vec(&(
                    &c.host_epoch,
                    self.revision,
                    c.stamp,
                    &self.fingerprint,
                    self.offset + self.limit,
                ))
                .unwrap(),
            )
        }))
    }
}
fn project(
    input: CapturedLibrary,
    query: ApplicationQuery,
    c: &QueryContext,
    window: &PageWindow,
) -> Result<QueryResult, ControlError> {
    let offset = window.offset;
    let limit = window.limit;
    let revision = window.revision;
    let page = |len| window.next(c, len);
    let CapturedLibrary {
        mut tracks,
        albums,
        liked,
        liked_albums,
        playlists,
    } = input;
    macro_rules! paged {
        ($items:expr,$variant:ident) => {{
            let items = $items;
            let next_cursor = page(items.len())?;
            Ok(QueryResult::$variant {
                page: Page {
                    items: items.into_iter().skip(offset).take(limit).collect(),
                    next_cursor,
                    revision,
                },
            })
        }};
    }
    if matches!(query, ApplicationQuery::Queue { .. }) {
        let next_cursor = page(c.queue.len())?;
        let items = c
            .queue
            .iter()
            .skip(offset)
            .take(limit)
            .zip(tracks.iter())
            .map(|(entry, track)| QueueEntry {
                entry_id: entry.entry_id.clone(),
                track: display_track(track, liked.contains(&track.id)),
            })
            .collect();
        return Ok(QueryResult::Queue {
            page: Page {
                items,
                next_cursor,
                revision,
            },
        });
    }
    // Preserve PC SQL encounter order for dominant-format ties; add ID for equal rows.
    if !matches!(
        query,
        ApplicationQuery::PlaylistTracks { .. } | ApplicationQuery::LikedTracks { .. }
    ) {
        tracks.sort_by(|a, b| {
            a.artist
                .cmp(&b.artist)
                .then(a.album.cmp(&b.album))
                .then(a.disc_number.cmp(&b.disc_number))
                .then(a.track_number.cmp(&b.track_number))
                .then(a.title.cmp(&b.title))
                .then(a.id.cmp(&b.id))
        });
    }
    let mut groups: HashMap<i64, Vec<&db::Track>> = HashMap::new();
    if matches!(
        query,
        ApplicationQuery::Albums { .. }
            | ApplicationQuery::AlbumDetail { .. }
            | ApplicationQuery::ArtistAlbums { .. }
            | ApplicationQuery::Search { .. }
    ) {
        for track in &tracks {
            membership_visit();
            if let Some(id) = track.album_id {
                groups.entry(id).or_default().push(track);
            }
        }
    }
    let summarize = |a: db::Album| {
        let members = groups.get(&a.id).map(Vec::as_slice).unwrap_or(&[]);
        album_projection(a, members)
    };
    let display = |t: &db::Track| display_track(t, liked.contains(&t.id));
    macro_rules! track_page {
        ($items:expr,$variant:ident) => {{
            let items = $items;
            let next_cursor = page(items.len())?;
            Ok(QueryResult::$variant {
                page: Page {
                    items: items
                        .into_iter()
                        .skip(offset)
                        .take(limit)
                        .map(display)
                        .collect(),
                    next_cursor,
                    revision,
                },
            })
        }};
    }

    match query {
        ApplicationQuery::Tracks { .. } => {
            track_page!(tracks.iter().collect::<Vec<_>>(), Tracks)
        }
        ApplicationQuery::AlbumTracks { album_id, liked_only, .. } => {
            let mut ts = tracks
                .iter()
                .filter(|t| t.album_id == Some(album_id as i64) && (!liked_only.unwrap_or(false) || liked.contains(&t.id)))
                .collect::<Vec<_>>();
            ts.sort_by_key(|t| (t.disc_number, t.track_number, &t.title, t.id));
            track_page!(ts, AlbumTracks)
        }
        ApplicationQuery::ArtistTracks { .. } => {
            track_page!(tracks.iter().collect::<Vec<_>>(), ArtistTracks)
        }
        ApplicationQuery::LikedTracks { .. } => {
            track_page!(tracks.iter().collect::<Vec<_>>(), LikedTracks)
        }
        ApplicationQuery::PlaylistTracks { .. } => {
            track_page!(tracks.iter().collect::<Vec<_>>(), PlaylistTracks)
        }
        ApplicationQuery::Playlists { .. } => paged!(playlists, Playlists),
        ApplicationQuery::Artists { .. } => paged!(artists(&tracks), Artists),
        ApplicationQuery::Albums {
            sort,
            liked_only,
            text,
            ..
        } => {
            let text = text.map(|t| normalize(t.trim()));
            let mut albums = albums
                .into_iter()
                .filter(|a| {
                    (!liked_only.unwrap_or(false) || liked_albums.contains(&a.id))
                        && text.as_ref().is_none_or(|s| {
                            normalize(&a.name).contains(s)
                                || normalize(a.artist.as_deref().unwrap_or("")).contains(s)
                        })
                })
                .map(summarize)
                .collect::<Vec<_>>();
            let pinned = c.pinned_albums.iter().copied().collect::<HashSet<_>>();
            let sort = sort.unwrap_or(AlbumSort::NameAsc);
            albums.sort_by(|a, b| {
                let a_id = a.display.id;
                let b_id = b.display.id;
                pinned
                    .contains(&b_id)
                    .cmp(&pinned.contains(&a_id))
                    .then_with(|| (b.count > 0).cmp(&(a.count > 0)))
                    .then_with(|| {
                        let artist = |a: &AlbumProjection| {
                            a.display
                                .artist
                                .as_deref()
                                .filter(|s| !s.trim().is_empty())
                                .map(str::trim)
                                .unwrap_or("ZZZ_UNKNOWN_ARTIST")
                                .to_owned()
                        };
                        match sort {
                            AlbumSort::ArtistAsc => compare_text(&artist(a), &artist(b)),
                            AlbumSort::ArtistDesc => compare_text(&artist(b), &artist(a)),
                            AlbumSort::YearAsc => optional_number(
                                a.display.sort_summary.year,
                                b.display.sort_summary.year,
                            ),
                            AlbumSort::YearDesc => optional_number(
                                b.display.sort_summary.year,
                                a.display.sort_summary.year,
                            ),
                            AlbumSort::AddedAsc => optional_number(a.oldest, b.oldest),
                            AlbumSort::AddedDesc => optional_number(b.newest, a.newest),
                            AlbumSort::NameDesc => compare_text(&b.display.name, &a.display.name),
                            AlbumSort::NameAsc => compare_text(&a.display.name, &b.display.name),
                        }
                    })
                    .then_with(|| {
                        compare_text(
                            a.display.artist.as_deref().unwrap_or(""),
                            b.display.artist.as_deref().unwrap_or(""),
                        )
                    })
                    .then_with(|| compare_text(&a.display.name, &b.display.name))
                    .then(a_id.cmp(&b_id))
            });
            paged!(
                albums.into_iter().map(|a| a.display).collect::<Vec<_>>(),
                Albums
            )
        }
        ApplicationQuery::AlbumDetail { album_id } => {
            let a = albums
                .into_iter()
                .find(|a| a.id == album_id as i64)
                .ok_or_else(|| error(ControlErrorCode::NotFound))?;
            let a = summarize(a);
            Ok(QueryResult::AlbumDetail {
                detail: AlbumDetail {
                    album: a.display,
                    original_year: a.original_year,
                    track_count: a.count,
                    liked: liked_albums.contains(&(album_id as i64)),
                },
                revision,
            })
        }
        ApplicationQuery::ArtistAlbums { artist_name, .. } => {
            let album_ids = tracks
                .iter()
                .filter(|t| t.artist.as_deref() == Some(&artist_name))
                .filter_map(|t| t.album_id)
                .collect::<HashSet<_>>();
            let mut albums = albums
                .into_iter()
                .filter(|a| album_ids.contains(&a.id))
                .map(|a| summarize(a).display)
                .collect::<Vec<_>>();
            albums.sort_by(|a, b| compare_text(&a.name, &b.name).then(a.id.cmp(&b.id)));
            paged!(albums, ArtistAlbums)
        }
        ApplicationQuery::Search { text, .. } => {
            let needle = normalize(text.trim());
            let mut matches = Vec::new();
            for t in &tracks {
                if [t.title.as_deref(), t.artist.as_deref(), t.album.as_deref()]
                    .into_iter()
                    .flatten()
                    .any(|s| normalize(s).contains(&needle))
                {
                    matches.push(SearchMatch::Track { track: display(t) });
                }
            }
            for a in albums {
                if normalize(&a.name).contains(&needle)
                    || normalize(a.artist.as_deref().unwrap_or("")).contains(&needle)
                {
                    matches.push(SearchMatch::Album {
                        album: summarize(a).display,
                    });
                }
            }
            for a in artists(&tracks) {
                if normalize(&a.name).contains(&needle) {
                    matches.push(SearchMatch::Artist { artist: a });
                }
            }
            paged!(matches, Search)
        }
        _ => Err(error(ControlErrorCode::Unsupported)),
    }
}
fn artists(tracks: &[db::Track]) -> Vec<DisplayArtist> {
    let mut map: HashMap<String, (u64, HashSet<i64>)> = HashMap::new();
    for t in tracks {
        if let Some(name) = t
            .artist
            .as_ref()
            .filter(|n| !n.is_empty() && n.len() <= MAX_IDENTIFIER_BYTES)
        {
            let v = map.entry(name.clone()).or_default();
            v.0 += 1;
            if let Some(id) = t.album_id {
                v.1.insert(id);
            }
        }
    }
    let mut items = map
        .into_iter()
        .map(|(name, (track_count, albums))| DisplayArtist {
            name,
            track_count,
            album_count: albums.len() as u64,
            artwork: None,
        })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| compare_text(&a.name, &b.name).then(a.name.cmp(&b.name)));
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::Mutex;
    #[tokio::test]
    async fn liked_album_tracks_filter_before_page_and_bind_cursor() {
        let q = fixture();
        {
            let conn = q.db.conn.lock().unwrap();
            conn.execute_batch("DELETE FROM tracks; DELETE FROM liked_tracks;").unwrap();
            for id in 1..=205 {
                conn.execute("INSERT INTO tracks(id,path,title,album_id,track_number) VALUES(?1,?2,'Fixture',1,?1)", rusqlite::params![id, format!("fixture-{id}.flac")]).unwrap();
                if id > 200 { conn.execute("INSERT INTO liked_tracks(track_id) VALUES(?1)", [id]).unwrap(); }
            }
        }
        let c = context(&q);
        let get = |liked: Option<bool>, cursor: Option<String>| {
            let mut v = serde_json::json!({"type":"album_tracks","albumId":1,"limit":2});
            if let Some(liked) = liked { v["likedOnly"] = serde_json::json!(liked); }
            if let Some(cursor) = cursor { v["cursor"] = serde_json::json!(cursor); }
            query(v)
        };
        let QueryResult::AlbumTracks { page: all } = q.query_library(get(None,None),c.clone()).await.unwrap() else { panic!() };
        assert_eq!(all.items.iter().map(|t| t.id).collect::<Vec<_>>(),vec![1,2]);
        let QueryResult::AlbumTracks { page: same } = q.query_library(get(Some(false),all.next_cursor.clone()),c.clone()).await.unwrap() else { panic!() };
        assert_eq!(same.items[0].id,3);
        let QueryResult::AlbumTracks { page: first } = q.query_library(get(Some(true),None),c.clone()).await.unwrap() else { panic!() };
        assert_eq!(first.items.iter().map(|t| t.id).collect::<Vec<_>>(),vec![201,202]);
        let cursor=first.next_cursor.unwrap();
        assert_eq!(q.query_library(get(Some(false),Some(cursor.clone())),c.clone()).await.unwrap_err().code,ControlErrorCode::RevisionConflict);
        let mut changed=c.clone();changed.revision+=1;
        assert_eq!(q.query_library(get(Some(true),Some(cursor.clone())),changed).await.unwrap_err().code,ControlErrorCode::RevisionConflict);
        let QueryResult::AlbumTracks { page: second } = q.query_library(get(Some(true),Some(cursor)),c.clone()).await.unwrap() else { panic!() };
        assert_eq!(second.items.iter().map(|t| t.id).collect::<Vec<_>>(),vec![203,204]);
        let QueryResult::AlbumTracks { page: last } = q.query_library(get(Some(true),second.next_cursor),c).await.unwrap() else { panic!() };
        assert_eq!(last.items[0].id,205);assert!(last.next_cursor.is_none());
        q.db.conn.lock().unwrap().execute("DELETE FROM liked_tracks",[]).unwrap();
        let QueryResult::AlbumTracks { page: empty } = q.query_library(get(Some(true),None),context(&q)).await.unwrap() else { panic!() };
        assert!(empty.items.is_empty());assert!(empty.next_cursor.is_none());
    }
    fn fixture() -> LibraryQueries {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::init_schema(&conn).unwrap();
        conn.execute_batch(r#"INSERT INTO albums(id,name,artist,year,original_year) VALUES(1,'Album 10','Ártist',2005,1999),(2,'Album 2','artist',2000,NULL);
        INSERT INTO tracks(id,path,title,artist,album,album_id,format,bitrate,metadata_json,date_added) VALUES
        (1,'E:/private/audio.flac','Song','Ártist','Album 10',1,'FLAC',900,'{"__sample_rate_hz":96000,"__bit_depth":24}','2020-01-01'),
        (2,'E:/private/audio2.flac','Second','artist','Album 2',2,'MPEG',320,'{}','2021-01-01');"#).unwrap();
        LibraryQueries::new(Database {
            conn: Arc::new(Mutex::new(conn)),
        })
    }
    fn context(q: &LibraryQueries) -> QueryContext {
        QueryContext {
            host_epoch: "epoch".into(),
            revision: 1,
            stamp: q.stamp().unwrap(),
            ..Default::default()
        }
    }
    fn query(value: serde_json::Value) -> ApplicationQuery {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn owner_detach_is_not_lost_when_a_local_clock_observation_is_finishing() {
        let q = fixture();
        q.set_host_owner(Some(uuid::Uuid::new_v4())).unwrap();
        let held = q.clock.lock().unwrap();
        let worker = q.clone();
        let (sent, received) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            sent.send(worker.set_host_owner(None)).unwrap();
        });
        // The clock section is tiny and never acquires DB/state or awaits; detach must not silently fail Busy.
        let early = received.recv_timeout(std::time::Duration::from_millis(30));
        drop(held);
        let result = early.unwrap_or_else(|_| {
            received
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap()
        });
        thread.join().unwrap();
        assert!(result.is_ok());
        assert!(q.clock.lock().unwrap().owner.is_none());
    }

    #[test]
    fn dropped_revision_callers_keep_four_global_worker_permits_until_work_finishes() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .max_blocking_threads(1)
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let q = Arc::new(fixture());
            let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let blocker = tokio::task::spawn_blocking(move || {
                let _ = entered_tx.send(());
                release_rx.recv().unwrap();
            });
            entered_rx.await.unwrap();
            let pending = (0..4)
                .map(|_| {
                    let q = q.clone();
                    tokio::spawn(async move { q.stamp_async().await })
                })
                .collect::<Vec<_>>();
            tokio::time::timeout(std::time::Duration::from_secs(1), async {
                while q.work.available_permits() != 0 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            for task in pending {
                task.abort();
                let _ = task.await;
            }
            assert_eq!(
                q.stamp_async().await.unwrap_err().code,
                ControlErrorCode::Busy
            );
            release_tx.send(()).unwrap();
            blocker.await.unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(1), async {
                while q.work.available_permits() != 4 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert!(q.stamp_async().await.is_ok());
        });
    }

    #[tokio::test]
    async fn cpu_projection_releases_db_and_rejects_a_write_after_capture() {
        let mut q = fixture();
        let db = q.db.clone();
        q.projection_probe = Some(Arc::new(move |stage, _| {
            if stage == "captured" {
                let conn = db
                    .conn
                    .try_lock()
                    .expect("projection still owns desktop DB");
                conn.execute("UPDATE tracks SET title='after capture' WHERE id=1", [])
                    .unwrap();
            }
        }));
        let result = q
            .query_library(
                query(serde_json::json!({"type":"albums","limit":1})),
                context(&q),
            )
            .await;
        assert_eq!(result.unwrap_err().code, ControlErrorCode::RevisionConflict);
    }
    #[tokio::test]
    async fn desktop_db_is_available_while_projection_is_paused_at_a_controlled_barrier() {
        let mut q = fixture();
        let db = q.db.clone();
        let context = context(&q);
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let entered = Mutex::new(Some(entered_tx));
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        let resume = Mutex::new(resume_rx);
        q.projection_probe = Some(Arc::new(move |stage, _| {
            if stage == "captured" {
                entered.lock().unwrap().take().unwrap().send(()).unwrap();
                resume
                    .lock()
                    .unwrap()
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .unwrap();
            }
        }));
        let pending = tokio::spawn(async move {
            q.query_library(
                query(serde_json::json!({"type":"albums","limit":1})),
                context,
            )
            .await
        });
        entered_rx.await.unwrap();
        {
            let conn = db
                .conn
                .try_lock()
                .expect("desktop query must not wait for projection CPU");
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get::<_, usize>(0))
                    .unwrap(),
                2
            );
            conn.execute(
                "UPDATE tracks SET title='committed during projection' WHERE id=1",
                [],
            )
            .unwrap();
        }
        resume_tx.send(()).unwrap();
        assert_eq!(
            pending.await.unwrap().unwrap_err().code,
            ControlErrorCode::RevisionConflict
        );
    }

    #[tokio::test]
    async fn album_membership_is_linear_for_large_synthetic_one_item_pages() {
        let mut q = fixture();
        q.db.conn.lock().unwrap().execute_batch("WITH RECURSIVE n(x) AS(SELECT 100 UNION ALL SELECT x+1 FROM n WHERE x<1099) INSERT INTO albums(id,name) SELECT x,printf('Album %04d',x) FROM n;WITH RECURSIVE n(x) AS(SELECT 100 UNION ALL SELECT x+1 FROM n WHERE x<10099) INSERT INTO tracks(id,path,album_id,title,metadata_json) SELECT x,printf('synthetic-%d',x),100+(x-100)/10,'Title','{}' FROM n;").unwrap();
        q.projection_probe = Some(Arc::new(|stage, count| {
            if stage == "projected" {
                assert_eq!(
                    count, 10002,
                    "album membership must visit each track once, not once per album"
                );
            }
        }));
        let QueryResult::Albums { page } = q
            .query_library(
                query(serde_json::json!({"type":"albums","limit":1})),
                context(&q),
            )
            .await
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(page.items.len(), 1);
        assert!(page.next_cursor.is_some());
    }
    #[tokio::test]
    async fn playlist_and_detail_queries_do_not_load_unrelated_track_rows() {
        let q = fixture();
        q.db.conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE tracks SET duration='unrelated-invalid-integer' WHERE id=2",
                [],
            )
            .unwrap();
        for request in [
            serde_json::json!({"type":"playlists","limit":1}),
            serde_json::json!({"type":"album_detail","albumId":1}),
        ] {
            assert!(q.query_library(query(request), context(&q)).await.is_ok());
        }
    }

    #[tokio::test]
    async fn local_and_host_share_clock_and_hosted_pins_cannot_be_overwritten() {
        let q = fixture();
        let authority = DesktopLibraryAuthority::new();
        let local = q.desktop_context(&authority, vec![1]).await.unwrap();
        let epoch = uuid::Uuid::new_v4();
        q.set_host_owner(Some(epoch)).unwrap();
        let hosted = q.publish_pins(epoch, vec![2]).unwrap();
        assert!(hosted.revision > local.revision);
        let delayed = q.desktop_context(&authority, vec![1]).await.unwrap();
        assert_eq!(delayed.pinned_albums, vec![2]);
        assert_eq!(delayed.revision, hosted.revision);
        assert_ne!(delayed.host_epoch, local.host_epoch);
        q.db.conn
            .lock()
            .unwrap()
            .execute("UPDATE tracks SET title='Changed' WHERE id=1", [])
            .unwrap();
        let fresh = q.desktop_context(&authority, vec![1]).await.unwrap();
        assert!(fresh.revision > hosted.revision);
        q.set_host_owner(None).unwrap();
        let detached = q.desktop_context(&authority, vec![1]).await.unwrap();
        assert!(detached.revision > fresh.revision);
        assert_ne!(detached.host_epoch, fresh.host_epoch);
    }

    #[test]
    fn shared_mvp_queries_are_closed() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/controller/library-queries.json"
        ))
        .unwrap();
        for q in fixture["valid"].as_array().unwrap() {
            assert!(
                serde_json::from_value::<ApplicationQuery>(q.clone()).is_ok(),
                "{q}"
            );
        }
        for q in fixture["invalid"].as_array().unwrap() {
            assert!(
                serde_json::from_value::<ApplicationQuery>(q.clone()).is_err(),
                "{q}"
            );
        }
    }
    #[tokio::test]
    async fn mvp_details_filters_pages_and_duplicate_queue_occurrences() {
        let q = fixture();
        q.db.conn.lock().unwrap().execute_batch("INSERT INTO liked_tracks(track_id) VALUES(1);INSERT INTO liked_albums(album_id) VALUES(1);INSERT INTO playlists(id,name) VALUES(1,'List');INSERT INTO playlist_tracks(playlist_id,track_id,position) VALUES(1,2,0),(1,1,1);").unwrap();
        for input in [
            serde_json::json!({"type":"album_detail","albumId":1}),
            serde_json::json!({"type":"playlists"}),
            serde_json::json!({"type":"playlist_tracks","playlistId":1}),
            serde_json::json!({"type":"artist_tracks","artistName":"Ártist"}),
            serde_json::json!({"type":"liked_tracks"}),
            serde_json::json!({"type":"search","text":"Song"}),
            serde_json::json!({"type":"artists"}),
        ] {
            let result = q.query_library(query(input), context(&q)).await.unwrap();
            assert!(!serde_json::to_string(&result).unwrap().contains("private"));
        }
        let QueryResult::Albums { page } = q
            .query_library(
                query(serde_json::json!({"type":"albums","likedOnly":true})),
                context(&q),
            )
            .await
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(page.items.len(), 1);
        let QueryResult::Tracks { page } = q
            .query_library(query(serde_json::json!({"type":"tracks"})), context(&q))
            .await
            .unwrap()
        else {
            panic!()
        };
        let mut c = context(&q);
        c.queue = vec![
            QueueEntry {
                entry_id: "first".into(),
                track: page.items[0].clone(),
            },
            QueueEntry {
                entry_id: "second".into(),
                track: page.items[0].clone(),
            },
        ];
        let QueryResult::Queue { page } = q
            .query_library(
                query(serde_json::json!({"type":"queue","limit":1})),
                c.clone(),
            )
            .await
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(page.items[0].entry_id, "first");
        let QueryResult::Queue{page}=q.query_library(query(serde_json::json!({"type":"queue","limit":1,"cursor":page.next_cursor.unwrap()})),c).await.unwrap() else{panic!()};
        assert_eq!(page.items[0].entry_id, "second");
    }

    #[tokio::test]
    async fn deleted_queued_tracks_fail_explicitly_instead_of_disappearing() {
        let q = fixture();
        let QueryResult::Tracks { page } = q
            .query_library(query(serde_json::json!({"type":"tracks"})), context(&q))
            .await
            .unwrap()
        else {
            panic!()
        };
        let track = page.items[0].clone();
        q.db.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM tracks WHERE id=?1", [track.id])
            .unwrap();
        let mut c = context(&q);
        c.queue = vec![QueueEntry {
            entry_id: "deleted-occurrence".into(),
            track,
        }];
        assert_eq!(
            q.query_library(query(serde_json::json!({"type":"queue"})), c)
                .await
                .unwrap_err()
                .code,
            ControlErrorCode::NotFound
        );
    }
    #[tokio::test]
    async fn concurrent_write_and_pins_cannot_mix_cursor_pages() {
        let q = fixture();
        let c = context(&q);
        let QueryResult::Albums { page } = q
            .query_library(
                query(serde_json::json!({"type":"albums","limit":1})),
                c.clone(),
            )
            .await
            .unwrap()
        else {
            panic!()
        };
        let mut pins = c.clone();
        pins.pinned_albums = vec![1];
        assert_eq!(q.query_library(query(serde_json::json!({"type":"albums","limit":1,"cursor":page.next_cursor.unwrap()})),pins).await.unwrap_err().code,ControlErrorCode::RevisionConflict);
        let db = q.db.clone();
        std::thread::spawn(move || {
            db.conn
                .lock()
                .unwrap()
                .execute("UPDATE albums SET year=2025 WHERE id=1", [])
                .unwrap();
        })
        .join()
        .unwrap();
        assert_eq!(
            q.query_library(query(serde_json::json!({"type":"albums"})), c)
                .await
                .unwrap_err()
                .code,
            ControlErrorCode::RevisionConflict
        );
        let held = q.work.clone().acquire_many_owned(4).await.unwrap();
        assert_eq!(
            q.query_library(query(serde_json::json!({"type":"tracks"})), context(&q))
                .await
                .unwrap_err()
                .code,
            ControlErrorCode::Busy
        );
        drop(held);
    }

    #[test]
    fn metadata_fallback_preserves_pc_object_order_and_unit_boundaries() {
        let q = fixture();
        let mut track = db::get_track_by_id(&q.db.conn.lock().unwrap(), 1)
            .unwrap()
            .unwrap();
        track.metadata_json=Some(r#"{"Z rate":"44.1 kHz","A rate":"96kHz","Z bits":"16 bit","A bits":"24 bit","Z year":"1999","A year":"2020"}"#.into());
        assert_eq!(audio(&track), (Some(44100.0), Some(16.0), Some(1999)));
        track.metadata_json = Some(r#"{"comment":"word1999word 24 bitsy 96000 hzy"}"#.into());
        assert_eq!(audio(&track), (None, None, None));
    }
    #[test]
    fn natural_accented_and_numeric_names_have_stable_ties() {
        assert_eq!(compare_text("Album 2", "album 10"), Ordering::Less);
        assert_eq!(compare_text("Álbum", "album"), Ordering::Equal);
        assert_eq!(compare_text("é", "e\u{301}"), Ordering::Equal);
    }
    #[tokio::test]
    async fn unloaded_albums_have_host_badges_and_order() {
        let q = fixture();
        let result = q
            .query_library(
                query(serde_json::json!({"type":"albums","sort":"name-asc","limit":1})),
                context(&q),
            )
            .await
            .unwrap();
        let QueryResult::Albums { page } = result else {
            panic!()
        };
        assert_eq!(page.items[0].id, 2);
        assert_eq!(page.items[0].quality_badges, vec!["MP3", "320kbps"]);
        let mut c = context(&q);
        c.pinned_albums = vec![1];
        let QueryResult::Albums { page } = q
            .query_library(
                query(serde_json::json!({"type":"albums","sort":"name-asc"})),
                c,
            )
            .await
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(page.items[0].id, 1);
        assert_eq!(page.items[0].quality_badges, vec!["FLAC", "96kHz", "24bit"]);
        assert_eq!(page.items[0].sort_summary.year, Some(1999));
    }
    #[tokio::test]
    async fn changing_revision_rejects_cursor() {
        let q = fixture();
        let c = context(&q);
        let QueryResult::Tracks { page } = q
            .query_library(
                query(serde_json::json!({"type":"tracks","limit":1})),
                c.clone(),
            )
            .await
            .unwrap()
        else {
            panic!()
        };
        q.db.conn
            .lock()
            .unwrap()
            .execute("UPDATE tracks SET title='Changed' WHERE id=2", [])
            .unwrap();
        let mut changed = context(&q);
        changed.revision += 1;
        let err=q.query_library(query(serde_json::json!({"type":"tracks","limit":1,"cursor":page.next_cursor.unwrap()})),changed).await.unwrap_err();
        assert_eq!(err.code, ControlErrorCode::RevisionConflict);
    }
    #[tokio::test]
    async fn wire_projection_omits_private_sources() {
        let q = fixture();
        let result = q
            .query_library(query(serde_json::json!({"type":"tracks"})), context(&q))
            .await
            .unwrap();
        let json = serde_json::to_value(result).unwrap();
        let serialized = &json["page"]["items"][0];
        assert!(serialized.get("path").is_none());
        for key in [
            "art_path",
            "local_src",
            "token",
            "cover_url",
            "stream_url",
            "metadata_json",
        ] {
            assert!(serialized.get(key).is_none());
        }
        assert!(!json.to_string().contains("E:/private"));
    }

    #[test]
    fn same_path_cover_rewrite_is_transactional_not_a_generic_noop() {
        let q = fixture();
        let conn = q.db.conn.lock().unwrap();
        db::update_track_cover_path(&conn, 1, Some("managed/1.png")).unwrap();
        let stamp = db::controller_library_stamp(&conn).unwrap();
        db::update_track_cover_path(&conn, 1, Some("managed/1.png")).unwrap();
        assert_eq!(db::controller_library_stamp(&conn).unwrap(), stamp + 1);
        conn.execute_batch("BEGIN").unwrap();
        db::update_track_cover_path(&conn, 1, Some("managed/1.png")).unwrap();
        conn.execute_batch("ROLLBACK").unwrap();
        assert_eq!(db::controller_library_stamp(&conn).unwrap(), stamp + 1);
        db::update_track_cover_path(&conn, 999, Some("managed/999.png")).unwrap();
        db::update_track_cover_path(&conn, 2, None).unwrap();
        assert_eq!(db::controller_library_stamp(&conn).unwrap(), stamp + 1);
        conn.execute_batch("CREATE TRIGGER reject_cover BEFORE UPDATE OF track_cover_path ON tracks BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
        assert!(db::update_track_cover_path(&conn, 1, Some("managed/1.png")).is_err());
        assert_eq!(db::controller_library_stamp(&conn).unwrap(), stamp + 1);
    }
    #[test]
    fn existing_pre_controller_schema_upgrades_without_replacing_library_rows() {
        let q = fixture();
        let conn = q.db.conn.lock().unwrap();
        for table in [
            "tracks",
            "albums",
            "playlists",
            "playlist_tracks",
            "liked_tracks",
            "liked_albums",
        ] {
            for event in ["INSERT", "UPDATE", "DELETE"] {
                conn.execute_batch(&format!("DROP TRIGGER controller_revision_{table}_{event}"))
                    .unwrap();
            }
        }
        conn.execute_batch("DROP TABLE controller_library_revision")
            .unwrap();
        crate::db::schema::init_schema(&conn).unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get::<_, u64>(0))
                .unwrap(),
            2
        );
        let before = db::controller_library_stamp(&conn).unwrap();
        conn.execute("UPDATE tracks SET title='after upgrade' WHERE id=1", [])
            .unwrap();
        assert!(db::controller_library_stamp(&conn).unwrap() > before);
    }
    #[test]
    fn committed_raw_writes_advance_stamp_but_rollback_and_noop_do_not() {
        let q = fixture();
        let before = q.stamp().unwrap();
        {
            let conn = q.db.conn.lock().unwrap();
            conn.execute_batch("BEGIN;UPDATE tracks SET title='rolled back';ROLLBACK;")
                .unwrap();
        }
        assert_eq!(q.stamp().unwrap(), before);
        q.db.conn
            .lock()
            .unwrap()
            .execute("UPDATE tracks SET title=title", [])
            .unwrap();
        assert_eq!(q.stamp().unwrap(), before);
        q.db.conn
            .lock()
            .unwrap()
            .execute("UPDATE tracks SET title='committed' WHERE id=1", [])
            .unwrap();
        assert!(q.stamp().unwrap() > before);
        crate::db::schema::init_schema(&q.db.conn.lock().unwrap()).unwrap();
        let before = q.stamp().unwrap();
        crate::db::schema::init_schema(&q.db.conn.lock().unwrap()).unwrap();
        assert_eq!(q.stamp().unwrap(), before);
    }
}

fn attach_artwork(
    result: &mut QueryResult,
    resources: &super::resources::ManagedResources,
    db: &Database,
    c: &QueryContext,
) -> Result<(), ControlError> {
    use super::resources::Entity;
    let register = |entity| -> Result<Option<ArtworkReference>, ControlError> {
        let source = {
            let conn = db
                .conn
                .try_lock()
                .map_err(|_| error(ControlErrorCode::Busy))?;
            if db::controller_library_stamp(&conn).map_err(db_error)? != c.stamp {
                return Err(error(ControlErrorCode::RevisionConflict));
            }
            resources.capture(&conn, entity)?
        };
        // Encoded-source hashing and registry work occur only after the DB guard is gone.
        source
            .map(|source| resources.register_captured(source, c))
            .transpose()
    };
    let album_art = |a: &mut DisplayAlbum| -> Result<(), ControlError> {
        a.artwork = register(Entity::Album(a.id))?;
        if a.artwork.is_none() {
            let id = {
                let conn = db
                    .conn
                    .try_lock()
                    .map_err(|_| error(ControlErrorCode::Busy))?;
                if db::controller_library_stamp(&conn).map_err(db_error)? != c.stamp {
                    return Err(error(ControlErrorCode::RevisionConflict));
                }
                conn.query_row("SELECT id FROM tracks WHERE album_id=?1 ORDER BY disc_number,track_number,title,id LIMIT 1",[a.id],|r|r.get::<_,u64>(0)).optional().map_err(db_error)?
            };
            if let Some(id) = id {
                a.artwork = register(Entity::Track(id))?;
            }
        }
        Ok(())
    };
    let track_art = |t: &mut DisplayTrack| -> Result<(), ControlError> {
        t.artwork = register(Entity::Track(t.id))?;
        if t.artwork.is_none() {
            if let Some(id) = t.album_id {
                t.artwork = register(Entity::Album(id))?;
            }
        }
        Ok(())
    };
    match result {
        QueryResult::Albums { page } | QueryResult::ArtistAlbums { page } => {
            for a in &mut page.items {
                album_art(a)?;
            }
        }
        QueryResult::AlbumDetail { detail, .. } => album_art(&mut detail.album)?,
        QueryResult::Tracks { page }
        | QueryResult::AlbumTracks { page }
        | QueryResult::ArtistTracks { page }
        | QueryResult::PlaylistTracks { page }
        | QueryResult::LikedTracks { page } => {
            for t in &mut page.items {
                track_art(t)?;
            }
        }
        QueryResult::Queue { page } => {
            for e in &mut page.items {
                track_art(&mut e.track)?;
            }
        }
        QueryResult::Search { page } => {
            for m in &mut page.items {
                match m {
                    SearchMatch::Track { track } => track_art(track)?,
                    SearchMatch::Album { album } => album_art(album)?,
                    _ => {}
                }
            }
        }
        _ => {}
    }
    Ok(())
}
