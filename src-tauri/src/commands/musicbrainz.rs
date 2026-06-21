//! MusicBrainz integration commands
//!
//! Provides artist metadata enrichment (genres, biography, Wikipedia links)
//! via the free, no-auth MusicBrainz API (musicbrainz.org/ws/2).
//!
//! Rate-limit: MusicBrainz enforces ≤ 1 req/sec. All calls that make more
//! than one HTTP request insert a `tokio::time::sleep(1.1 s)` between them.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

const MB_API_BASE: &str = "https://musicbrainz.org/ws/2";
const MB_USER_AGENT: &str = "Audion/1.3.1 (https://audionplayer.com)";
const WIKI_SUMMARY_BASE: &str = "https://en.wikipedia.org/api/rest_v1/page/summary";

// ── Raw MusicBrainz JSON shapes ───────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct MbArtistSearchResponse {
    artists: Option<Vec<MbArtistResult>>,
}

#[derive(Debug, Deserialize)]
struct MbArtistResult {
    id: String,
    name: String,
    disambiguation: Option<String>,
    tags: Option<Vec<MbTag>>,
    genres: Option<Vec<MbTag>>,
}

#[derive(Debug, Deserialize)]
struct MbArtistDetail {
    tags: Option<Vec<MbTag>>,
    genres: Option<Vec<MbTag>>,
    relations: Option<Vec<MbRelation>>,
}

#[derive(Debug, Deserialize)]
struct MbTag {
    name: String,
    count: Option<i32>,
}

#[derive(Debug, Deserialize, Clone)]
struct MbRelation {
    #[serde(rename = "type")]
    rel_type: String,
    url: Option<MbUrl>,
}

#[derive(Debug, Deserialize, Clone)]
struct MbUrl {
    resource: String,
}

#[derive(Debug, Deserialize)]
struct WikiSummaryResponse {
    extract: Option<String>,
}

// ── Public types returned to the frontend ────────────────────────────────────

/// Rich artist metadata fetched from MusicBrainz (and optionally Wikipedia).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MbArtistInfo {
    /// MusicBrainz Artist ID (UUID string).
    pub mbid: Option<String>,
    pub name: String,
    /// Extra disambiguation text when multiple artists share a name.
    pub disambiguation: Option<String>,
    /// Up to 5 genre names, sorted by vote count.
    pub genres: Vec<String>,
    /// English Wikipedia page URL, if available.
    pub wikipedia_url: Option<String>,
    /// Plain-text extract from the Wikipedia article (first paragraph).
    pub bio: Option<String>,
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn mb_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(MB_USER_AGENT)
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))
}

/// Merge `genres` + `tags` arrays, deduplicate, sort by count, return top N.
fn collect_genres(
    tags: Option<&Vec<MbTag>>,
    genres: Option<&Vec<MbTag>>,
    take: usize,
) -> Vec<String> {
    let mut combined: Vec<(String, i32)> = Vec::new();

    let add = |combined: &mut Vec<(String, i32)>, tag: &MbTag| {
        let name_lower = tag.name.to_lowercase();
        // Skip decade tags ("1990s"), obviously non-genre tags
        if name_lower
            .chars()
            .next()
            .map_or(false, |c| c.is_ascii_digit())
        {
            return;
        }
        if matches!(
            name_lower.as_str(),
            "seen live" | "favorites" | "favourite" | "amazing"
        ) {
            return;
        }
        if !combined.iter().any(|(n, _)| n.to_lowercase() == name_lower) {
            combined.push((title_case(&tag.name), tag.count.unwrap_or(0)));
        }
    };

    if let Some(g) = genres {
        for t in g {
            add(&mut combined, t);
        }
    }
    if let Some(t) = tags {
        for t in t {
            add(&mut combined, t);
        }
    }

    combined.sort_by(|a, b| b.1.cmp(&a.1));
    combined.into_iter().map(|(n, _)| n).take(take).collect()
}

fn title_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Extract the first English Wikipedia URL from an MB `url-rels` array.
fn wikipedia_from_relations(relations: &[MbRelation]) -> Option<String> {
    relations
        .iter()
        .filter_map(|r| r.url.as_ref())
        .map(|u| u.resource.clone())
        .find(|u| u.contains("en.wikipedia.org"))
}

/// Fetch the Wikipedia plain-text summary for a Wikipedia URL.
async fn fetch_wiki_bio(wiki_url: &str) -> Option<String> {
    // URL shape: https://en.wikipedia.org/wiki/Radiohead
    let title = wiki_url.split("/wiki/").last()?;
    let client = mb_client().ok()?;
    let url = format!("{}/{}", WIKI_SUMMARY_BASE, title);

    let resp: WikiSummaryResponse = client.get(&url).send().await.ok()?.json().await.ok()?;

    resp.extract
}

// ── Tauri commands ────────────────────────────────────────────────────────────

/// Fetch MusicBrainz metadata for a single artist by name.
///
/// Makes 2 HTTP requests (search + detail) plus an optional Wikipedia fetch,
/// each separated by a 1.1 s sleep to respect the MB rate limit.
#[tauri::command]
pub async fn get_artist_musicbrainz_info(artist_name: String) -> Result<MbArtistInfo, String> {
    let client = mb_client()?;

    // ── 1. Search for the artist ─────────────────────────────────────────────
    let search_resp = client
        .get(format!("{}/artist", MB_API_BASE))
        .query(&[
            ("query", format!("artist:\"{}\"", artist_name)),
            ("limit", "1".into()),
            ("fmt", "json".into()),
        ])
        .send()
        .await
        .map_err(|e| format!("MusicBrainz search error: {}", e))?;

    if !search_resp.status().is_success() {
        return Err(format!(
            "MusicBrainz search returned {}",
            search_resp.status()
        ));
    }

    let search_data: MbArtistSearchResponse = search_resp
        .json()
        .await
        .map_err(|e| format!("MusicBrainz search parse error: {}", e))?;

    let artist = match search_data.artists.and_then(|v| v.into_iter().next()) {
        Some(a) => a,
        None => {
            return Ok(MbArtistInfo {
                mbid: None,
                name: artist_name,
                disambiguation: None,
                genres: vec![],
                wikipedia_url: None,
                bio: None,
            })
        }
    };

    let mbid = artist.id.clone();

    // Rate-limit gap before second request
    sleep(Duration::from_millis(1100)).await;

    // ── 2. Fetch full artist detail with genres + url-rels ───────────────────
    let detail_resp = client
        .get(format!("{}/artist/{}", MB_API_BASE, mbid))
        .query(&[("inc", "genres+tags+url-rels"), ("fmt", "json")])
        .send()
        .await
        .map_err(|e| format!("MusicBrainz detail error: {}", e))?;

    let (genres, wikipedia_url) = if detail_resp.status().is_success() {
        let detail: MbArtistDetail = detail_resp
            .json()
            .await
            .map_err(|e| format!("MusicBrainz detail parse error: {}", e))?;

        let genres = collect_genres(detail.tags.as_ref(), detail.genres.as_ref(), 5);
        let wiki_url = detail
            .relations
            .as_deref()
            .and_then(wikipedia_from_relations);

        (genres, wiki_url)
    } else {
        // Fall back to whatever tags came with the search result
        let genres = collect_genres(artist.tags.as_ref(), artist.genres.as_ref(), 5);
        (genres, None)
    };

    // ── 3. Fetch Wikipedia bio (best-effort) ─────────────────────────────────
    let bio = if let Some(ref url) = wikipedia_url {
        sleep(Duration::from_millis(300)).await;
        fetch_wiki_bio(url).await
    } else {
        None
    };

    Ok(MbArtistInfo {
        mbid: Some(mbid),
        name: artist.name,
        disambiguation: artist.disambiguation,
        genres,
        wikipedia_url,
        bio,
    })
}

/// Aggregate the most common genres across the user's top played artists by
/// querying MusicBrainz. Returns up to 5 `[genre, count]` pairs sorted by
/// how many of the top artists belong to that genre.
///
/// Uses 1 MB request per artist with 1.1 s gaps between them.
#[tauri::command]
pub async fn get_top_genres_from_mb(
    artist_limit: Option<usize>,
    db: tauri::State<'_, crate::db::Database>,
) -> Result<Vec<(String, u32)>, String> {
    let limit = artist_limit.unwrap_or(5).min(10); // cap to avoid very long waits

    // Fetch top artists from local play history — collect into owned Vec<String>
    // before any async work so the MutexGuard is released immediately.
    let artist_names: Vec<String> = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT t.artist, COUNT(*) as plays
                 FROM play_history ph
                 JOIN tracks t ON ph.track_id = t.id
                 WHERE t.artist IS NOT NULL
                 GROUP BY lower(t.artist)
                 ORDER BY plays DESC
                 LIMIT ?1",
            )
            .map_err(|e| e.to_string())?;

        // Avoid `?` on query_map to prevent the borrow from escaping the block.
        let rows = stmt
            .query_map(rusqlite::params![limit as i64], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|e| e.to_string())?;

        rows.filter_map(|r| r.ok()).collect()
    };

    if artist_names.is_empty() {
        return Ok(vec![]);
    }

    let client = mb_client()?;
    let mut genre_counts: HashMap<String, u32> = HashMap::new();

    for (i, name) in artist_names.iter().enumerate() {
        if i > 0 {
            sleep(Duration::from_millis(1100)).await;
        }

        let Ok(resp) = client
            .get(format!("{}/artist", MB_API_BASE))
            .query(&[
                ("query", format!("artist:\"{}\"", name)),
                ("limit", "1".into()),
                ("fmt", "json".into()),
            ])
            .send()
            .await
        else {
            continue;
        };

        let Ok(data) = resp.json::<MbArtistSearchResponse>().await else {
            continue;
        };

        let Some(artist) = data.artists.and_then(|v| v.into_iter().next()) else {
            continue;
        };

        // Use tags from the search result (detail call not needed for aggregation)
        let genres = collect_genres(artist.tags.as_ref(), artist.genres.as_ref(), 3);
        for genre in genres {
            *genre_counts.entry(genre).or_insert(0) += 1;
        }
    }

    let mut sorted: Vec<(String, u32)> = genre_counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    sorted.truncate(5);

    Ok(sorted)
}

// =============================================================================
// RECORDING LOOKUP · RELEASE INFO · SIMILAR ARTISTS · DISCOGRAPHY
// =============================================================================

// ── Additional raw MB JSON shapes ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct MbRecordingSearchResponse {
    recordings: Option<Vec<MbRecordingResult>>,
}

#[derive(Debug, Deserialize)]
struct MbRecordingResult {
    id: String,
    tags: Option<Vec<MbTag>>,
    genres: Option<Vec<MbTag>>,
    isrcs: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct MbReleaseSearchResponse {
    releases: Option<Vec<MbReleaseResult>>,
}

#[derive(Debug, Deserialize)]
struct MbReleaseResult {
    id: String,
    date: Option<String>,
    country: Option<String>,
    #[serde(rename = "label-info")]
    label_info: Option<Vec<MbLabelInfo>>,
    #[serde(rename = "release-group")]
    release_group: Option<MbReleaseGroupPartial>,
}

#[derive(Debug, Deserialize)]
struct MbLabelInfo {
    label: Option<MbLabelNameOnly>,
}

#[derive(Debug, Deserialize)]
struct MbLabelNameOnly {
    name: String,
}

#[derive(Debug, Deserialize)]
struct MbReleaseGroupPartial {
    id: Option<String>,
    #[serde(rename = "primary-type")]
    primary_type: Option<String>,
    #[serde(rename = "secondary-types")]
    secondary_types: Option<Vec<String>>,
    #[serde(rename = "first-release-date")]
    first_release_date: Option<String>,
}

/// Artist detail response when fetching `inc=artist-rels`.
#[derive(Debug, Deserialize)]
struct MbArtistDetailWithRels {
    relations: Option<Vec<MbArtistRel>>,
}

#[derive(Debug, Deserialize, Clone)]
struct MbArtistRel {
    #[serde(rename = "type")]
    rel_type: String,
    /// Populated only for artist-to-artist relations.
    artist: Option<MbRelatedArtist>,
}

#[derive(Debug, Deserialize, Clone)]
struct MbRelatedArtist {
    name: String,
}

#[derive(Debug, Deserialize)]
struct MbReleaseGroupBrowseResponse {
    #[serde(rename = "release-groups")]
    release_groups: Option<Vec<MbReleaseGroupItem>>,
}

#[derive(Debug, Deserialize)]
struct MbReleaseGroupItem {
    id: String,
    title: String,
    #[serde(rename = "primary-type")]
    primary_type: Option<String>,
    #[serde(rename = "secondary-types")]
    secondary_types: Option<Vec<String>>,
    #[serde(rename = "first-release-date")]
    first_release_date: Option<String>,
}

// ── Additional public return types ────────────────────────────────────────────

/// Result of enriching a local track with MusicBrainz recording data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MbTrackEnrichment {
    /// MusicBrainz Recording ID written back to the local database.
    pub mbid: Option<String>,
    /// Top genre tag for the recording.
    pub genre: Option<String>,
    /// ISRC codes (International Standard Recording Codes) for this recording.
    pub isrcs: Vec<String>,
}

/// Release metadata from MusicBrainz (label, year, country, release type).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MbReleaseInfo {
    pub mbid: Option<String>,
    pub year: Option<String>,
    pub original_year: Option<String>,
    pub country: Option<String>,
    pub label: Option<String>,
    /// e.g. "Album", "EP", "Single", "Live", "Compilation"
    pub release_type: Option<String>,
}

/// An artist related to the queried artist on MusicBrainz.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MbSimilarArtist {
    pub name: String,
    /// MB relation type e.g. "member of band", "collaboration", "supporting musician".
    pub relation_type: String,
    /// `true` when this artist has at least one track in the local library.
    pub in_library: bool,
}

/// A release group (album/EP/single/etc.) from an artist's MusicBrainz discography.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MbDiscographyItem {
    /// MusicBrainz Release Group ID (UUID).
    pub mbid: String,
    pub title: String,
    pub year: Option<String>,
    /// Primary or most specific release type e.g. "Album", "EP", "Single", "Live".
    pub release_type: String,
    /// Cover Art Archive URL for the front cover (250px thumbnail).
    /// The URL may return a 404 if no cover art has been submitted.
    pub cover_url: String,
}

// ── Shared helpers ────────────────────────────────────────────────────────────

/// Build a human-readable release type from MB primary + secondary type fields.
fn release_type_label(primary: &Option<String>, secondary: &Option<Vec<String>>) -> String {
    if let Some(ref sec) = secondary {
        if !sec.is_empty() {
            return sec[0].clone();
        }
    }
    primary.clone().unwrap_or_else(|| "Release".into())
}

/// Extract the 4-digit year from a date string like "2001-06-04", "2001", or "2001-06".
fn year_from_date(date: &str) -> String {
    date.split('-').next().unwrap_or(date).to_string()
}

/// Strip common reissue/edition suffixes from album names for better MB matching.
/// e.g. "...And Justice for All (Remastered)" → "...And Justice for All"
fn clean_album_name(name: &str) -> String {
    let patterns = [
        "(remastered)",
        "(remaster)",
        "(deluxe edition)",
        "(deluxe)",
        "(expanded edition)",
        "(special edition)",
        "(bonus track version)",
        "(bonus tracks)",
        "(anniversary edition)",
        "(super deluxe)",
        "(super deluxe edition)",
        "[remastered]",
        "[remaster]",
        "[deluxe edition]",
        "[deluxe]",
        "[expanded edition]",
        "[special edition]",
    ];
    let lower = name.to_lowercase();
    let mut result = name.to_string();
    for pat in &patterns {
        if let Some(pos) = lower.find(pat) {
            result = result[..pos].trim().to_string();
            break;
        }
    }
    result
}

/// Response for a release-group lookup (to get first-release-date).
#[derive(Debug, Deserialize)]
struct MbReleaseGroupLookup {
    #[serde(rename = "first-release-date")]
    first_release_date: Option<String>,
}

// ── New Tauri commands ────────────────────────────────────────────────────────

/// Search MusicBrainz for a recording by artist + title.
///
/// Writes the found `musicbrainz_recording_id` and top genre back to the
/// local database (best-effort, silent on DB failure) and returns any ISRC codes.
///
/// Uses 1 HTTP request: `/recording?query=...&inc=isrcs+genres+tags`.
#[tauri::command]
pub async fn enrich_track_metadata_mb(
    track_id: i64,
    artist: String,
    title: String,
    db: tauri::State<'_, crate::db::Database>,
) -> Result<MbTrackEnrichment, String> {
    let client = mb_client()?;

    let resp = client
        .get(format!("{}/recording", MB_API_BASE))
        .query(&[
            (
                "query",
                format!("artist:\"{}\" AND recording:\"{}\"", artist, title),
            ),
            ("limit", "1".into()),
            ("inc", "isrcs+genres+tags".into()),
            ("fmt", "json".into()),
        ])
        .send()
        .await
        .map_err(|e| format!("MusicBrainz recording search error: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("MusicBrainz returned {}", resp.status()));
    }

    let data: MbRecordingSearchResponse = resp
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    let recording = match data.recordings.and_then(|v| v.into_iter().next()) {
        Some(r) => r,
        None => {
            return Ok(MbTrackEnrichment {
                mbid: None,
                genre: None,
                isrcs: vec![],
            })
        }
    };

    let mbid = recording.id.clone();
    let genres = collect_genres(recording.tags.as_ref(), recording.genres.as_ref(), 1);
    let top_genre = genres.into_iter().next();
    let isrcs = recording.isrcs.clone().unwrap_or_default();

    // Write back to DB (best-effort — don't propagate DB failures)
    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        crate::db::queries::update_track_mb_data(
            &conn,
            track_id,
            Some(mbid.as_str()),
            top_genre.as_deref(),
        )
        .ok();
    }

    Ok(MbTrackEnrichment {
        mbid: Some(mbid),
        genre: top_genre,
        isrcs,
    })
}

/// Search MusicBrainz for a release matching the given album + artist name.
/// Returns label, release year, country, and release type (Album/EP/Single/etc.).
///
/// Uses 1-2 HTTP requests:
/// 1. `/release?query=...&inc=labels+release-groups` to find the release.
/// 2. `/release-group/{id}` to get `first-release-date` (the search API doesn't include it).
///
/// Album names are cleaned of common suffixes like "(Remastered)" before searching.
#[tauri::command]
pub async fn get_release_mb_info(
    album_name: String,
    artist_name: String,
) -> Result<MbReleaseInfo, String> {
    let client = mb_client()?;

    let cleaned_name = clean_album_name(&album_name);

    let resp = client
        .get(format!("{}/release", MB_API_BASE))
        .query(&[
            (
                "query",
                format!("release:\"{}\" AND artist:\"{}\"", cleaned_name, artist_name),
            ),
            ("limit", "1".into()),
            ("inc", "labels+release-groups".into()),
            ("fmt", "json".into()),
        ])
        .send()
        .await
        .map_err(|e| format!("MusicBrainz release search error: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("MusicBrainz returned {}", resp.status()));
    }

    let data: MbReleaseSearchResponse = resp
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    let release = match data.releases.and_then(|v| v.into_iter().next()) {
        Some(r) => r,
        None => {
            return Ok(MbReleaseInfo {
                mbid: None,
                year: None,
                original_year: None,
                country: None,
                label: None,
                release_type: None,
            })
        }
    };

    let year = release.date.as_deref().map(year_from_date);

    // The search API returns a partial release-group without first-release-date.
    // Try to get it from the search response first; if missing, fetch the release-group directly.
    let mut original_year = release
        .release_group
        .as_ref()
        .and_then(|rg| rg.first_release_date.as_deref())
        .filter(|d| !d.is_empty())
        .map(year_from_date);

    if original_year.is_none() {
        if let Some(rg_id) = release.release_group.as_ref().and_then(|rg| rg.id.as_deref()) {
            // Small delay to respect MB rate limit (1 req/sec)
            sleep(Duration::from_millis(1100)).await;

            if let Ok(rg_resp) = client
                .get(format!("{}/release-group/{}", MB_API_BASE, rg_id))
                .query(&[("fmt", "json")])
                .send()
                .await
            {
                if rg_resp.status().is_success() {
                    if let Ok(rg_data) = rg_resp.json::<MbReleaseGroupLookup>().await {
                        original_year = rg_data
                            .first_release_date
                            .as_deref()
                            .filter(|d| !d.is_empty())
                            .map(year_from_date);
                    }
                }
            }
        }
    }

    let label = release
        .label_info
        .as_deref()
        .and_then(|infos| infos.first())
        .and_then(|li| li.label.as_ref())
        .map(|l| l.name.clone());
    let release_type = release
        .release_group
        .as_ref()
        .map(|rg| release_type_label(&rg.primary_type, &rg.secondary_types));

    Ok(MbReleaseInfo {
        mbid: Some(release.id),
        year,
        original_year,
        country: release.country,
        label,
        release_type,
    })
}

/// Find artists related to the given artist on MusicBrainz (band members,
/// collaborators, etc.) and mark which ones exist in the local library.
///
/// Makes 2 rate-limited HTTP requests:
/// 1. Artist search to resolve the MBID.
/// 2. `/artist/<MBID>?inc=artist-rels` to get related artists.
#[tauri::command]
pub async fn get_similar_artists_mb(
    artist_name: String,
    db: tauri::State<'_, crate::db::Database>,
) -> Result<Vec<MbSimilarArtist>, String> {
    let client = mb_client()?;

    // ── 1. Resolve artist MBID ────────────────────────────────────────────────
    let search_resp = client
        .get(format!("{}/artist", MB_API_BASE))
        .query(&[
            ("query", format!("artist:\"{}\"", artist_name)),
            ("limit", "1".into()),
            ("fmt", "json".into()),
        ])
        .send()
        .await
        .map_err(|e| format!("MB artist search error: {}", e))?;

    let search_data: MbArtistSearchResponse = search_resp
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    let mbid = match search_data.artists.and_then(|v| v.into_iter().next()) {
        Some(a) => a.id,
        None => return Ok(vec![]),
    };

    sleep(Duration::from_millis(1100)).await;

    // ── 2. Fetch artist-to-artist relations ───────────────────────────────────
    let detail_resp = client
        .get(format!("{}/artist/{}", MB_API_BASE, mbid))
        .query(&[("inc", "artist-rels"), ("fmt", "json")])
        .send()
        .await
        .map_err(|e| format!("MB artist detail error: {}", e))?;

    if !detail_resp.status().is_success() {
        return Ok(vec![]);
    }

    let detail: MbArtistDetailWithRels = detail_resp
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    let artist_rels: Vec<(String, String)> = detail
        .relations
        .unwrap_or_default()
        .into_iter()
        .filter_map(|r| r.artist.map(|a| (a.name, r.rel_type)))
        .take(30)
        .collect();

    if artist_rels.is_empty() {
        return Ok(vec![]);
    }

    // ── 3. Cross-reference with local library ─────────────────────────────────
    let local_lower: Vec<String> = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT DISTINCT lower(artist) FROM tracks WHERE artist IS NOT NULL")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.filter_map(|r| r.ok()).collect()
    };

    let results = artist_rels
        .into_iter()
        .map(|(name, relation_type)| {
            let in_library = local_lower.iter().any(|la| *la == name.to_lowercase());
            MbSimilarArtist {
                name,
                relation_type,
                in_library,
            }
        })
        .collect();

    Ok(results)
}

/// Fetch the full release-group discography for an artist from MusicBrainz,
/// sorted newest-first.
///
/// Makes 2 rate-limited HTTP requests:
/// 1. Artist search to resolve the MBID.
/// 2. `/release-group?artist=<MBID>&limit=100` to get all release groups.
#[tauri::command]
pub async fn get_artist_discography_mb(
    artist_name: String,
) -> Result<Vec<MbDiscographyItem>, String> {
    let client = mb_client()?;

    // ── 1. Resolve MBID ───────────────────────────────────────────────────────
    let search_resp = client
        .get(format!("{}/artist", MB_API_BASE))
        .query(&[
            ("query", format!("artist:\"{}\"", artist_name)),
            ("limit", "1".into()),
            ("fmt", "json".into()),
        ])
        .send()
        .await
        .map_err(|e| format!("MB search error: {}", e))?;

    let search_data: MbArtistSearchResponse = search_resp
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    let mbid = match search_data.artists.and_then(|v| v.into_iter().next()) {
        Some(a) => a.id,
        None => return Ok(vec![]),
    };

    sleep(Duration::from_millis(1100)).await;

    // ── 2. Browse release-groups by MBID ──────────────────────────────────────
    let browse_resp = client
        .get(format!("{}/release-group", MB_API_BASE))
        .query(&[("artist", mbid.as_str()), ("limit", "100"), ("fmt", "json")])
        .send()
        .await
        .map_err(|e| format!("MB browse error: {}", e))?;

    if !browse_resp.status().is_success() {
        return Ok(vec![]);
    }

    let data: MbReleaseGroupBrowseResponse = browse_resp
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    let mut items: Vec<MbDiscographyItem> = data
        .release_groups
        .unwrap_or_default()
        .into_iter()
        .map(|rg| {
            let cover_url = format!(
                "https://coverartarchive.org/release-group/{}/front-250",
                rg.id
            );
            MbDiscographyItem {
                mbid: rg.id,
                title: rg.title,
                year: rg.first_release_date.as_deref().map(year_from_date),
                release_type: release_type_label(&rg.primary_type, &rg.secondary_types),
                cover_url,
            }
        })
        .collect();

    // Sort newest-first; items without a year sink to the bottom
    items.sort_by(|a, b| match (&b.year, &a.year) {
        (Some(y1), Some(y2)) => y1.cmp(y2),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });

    Ok(items)
}

// ── Album year enrichment ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlbumYearEnrichResult {
    pub year: Option<i32>,
    pub original_year: Option<i32>,
}

/// Parse a year string (e.g. "2001") into an i32.
fn parse_year_str(s: &str) -> Option<i32> {
    let y: i32 = s.parse().ok()?;
    if (1900..=2100).contains(&y) {
        Some(y)
    } else {
        None
    }
}

/// Enrich a single album's year and original_year by querying MusicBrainz.
/// Writes the results to the local database.
#[tauri::command]
pub async fn enrich_album_year(
    album_id: i64,
    album_name: String,
    artist_name: String,
    db: tauri::State<'_, crate::db::Database>,
) -> Result<AlbumYearEnrichResult, String> {
    let mb_info = get_release_mb_info(album_name, artist_name).await?;

    let year = mb_info.year.as_deref().and_then(parse_year_str);
    let original_year = mb_info.original_year.as_deref().and_then(parse_year_str);

    if year.is_some() || original_year.is_some() {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        crate::db::queries::update_album_years(&conn, album_id, year, original_year)
            .map_err(|e| e.to_string())?;
    }

    Ok(AlbumYearEnrichResult {
        year,
        original_year,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchEnrichResult {
    pub enriched: u32,
    pub failed: u32,
    pub total: u32,
}

/// Enrich all albums that are missing `original_year` by querying MusicBrainz.
/// Emits `album-enrich-progress` events with `{ done, total }` payloads.
#[tauri::command]
pub async fn enrich_all_album_years(
    app: tauri::AppHandle,
    db: tauri::State<'_, crate::db::Database>,
) -> Result<BatchEnrichResult, String> {
    use tauri::Emitter;

    let albums = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        crate::db::queries::get_albums_missing_original_year(&conn).map_err(|e| e.to_string())?
    };

    let total = albums.len() as u32;
    let mut enriched: u32 = 0;
    let mut failed: u32 = 0;

    for (i, album) in albums.iter().enumerate() {
        let artist = album.artist.as_deref().unwrap_or("");
        match get_release_mb_info(album.name.clone(), artist.to_string()).await {
            Ok(mb_info) => {
                let year = mb_info.year.as_deref().and_then(parse_year_str);
                let original_year = mb_info.original_year.as_deref().and_then(parse_year_str);

                if year.is_some() || original_year.is_some() {
                    let conn = db.conn.lock().map_err(|e| e.to_string())?;
                    if crate::db::queries::update_album_years(
                        &conn, album.id, year, original_year,
                    )
                    .is_ok()
                    {
                        enriched += 1;
                    } else {
                        failed += 1;
                    }
                } else {
                    failed += 1;
                }
            }
            Err(e) => {
                tracing::warn!(album = %album.name, error = %e, "MB enrichment failed");
                failed += 1;
            }
        }

        let _ = app.emit(
            "album-enrich-progress",
            serde_json::json!({ "done": i + 1, "total": total }),
        );

        // Rate limit: 1.1s between requests (MusicBrainz policy)
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    }

    Ok(BatchEnrichResult {
        enriched,
        failed,
        total,
    })
}
// ── Discovery search types & commands ────────────────────────────────────────

/// Raw MB artist search result with optional area for discovery.
#[derive(Debug, Deserialize)]
struct MbArtistSearchResultRaw {
    id: String,
    name: String,
    disambiguation: Option<String>,
    #[serde(rename = "type")]
    artist_type: Option<String>,
    country: Option<String>,
    tags: Option<Vec<MbTag>>,
    genres: Option<Vec<MbTag>>,
    #[serde(rename = "life-span")]
    life_span: Option<MbLifeSpan>,
}

#[derive(Debug, Deserialize)]
struct MbLifeSpan {
    begin: Option<String>,
    end: Option<String>,
    ended: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct MbArtistSearchMultiResponse {
    artists: Option<Vec<MbArtistSearchResultRaw>>,
}

/// Raw MB release-group search result for discovery.
#[derive(Debug, Deserialize)]
struct MbReleaseGroupSearchRaw {
    id: String,
    title: String,
    #[serde(rename = "primary-type")]
    primary_type: Option<String>,
    #[serde(rename = "secondary-types")]
    secondary_types: Option<Vec<String>>,
    #[serde(rename = "first-release-date")]
    first_release_date: Option<String>,
    #[serde(rename = "artist-credit")]
    artist_credit: Option<Vec<MbArtistCreditLight>>,
    tags: Option<Vec<MbTag>>,
    releases: Option<Vec<MbReleaseInGroup>>,
}

#[derive(Debug, Deserialize)]
struct MbArtistCreditLight {
    name: Option<String>,
    artist: Option<MbArtistCreditArtist>,
}

#[derive(Debug, Deserialize)]
struct MbArtistCreditArtist {
    id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct MbReleaseInGroup {
    id: String,
    country: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MbReleaseGroupSearchMultiResponse {
    #[serde(rename = "release-groups")]
    release_groups: Option<Vec<MbReleaseGroupSearchRaw>>,
}

// ── Public discovery result types ─────────────────────────────────────────────

/// A single artist result from a MusicBrainz discovery search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MbDiscoverArtist {
    pub mbid: String,
    pub name: String,
    pub disambiguation: Option<String>,
    pub artist_type: Option<String>,
    pub country: Option<String>,
    pub genres: Vec<String>,
    pub active_years: Option<String>,
}

/// A single release-group result from a MusicBrainz discovery search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MbDiscoverRelease {
    pub mbid: String,
    pub title: String,
    pub artist_name: String,
    pub artist_mbid: Option<String>,
    pub release_type: String,
    pub year: Option<String>,
    pub country: Option<String>,
    pub genres: Vec<String>,
}

/// Search MusicBrainz for artists matching `query`. Returns up to `limit`
/// results (max 25). Single HTTP request.
#[tauri::command]
pub async fn search_artists_mb(
    query: String,
    limit: Option<u32>,
) -> Result<Vec<MbDiscoverArtist>, String> {
    let client = mb_client()?;
    let lim = limit.unwrap_or(15).min(25);

    let resp = client
        .get(format!("{}/artist", MB_API_BASE))
        .query(&[
            ("query", query.as_str()),
            ("limit", &lim.to_string()),
            ("fmt", "json"),
        ])
        .send()
        .await
        .map_err(|e| format!("MB search error: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("MB returned {}", resp.status()));
    }

    let data: MbArtistSearchMultiResponse = resp
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    let results = data
        .artists
        .unwrap_or_default()
        .into_iter()
        .map(|a| {
            let genres = collect_genres(a.tags.as_ref(), a.genres.as_ref(), 5);
            let active_years = a
                .life_span
                .map(|ls| {
                    let begin = ls.begin.as_deref().map(year_from_date).unwrap_or_default();
                    let end = if ls.ended.unwrap_or(false) {
                        ls.end
                            .as_deref()
                            .map(year_from_date)
                            .unwrap_or_else(|| "?".into())
                    } else {
                        "present".into()
                    };
                    if begin.is_empty() {
                        return String::new();
                    }
                    format!("{} – {}", begin, end)
                })
                .filter(|s| !s.is_empty());

            MbDiscoverArtist {
                mbid: a.id,
                name: a.name,
                disambiguation: a.disambiguation.filter(|d| !d.is_empty()),
                artist_type: a.artist_type,
                country: a.country.filter(|c| !c.is_empty()),
                genres,
                active_years,
            }
        })
        .collect();

    Ok(results)
}

/// Search MusicBrainz for release groups (albums/EPs/singles) matching `query`.
/// Returns up to `limit` results (max 25). Single HTTP request.
#[tauri::command]
pub async fn search_releases_mb(
    query: String,
    limit: Option<u32>,
) -> Result<Vec<MbDiscoverRelease>, String> {
    let client = mb_client()?;
    let lim = limit.unwrap_or(15).min(25);

    let resp = client
        .get(format!("{}/release-group", MB_API_BASE))
        .query(&[
            ("query", query.as_str()),
            ("limit", &lim.to_string()),
            ("fmt", "json"),
        ])
        .send()
        .await
        .map_err(|e| format!("MB search error: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("MB returned {}", resp.status()));
    }

    let data: MbReleaseGroupSearchMultiResponse = resp
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    let results = data
        .release_groups
        .unwrap_or_default()
        .into_iter()
        .map(|rg| {
            let (artist_name, artist_mbid) = rg
                .artist_credit
                .and_then(|ac| ac.into_iter().next())
                .map(|ac| {
                    let name = ac.name.unwrap_or_else(|| {
                        ac.artist
                            .as_ref()
                            .map(|a| a.name.clone())
                            .unwrap_or_default()
                    });
                    let mbid = ac.artist.map(|a| a.id);
                    (name, mbid)
                })
                .unwrap_or_else(|| ("Unknown Artist".into(), None));

            let country = rg.releases.and_then(|rels| {
                rels.into_iter()
                    .find_map(|r| r.country.filter(|c| !c.is_empty()))
            });

            let genres = collect_genres(rg.tags.as_ref(), None, 3);

            MbDiscoverRelease {
                mbid: rg.id,
                title: rg.title,
                artist_name,
                artist_mbid,
                release_type: release_type_label(&rg.primary_type, &rg.secondary_types),
                year: rg.first_release_date.as_deref().map(year_from_date),
                country,
                genres,
            }
        })
        .collect();

    Ok(results)
}

// ── Track fetching commands ──────────────────────────────────────────────────

/// A single track from a MusicBrainz release.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MbTrack {
    pub mbid: String,
    pub title: String,
    pub artist: String,
    pub duration_ms: Option<u32>,
    pub track_number: u32,
    pub disc_number: u32,
}

#[derive(Debug, Deserialize)]
struct MbArtistRecordingsResponse {
    recordings: Option<Vec<MbArtistRecording>>,
}

#[derive(Debug, Deserialize)]
struct MbArtistRecording {
    id: String,
    title: String,
    length: Option<u32>,
    #[serde(rename = "artist-credit")]
    artist_credit: Option<Vec<MbArtistCreditLight>>,
}

#[derive(Debug, Deserialize)]
struct MbReleaseGroupDetail {
    releases: Option<Vec<MbReleaseInGroupDetail>>,
}

#[derive(Debug, Deserialize)]
struct MbReleaseInGroupDetail {
    id: String,
    status: Option<String>,
    country: Option<String>,
    #[serde(rename = "track-count")]
    track_count: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct MbReleaseMediaDetail {
    media: Option<Vec<MbMedia>>,
}

#[derive(Debug, Deserialize)]
struct MbMedia {
    position: u32,
    tracks: Vec<MbTrackRaw>,
}

#[derive(Debug, Deserialize)]
struct MbTrackRaw {
    title: String,
    length: Option<u32>,
    position: u32,
    recording: MbRecordingPartial,
}

#[derive(Debug, Deserialize)]
struct MbRecordingPartial {
    id: String,
    #[serde(rename = "artist-credit")]
    artist_credit: Option<Vec<MbArtistCreditLight>>,
}

/// Fetch all tracks for a given release-group MBID.
///
/// Makes up to 2 HTTP requests (browse releases + release detail).
#[tauri::command]
pub async fn get_release_group_tracks_mb(rg_mbid: String) -> Result<Vec<MbTrack>, String> {
    let client = mb_client()?;

    // 1. Get releases for this release group to find the "best" one
    let rg_resp = client
        .get(format!("{}/release-group/{}", MB_API_BASE, rg_mbid))
        .query(&[("inc", "releases"), ("fmt", "json")])
        .send()
        .await
        .map_err(|e| format!("MB release-group browse error: {}", e))?;

    if !rg_resp.status().is_success() {
        return Err(format!("MB returned {}", rg_resp.status()));
    }

    let rg_data: MbReleaseGroupDetail = rg_resp
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    let releases = rg_data.releases.unwrap_or_default();
    if releases.is_empty() {
        return Ok(vec![]);
    }

    // Select the "best" release: Priority for Official status, then specific countries
    let best_release = releases
        .iter()
        .find(|r| {
            r.status.as_deref() == Some("Official")
                && (r.country.as_deref() == Some("US") || r.country.as_deref() == Some("GB"))
        })
        .or_else(|| {
            releases
                .iter()
                .find(|r| r.status.as_deref() == Some("Official"))
        })
        .unwrap_or_else(|| &releases[0]);

    let release_mbid = &best_release.id;

    // Rate-limit gap
    sleep(Duration::from_millis(1100)).await;

    // 2. Fetch recordings for the selected release
    let rel_resp = client
        .get(format!("{}/release/{}", MB_API_BASE, release_mbid))
        .query(&[("inc", "recordings+artist-credits"), ("fmt", "json")])
        .send()
        .await
        .map_err(|e| format!("MB release detail error: {}", e))?;

    if !rel_resp.status().is_success() {
        return Err(format!("MB returned {}", rel_resp.status()));
    }

    let rel_data: MbReleaseMediaDetail = rel_resp
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    let mut tracks = Vec::new();

    for media in rel_data.media.unwrap_or_default() {
        for track in media.tracks {
            let artist_name = track
                .recording
                .artist_credit
                .and_then(|ac| ac.into_iter().next())
                .map(|ac| {
                    ac.name.unwrap_or_else(|| {
                        ac.artist
                            .as_ref()
                            .map(|a| a.name.clone())
                            .unwrap_or_default()
                    })
                })
                .unwrap_or_else(|| "Unknown Artist".into());

            tracks.push(MbTrack {
                mbid: track.recording.id,
                title: track.title,
                artist: artist_name,
                duration_ms: track.length,
                track_number: track.position,
                disc_number: media.position,
            });
        }
    }

    Ok(tracks)
}

/// Fetch a list of recordings (featured tracks) for an artist by MBID.
#[tauri::command]
pub async fn get_artist_top_tracks_mb(artist_mbid: String) -> Result<Vec<MbTrack>, String> {
    let client = mb_client()?;

    let resp = client
        .get(format!("{}/recording", MB_API_BASE))
        .query(&[
            ("artist", artist_mbid.as_str()),
            ("limit", "20"),
            ("inc", "artist-credits"),
            ("fmt", "json"),
        ])
        .send()
        .await
        .map_err(|e| format!("MB recordings error: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("MB returned {}", resp.status()));
    }

    let data: MbArtistRecordingsResponse = resp
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    let tracks = data
        .recordings
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(i, r)| {
            let artist_name = r
                .artist_credit
                .and_then(|ac| ac.into_iter().next())
                .map(|ac| {
                    ac.name.unwrap_or_else(|| {
                        ac.artist
                            .as_ref()
                            .map(|a| a.name.clone())
                            .unwrap_or_default()
                    })
                })
                .unwrap_or_else(|| "Unknown Artist".into());

            MbTrack {
                mbid: r.id,
                title: r.title,
                artist: artist_name,
                duration_ms: r.length,
                track_number: (i + 1) as u32,
                disc_number: 1,
            }
        })
        .collect();

    Ok(tracks)
}

// =============================================================================
// ALBUM INFO MODAL � rich release detail with tracklist + Cover Art Archive
// =============================================================================
//
// `MbReleaseInfo` above is intentionally small � just enough to enrich the
// album year in the database. The "Info" modal opened from AlbumDetail
// needs a much richer payload (tracklist with MBIDs, barcode, packaging,
// format, CAA cover URLs, optional Wikipedia summary). That's what these
// types and commands provide.
//
// We also cache the full release detail in memory so opening the modal a
// second time for the same album is instant and doesn't hit the MB
// 1-req/sec rate limit.
// =============================================================================

const CAA_BASE: &str = "https://coverartarchive.org";
const RELEASE_DETAIL_TTL_SECS: u64 = 30 * 24 * 60 * 60; // 30 days

/// Rich release metadata for the Album Info modal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MbReleaseDetail {
    /// MusicBrainz Release ID (the specific pressing, not the release-group).
    pub mbid: String,
    /// MusicBrainz Release Group ID (the abstract "album").
    pub release_group_mbid: String,
    pub title: String,
    pub artist: String,
    pub artist_mbid: Option<String>,
    pub year: Option<String>,
    /// Year of the *first* release of this release-group (often differs
    /// from `year` for reissues / remasters).
    pub original_year: Option<String>,
    pub country: Option<String>,
    pub label: Option<String>,
    pub catalog_number: Option<String>,
    pub barcode: Option<String>,
    pub packaging: Option<String>,
    pub format: Option<String>,
    pub language: Option<String>,
    pub script: Option<String>,
    pub release_type: Option<String>,
    pub track_count: u32,
    pub total_duration_ms: Option<u64>,
    /// Cover Art Archive front cover URLs (None if no cover is uploaded).
    pub cover_url_250: Option<String>,
    pub cover_url_500: Option<String>,
    pub cover_url_1200: Option<String>,
    pub wikipedia_url: Option<String>,
    pub wiki_extract: Option<String>,
    pub tracks: Vec<MbReleaseTrack>,
}

/// One track in a release's tracklist.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MbReleaseTrack {
    /// MusicBrainz Recording ID � links to the canonical recording page.
    pub mbid: String,
    pub position: u32,
    pub disc_number: u32,
    pub title: String,
    pub length_ms: Option<u64>,
    pub artist_credit: Option<String>,
}

// -- Cache -------------------------------------------------------------------

#[derive(Clone)]
struct CachedReleaseDetail {
    detail: MbReleaseDetail,
    fetched_at: std::time::Instant,
}

fn release_detail_cache() -> &'static Mutex<HashMap<String, CachedReleaseDetail>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CachedReleaseDetail>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_key(album: &str, artist: &str) -> String {
    format!(
        "{}|{}",
        clean_album_name(album).to_lowercase(),
        artist.to_lowercase()
    )
}

// -- MB JSON shapes used only by the release-detail flow ---------------------

#[derive(Debug, Deserialize)]
struct MbReleaseSearchResponse2 {
    releases: Option<Vec<MbReleaseSearchEntry>>,
}

#[derive(Debug, Deserialize)]
struct MbReleaseSearchEntry {
    id: String,
    title: String,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    barcode: Option<String>,
    #[serde(default)]
    packaging: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    script: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    media: Option<Vec<MbReleaseMedia>>,
    #[serde(default)]
    label_info: Option<Vec<MbReleaseLabelInfo>>,
    #[serde(default)]
    artist_credit: Option<Vec<MbArtistCredit>>,
    #[serde(default)]
    relations: Option<Vec<MbRelation>>,
    #[serde(default, rename = "release-group")]
    release_group: Option<MbReleaseGroupSummary>,
}

#[derive(Debug, Deserialize)]
struct MbReleaseMedia {
    format: Option<String>,
    #[serde(default)]
    tracks: Option<Vec<MbMediaTrack>>,
}

#[derive(Debug, Deserialize)]
struct MbMediaTrack {
    id: String,
    position: Option<u32>,
    number: Option<String>,
    title: String,
    length: Option<u64>,
    #[serde(default)]
    artist_credit: Option<Vec<MbArtistCredit>>,
}

#[derive(Debug, Deserialize)]
struct MbReleaseLabelInfo {
    label: Option<MbLabel>,
    #[serde(default)]
    catalog_number: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MbLabel {
    name: String,
}

#[derive(Debug, Deserialize)]
struct MbArtistCredit {
    name: String,
    artist: Option<MbArtistBrief>,
    joinphrase: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MbArtistBrief {
    id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct MbReleaseGroupSummary {
    id: String,
    #[serde(default)]
    primary_type: Option<String>,
    #[serde(default)]
    secondary_types: Option<Vec<String>>,
    #[serde(default)]
    first_release_date: Option<String>,
    /// When the search endpoint returns the release-group inline.
    #[serde(default)]
    relations: Option<Vec<MbRelation>>,
}

/// Release-group lookup (with ?inc=releases+url-rels). Used to enumerate
/// the releases of a release-group so we can pick the canonical one.
#[derive(Debug, Deserialize)]
struct MbReleaseGroupLookup2 {
    id: String,
    #[serde(default)]
    primary_type: Option<String>,
    #[serde(default)]
    secondary_types: Option<Vec<String>>,
    #[serde(default)]
    first_release_date: Option<String>,
    #[serde(default)]
    relations: Option<Vec<MbRelation>>,
    #[serde(default)]
    releases: Option<Vec<MbReleaseGroupRelease>>,
}

#[derive(Debug, Deserialize)]
struct MbReleaseGroupRelease {
    id: String,
    title: String,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    packaging: Option<String>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    media: Option<Vec<MbReleaseMedia>>,
    #[serde(default)]
    label_info: Option<Vec<MbReleaseLabelInfo>>,
}

// -- Helpers -----------------------------------------------------------------

/// Pick the canonical release of a release-group.
///
/// Preference order:
///   1. status == "Official"  (skip Promotion / Bootleg / Pseudo-Release)
///   2. earliest release date
///   3. format matches the release-group's primary type (CD for Album, etc.)
fn pick_canonical_release(releases: &[MbReleaseGroupRelease]) -> Option<&MbReleaseGroupRelease> {
    // First pass: Official + earliest date.
    let mut best_official: Option<&MbReleaseGroupRelease> = None;
    for r in releases {
        if r.status.as_deref() != Some("Official") {
            continue;
        }
        match best_official {
            None => best_official = Some(r),
            Some(cur) => {
                let cur_date = cur.date.as_deref().unwrap_or("9999");
                let new_date = r.date.as_deref().unwrap_or("9999");
                if new_date < cur_date {
                    best_official = Some(r);
                }
            }
        }
    }
    if let Some(r) = best_official {
        return Some(r);
    }
    // Fallback: anything (e.g. only Bootlegs exist).
    releases.first()
}

/// Convert MediaTrack.position/number to a numeric position.
/// MediaTrack format from MB is usually { position: 1, number: "1" } or just { number: "1.2" } for discs.
fn parse_track_position(number: Option<&str>, position: Option<u32>) -> (u32, u32) {
    // The "position" field is 1-based index in the release.
    if let Some(pos) = position {
        // Disc numbers look like "1.05" � extract disc from number if present.
        if let Some(n) = number {
            if let Some((disc, _)) = n.split_once('.') {
                if let Ok(d) = disc.parse::<u32>() {
                    return (d, pos);
                }
            }
        }
        return (1, pos);
    }
    if let Some(n) = number {
        if let Some((disc_str, track_str)) = n.split_once('.') {
            let disc = disc_str.parse::<u32>().unwrap_or(1);
            let track = track_str.parse::<u32>().unwrap_or(1);
            return (disc, track);
        }
        if let Ok(t) = n.parse::<u32>() {
            return (1, t);
        }
    }
    (1, 1)
}

/// Fetch the Wikipedia summary for a Wikipedia URL. Used by both the
/// direct-MB-URL path and the Wikidata-resolved path.
async fn fetch_wiki_summary_for_url_simple(wiki_url: &str) -> Option<(String, String)> {
    // URL shape: https://en.wikipedia.org/wiki/Radiohead
    // or https://es.wikipedia.org/wiki/Radiohead
    let (lang, title) = if let Some(rest) = wiki_url.strip_prefix("https://") {
        let mut parts = rest.splitn(3, '/');
        let host = parts.next()?;
        let lang = host.split('.').next()?;
        let title = parts.next()?.strip_prefix("wiki/")?;
        (lang.to_string(), title.to_string())
    } else {
        return None;
    };
    let client = mb_client().ok()?;
    let summary_url = format!("https://{lang}.wikipedia.org/api/rest_v1/page/summary/{title}");
    let resp = client.get(&summary_url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let data: WikiSummaryResponse = resp.json().await.ok()?;
    Some((wiki_url.to_string(), data.extract?))
}

/// Build a Wikipedia URL from release-group relations, preferring English.
fn wikipedia_from_release_group(rg: &MbReleaseGroupLookup2) -> Option<String> {
    rg.relations
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .filter_map(|r| r.url.as_ref())
        .map(|u| u.resource.clone())
        .find(|u| u.contains("en.wikipedia.org"))
}

/// Extract the Wikidata Q-identifier from a MusicBrainz URL-relation
/// pointing to wikidata.org. Returns the bare ID (e.g. "Q12345").
/// MB stores Wikidata links as `https://www.wikidata.org/wiki/Q12345`.
fn wikidata_id_from_relations(relations: &[MbRelation]) -> Option<String> {
    relations
        .iter()
        .filter_map(|r| r.url.as_ref())
        .map(|u| u.resource.as_str())
        .find(|u| u.contains("wikidata.org/wiki/"))
        .and_then(|u| u.rsplit('/').next().map(|s| s.to_string()))
        .filter(|id| id.starts_with('Q') && id[1..].chars().all(|c| c.is_ascii_digit()))
}

/// Resolve a Wikidata Q-id to its English Wikipedia article title via
/// the Wikidata API. Returns `Some("Article Title")` or `None`.
async fn fetch_enwiki_title_from_wikidata(qid: &str) -> Option<String> {
    let client = mb_client().ok()?;
    let url = "https://www.wikidata.org/w/api.php";
    let resp = client
        .get(url)
        .query(&[
            ("action", "wbgetentities"),
            ("ids", qid),
            ("props", "sitelinks"),
            ("sitefilter", "enwiki"),
            ("format", "json"),
        ])
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let data: WikidataEntityResponse = resp.json().await.ok()?;
    let title = data
        .entities?
        .remove(qid)?
        .sitelinks?
        .remove("enwiki")?
        .title?;
    if title.is_empty() {
        return None;
    }
    Some(title)
}

/// Resolve Wikidata Q-id all the way to a Wikipedia (URL, extract) pair.
async fn fetch_album_wiki_via_wikidata(qid: &str) -> Option<(String, String)> {
    let title = fetch_enwiki_title_from_wikidata(qid).await?;
    let url_path = title.replace(' ', "_");
    let client = mb_client().ok()?;
    let summary: WikiSummaryResponse = client
        .get(format!(
            "https://en.wikipedia.org/api/rest_v1/page/summary/{url_path}"
        ))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let extract = summary.extract?;
    let url = format!("https://en.wikipedia.org/wiki/{url_path}");
    Some((url, extract))
}

#[derive(Debug, Deserialize)]
struct WikidataEntityResponse {
    entities: Option<std::collections::HashMap<String, WikidataEntity>>,
}

#[derive(Debug, Deserialize)]
struct WikidataEntity {
    sitelinks: Option<std::collections::HashMap<String, WikidataSitelink>>,
}

#[derive(Debug, Deserialize)]
struct WikidataSitelink {
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WikipediaSearchResponse {
    pages: Option<Vec<WikipediaSearchPage>>,
}

#[derive(Debug, Deserialize)]
struct WikipediaSearchPage {
    title: Option<String>,
    /// Short text snippet used for relevance scoring. Includes the
    /// matched terms in bold — we use this to verify that the article
    /// actually mentions the artist name.
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    excerpt: Option<String>,
}

/// Tokenize an artist name into meaningful words (lowercased, length ≥ 3,
/// alphanumeric only). "DJ OK" → ["ok"], "French 79" → ["french"], "U2" → []
/// (drops too-short tokens to avoid false matches).
fn artist_tokens(artist: &str) -> Vec<String> {
    artist
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 3)
        .map(|s| s.to_string())
        .collect()
}

/// Check that the artist's tokens appear in `blob` (HTML stripped).
/// Returns true if every token is found — i.e. the blob plausibly
/// describes this artist. Used by all Wikipedia lookup paths (search,
/// Wikidata, direct URL) to reject same-named but unrelated entities.
fn blob_mentions_artist(blob: &str, artist: &str) -> bool {
    if artist.trim().is_empty() {
        return true;
    }
    let tokens = artist_tokens(artist);
    // If the artist name is too short to tokenize (e.g. "U2"), we
    // accept the match — there's no way to disambiguate anyway.
    if tokens.is_empty() {
        return true;
    }
    let blob = blob
        .replace('<', " ")
        .replace('>', " ")
        .to_lowercase();
    tokens.iter().all(|t| blob.contains(t))
}

/// Wrapper kept for the search-result path which has separate fields.
fn search_result_matches_artist(page: &WikipediaSearchPage, artist: &str) -> bool {
    let blob = format!(
        "{} {}",
        page.description.as_deref().unwrap_or(""),
        page.excerpt.as_deref().unwrap_or("")
    );
    blob_mentions_artist(&blob, artist)
}

/// Fallback: search Wikipedia directly for an album when MusicBrainz
/// doesn't have a Wikidata entry. To avoid the wrong-entity problem
/// (e.g. "Joshua" → biblical character) we (a) add the artist name to
/// the query and (b) reject the top hit unless its description/excerpt
/// actually mentions the artist.
async fn search_wikipedia_album_summary(
    album: &str,
    artist: &str,
) -> Option<(String, String)> {
    let client = mb_client().ok()?;

    // Try up to two queries: album + artist, then album alone with
    // artist-name validation against the result.
    let queries: [&str; 2] = [
        // First try: disambiguated. Quoting the album name forces exact match.
        // The "album" suffix nudges the ranking toward music-related pages.
        // We don't use this exact string as a query param; build below.
        // Placeholder; real queries are constructed below.
        "",
        "",
    ];
    // Real query strings (allocated to satisfy lifetime of &str refs).
    let q1 = format!("\"{album}\" {artist} album");
    let q2 = format!("\"{album}\" album");
    let queries: [&str; 2] = [&q1, &q2];

    for query in queries.iter() {
        let search: WikipediaSearchResponse = client
            .get("https://en.wikipedia.org/w/rest.php/v1/search/page")
            .query(&[
                ("q", query.to_string()),
                ("limit", "3".to_string()), // fetch a few so we can validate
            ])
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()?;

        let pages = search.pages.unwrap_or_default();
        // ONLY accept pages that actually mention the artist. Returning
        // an unvalidated top hit is exactly the wrong-article problem
        // ("Joshua French 79" → "The Joshua Tree" by U2).
        let Some(page) = pages
            .iter()
            .find(|p| {
                p.title.is_some()
                    && search_result_matches_artist(p, artist)
            })
        else {
            continue;
        };
        let Some(title) = page.title.as_deref() else { continue };
        if title.is_empty() {
            continue;
        }

        let url_path = title.replace(' ', "_");
        let summary: WikiSummaryResponse = client
            .get(format!(
                "https://en.wikipedia.org/api/rest_v1/page/summary/{url_path}"
            ))
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()?;

        if let Some(extract) = summary.extract {
            let url = format!("https://en.wikipedia.org/wiki/{url_path}");
            return Some((url, extract));
        }
    }

    None
}

/// Convert an MBIS barcode string ("-barcode-") into a plain digits string.
fn normalize_barcode(raw: Option<&str>) -> Option<String> {
    let s = raw?.trim();
    if s.is_empty() {
        return None;
    }
    Some(s.to_string())
}

// -- Internal fetcher (shared by cached + refresh commands) ------------------

async fn fetch_release_detail_internal(
    album_name: &str,
    artist_name: &str,
) -> Result<MbReleaseDetail, String> {
    let client = mb_client()?;
    let cleaned = clean_album_name(album_name);

    // 1) Find the release-group via release search. We need the rg_mbid
    //    so we can enumerate releases and pick the canonical one.
    let search_resp = client
        .get(format!("{}/release", MB_API_BASE))
        .query(&[
            (
                "query",
                format!("release:\"{}\" AND artist:\"{}\"", cleaned, artist_name),
            ),
            ("limit", "1".into()),
            ("inc", "labels+release-groups+artist-credits+media+recordings".into()),
            ("fmt", "json".into()),
        ])
        .send()
        .await
        .map_err(|e| format!("MusicBrainz release search error: {}", e))?;

    if !search_resp.status().is_success() {
        return Err(format!("MusicBrainz returned {}", search_resp.status()));
    }

    let search: MbReleaseSearchResponse2 = search_resp
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    let first = match search.releases.and_then(|v| v.into_iter().next()) {
        Some(r) => r,
        None => return Err("No release found on MusicBrainz".into()),
    };

    let release_group_mbid = first
        .release_group
        .as_ref()
        .map(|rg| rg.id.clone())
        .ok_or_else(|| "Release has no release-group".to_string())?;

    // 2) Enumerate releases of the release-group so we can pick the
    //    canonical one (Official + earliest).
    sleep(Duration::from_millis(1100)).await;
    let rg_resp = client
        .get(format!("{}/release-group/{}", MB_API_BASE, release_group_mbid))
        .query(&[("inc", "releases+url-rels"), ("fmt", "json")])
        .send()
        .await
        .map_err(|e| format!("MusicBrainz release-group error: {}", e))?;

    if !rg_resp.status().is_success() {
        return Err(format!("MusicBrainz returned {}", rg_resp.status()));
    }
    let rg_data: MbReleaseGroupLookup2 = rg_resp
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    let chosen = rg_data
        .releases
        .as_deref()
        .and_then(|r| pick_canonical_release(r))
        .ok_or_else(|| "Release-group has no releases".to_string())?;

    // 3) Fetch full release detail (tracklist, label-info, packaging, etc.)
    let chosen_id = chosen.id.clone();
    sleep(Duration::from_millis(1100)).await;
    let detail_resp = client
        .get(format!("{}/release/{}", MB_API_BASE, chosen_id))
        .query(&[
            ("inc", "recordings+artist-credits+labels+release-rels+url-rels+media"),
            ("fmt", "json"),
        ])
        .send()
        .await
        .map_err(|e| format!("MusicBrainz release detail error: {}", e))?;

    if !detail_resp.status().is_success() {
        return Err(format!("MusicBrainz returned {}", detail_resp.status()));
    }
    let detail: MbReleaseSearchEntry = detail_resp
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

// 4) Wikipedia URL: prefer the release's own relations, fall back to the
    //    release-group relations. `mut` because the Wikidata path below
    //    may discover a URL and want to expose it.
    let mut wiki_url = wikipedia_from_relations(detail.relations.as_deref().unwrap_or(&[]))
        .or_else(|| wikipedia_from_release_group(&rg_data));

    // 5) Wiki summary. Try three paths in order; every path validates
    //    that the returned Wikipedia article actually mentions the artist
    //    (else we fall through to the next path). Without validation,
    //    "Joshua" by French 79 returns the biblical character or U2's
    //    "The Joshua Tree" depending on popularity, neither of which
    //    mentions French 79.
    let mut wiki_extract: Option<String> = None;

    // Path A: direct Wikipedia URL from MB.
    if let Some(url) = wiki_url.as_deref() {
        sleep(Duration::from_millis(1100)).await;
        if let Some((resolved_url, extract)) = fetch_wiki_summary_for_url_simple(url).await {
            if blob_mentions_artist(&extract, artist_name) {
                wiki_extract = Some(extract);
                wiki_url = Some(resolved_url);
            } else {
                tracing::info!(
                    "Rejecting direct-MB Wikipedia match for {} — extract doesn't mention artist",
                    album_name
                );
                wiki_url = None; // force fall-through to next path
            }
        }
    }

    // Path B: Wikidata Q-id from MB relations → enwiki sitelink.
    if wiki_extract.is_none() {
        let qid = wikidata_id_from_relations(
            detail.relations.as_deref().unwrap_or(&[]),
        )
        .or_else(|| {
            rg_data
                .relations
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .filter_map(|r| r.url.as_ref())
                .map(|u| u.resource.as_str())
                .find(|u| u.contains("wikidata.org/wiki/"))
                .and_then(|u| u.rsplit('/').next().map(|s| s.to_string()))
                .filter(|id| id.starts_with('Q') && id[1..].chars().all(|c| c.is_ascii_digit()))
        });
        if let Some(qid) = qid {
            sleep(Duration::from_millis(1100)).await;
            if let Some((url, extract)) = fetch_album_wiki_via_wikidata(&qid).await {
                if blob_mentions_artist(&extract, artist_name) {
                    wiki_extract = Some(extract);
                    wiki_url = Some(url);
                } else {
                    tracing::info!(
                        "Rejecting Wikidata-resolved Wikipedia match for {} (Q={}) — extract doesn't mention artist",
                        album_name, qid
                    );
                }
            }
        }
    }

    // Path C: Wikipedia search with artist-name validation (last resort).
    if wiki_extract.is_none() {
        sleep(Duration::from_millis(1100)).await;
        if let Some((url, extract)) =
            search_wikipedia_album_summary(album_name, artist_name).await
        {
            wiki_extract = Some(extract);
            wiki_url = Some(url);
        }
    }

    // 6) Flatten tracklist from `media[].tracks[]`.
    let mut tracks: Vec<MbReleaseTrack> = Vec::new();
    let mut total_ms: u64 = 0;
    if let Some(media) = detail.media.as_deref() {
        for m in media {
            if let Some(track_list) = m.tracks.as_deref() {
                for t in track_list {
                    let (disc, pos) = parse_track_position(t.number.as_deref(), t.position);
                    if let Some(len) = t.length {
                        total_ms = total_ms.saturating_add(len);
                    }
                    let credit = t
                        .artist_credit
                        .as_deref()
                        .map(|c| {
                            c.iter()
                                .map(|a| {
                                    a.joinphrase
                                        .as_deref()
                                        .map(|j| format!("{}{}", a.name, j))
                                        .unwrap_or_else(|| a.name.clone())
                                })
                                .collect::<Vec<_>>()
                                .join("")
                        })
                        .filter(|s| !s.is_empty());
                    tracks.push(MbReleaseTrack {
                        mbid: t.id.clone(),
                        position: pos,
                        disc_number: disc,
                        title: t.title.clone(),
                        length_ms: t.length,
                        artist_credit: credit,
                    });
                }
            }
        }
    }

    // 7) Build the public struct.
    let label = detail
        .label_info
        .as_deref()
        .and_then(|infos| infos.first())
        .and_then(|li| li.label.as_ref())
        .map(|l| l.name.clone());
    let catalog = detail
        .label_info
        .as_deref()
        .and_then(|infos| infos.first())
        .and_then(|li| li.catalog_number.clone())
        .filter(|s| !s.is_empty());
    let format = detail
        .media
        .as_deref()
        .and_then(|m| m.first())
        .and_then(|m| m.format.clone());
    let year = detail.date.as_deref().map(year_from_date);
    let original_year = rg_data
        .first_release_date
        .as_deref()
        .filter(|d| !d.is_empty())
        .map(year_from_date);
    let release_type = release_type_label(&rg_data.primary_type, &rg_data.secondary_types);
    let artist_credit = detail
        .artist_credit
        .as_deref()
        .map(|c| {
            c.iter()
                .map(|a| {
                    a.joinphrase
                        .as_deref()
                        .map(|j| format!("{}{}", a.name, j))
                        .unwrap_or_else(|| a.name.clone())
                })
                .collect::<Vec<_>>()
                .join("")
        })
        .filter(|s: &String| !s.is_empty())
        .unwrap_or_else(|| artist_name.to_string());
    let artist_mbid = detail
        .artist_credit
        .as_deref()
        .and_then(|c| c.first())
        .and_then(|a| a.artist.as_ref())
        .map(|a| a.id.clone());

    // 8) Cover Art Archive URLs � we don't pre-verify existence; the
    //    browser will hit a 404 if the cover isn't uploaded.
    let cover_500 = Some(format!("{}/release/{}/front-500", CAA_BASE, detail.id));
    let cover_250 = Some(format!("{}/release/{}/front-250", CAA_BASE, detail.id));
    let cover_1200 = Some(format!("{}/release/{}/front-1200", CAA_BASE, detail.id));

    Ok(MbReleaseDetail {
        mbid: detail.id,
        release_group_mbid,
        title: detail.title,
        artist: artist_credit,
        artist_mbid,
        year,
        original_year,
        country: detail.country,
        label,
        catalog_number: catalog,
        barcode: normalize_barcode(detail.barcode.as_deref()),
        packaging: detail.packaging,
        format,
        language: detail.language,
        script: detail.script,
        release_type: Some(release_type),
        track_count: tracks.len() as u32,
        total_duration_ms: if total_ms > 0 { Some(total_ms) } else { None },
        cover_url_250: cover_250,
        cover_url_500: cover_500,
        cover_url_1200: cover_1200,
        wikipedia_url: wiki_url,
        wiki_extract,
        tracks,
    })
}

// -- Tauri commands ----------------------------------------------------------

/// Fetch rich MusicBrainz release detail (tracklist, barcode, packaging,
/// format, cover art, optional Wikipedia summary) for the modal opened
/// from AlbumDetail. Uses an in-memory cache (30-day TTL) keyed by
/// `(album, artist)` to avoid hammering MusicBrainz when the user reopens
/// the modal for the same album.
#[tauri::command]
pub async fn get_release_detail_mb(
    album_name: String,
    artist_name: String,
) -> Result<MbReleaseDetail, String> {
    let key = cache_key(&album_name, &artist_name);

    // Cache check
    {
        let cache = release_detail_cache().lock().await;
        if let Some(entry) = cache.get(&key) {
            if entry.fetched_at.elapsed().as_secs() < RELEASE_DETAIL_TTL_SECS {
                return Ok(entry.detail.clone());
            }
        }
    }

    let detail = fetch_release_detail_internal(&album_name, &artist_name).await?;

    let mut cache = release_detail_cache().lock().await;
    cache.insert(
        key,
        CachedReleaseDetail {
            detail: detail.clone(),
            fetched_at: std::time::Instant::now(),
        },
    );

    Ok(detail)
}

/// Bypasses the cache and re-fetches from MusicBrainz. Used by the
/// "refresh" button in the modal.
#[tauri::command]
pub async fn refresh_release_detail_mb(
    album_name: String,
    artist_name: String,
) -> Result<MbReleaseDetail, String> {
    let detail = fetch_release_detail_internal(&album_name, &artist_name).await?;
    let mut cache = release_detail_cache().lock().await;
    cache.insert(
        cache_key(&album_name, &artist_name),
        CachedReleaseDetail {
            detail: detail.clone(),
            fetched_at: std::time::Instant::now(),
        },
    );
    Ok(detail)
}

/// Returns the Cover Art Archive URL for a release at the requested size
/// (250, 500, or 1200). Returns `Ok(None)` if the size is invalid.
/// Existence of the cover is NOT pre-verified � the URL may 404 if no
/// cover has been uploaded for this release; the modal handles that with
/// an `<img onerror>` fallback to the local album cover.
#[tauri::command]
pub async fn get_release_cover_art(
    release_id: String,
    size: Option<u32>,
) -> Result<Option<String>, String> {
    let size = size.unwrap_or(500);
    let suffix = match size {
        250 | 500 | 1200 => size.to_string(),
        _ => return Ok(None),
    };
    Ok(Some(format!(
        "{}/release/{}/front-{}",
        CAA_BASE, release_id, suffix
    )))
}
