// Per-player state and TCP connection management.
//
// Each connected Squeeze player has a `SqueezePlayer` holding its TCP writer,
// playback state, queue, and generation counters for gapless prefetch.

use crate::squeeze::codec::{self, MacAddress, StatEvent, StatMessage, AudioFormat, StrmStartParams};
use crate::squeeze::queue::{PlayQueue, QueueTrack, RepeatMode};
use serde::Serialize;
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::path::Path;
use std::sync::{Arc, LazyLock};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::Mutex;

/// Playback state reported to the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PlayerState {
    Disconnected,
    Stopped,
    Buffering,
    Playing,
    Paused,
}

/// Info about a connected player (sent to frontend).
#[derive(Debug, Clone, Serialize)]
pub struct PlayerInfo {
    pub mac: String,
    pub name: String,
    pub state: PlayerState,
    pub capabilities: String,
    pub current_track: Option<QueueTrack>,
    pub elapsed_ms: u32,
    pub volume: u8,
    pub repeat: RepeatMode,
    pub shuffle: bool,
    pub queue_length: usize,
    pub queue_position: Option<usize>,
    pub current_queue_index: Option<usize>,
}

static NEXT_TCP_SESSION: AtomicU64 = AtomicU64::new(1);
// Generation URLs must remain distinct when a player entry is reconstructed.
// A randomized process seed also avoids predictably reusing URLs after restart.
static NEXT_STREAM_GENERATION: LazyLock<AtomicU64> =
    LazyLock::new(|| AtomicU64::new(rand::random::<u64>().max(1)));

/// A connected Squeeze player.
pub struct SqueezePlayer {
    pub mac: MacAddress,
    pub name: String,
    pub capabilities: String,
    writer: Option<OwnedWriteHalf>,
    tcp_session_id: Option<u64>,
    pub state: PlayerState,
    pub queue: PlayQueue,
    pub volume: u8, // 0-100
    pub elapsed_ms: u32,
    pub seek_offset_ms: u32,
    /// Stream generation — incremented each time a new stream is started.
    pub generation: u64,
    /// Generation confirmed by STMs (track started).
    pub confirmed_generation: Option<u64>,
    /// Generation for which STMd prefetch was done.
    pub prefetched_generation: Option<u64>,
    /// Suppress STMu (track finished) temporarily during seeks.
    pub suppress_track_finished: bool,
    /// Server IP to advertise to this player for HTTP streaming.
    pub server_ip: Ipv4Addr,
    /// Whether this is a Cometd (HTTP) player vs binary SlimProto.
    pub is_cometd: bool,
    /// Wall-clock instant when playback started (for CometD elapsed tracking).
    pub play_started_at: Option<Instant>,
    /// Track to show in UI during gapless transition (before old track finishes).
    pub display_track: Option<QueueTrack>,
    /// Underlying occurrence paired with display_track while the next stream is buffered.
    display_queue_index: Option<usize>,
}

/// Map known MAC addresses to friendly device names.
pub fn friendly_name(mac: &MacAddress) -> Option<&'static str> {
    match mac.to_string().as_str() {
        "80:0a:80:5e:d6:a5" => Some("EverSolo Play"),
        _ => None,
    }
}

pub struct SeekPlan {
    pub track: QueueTrack,
    pub elapsed_ms: u32,
    occurrence: usize,
}
impl SqueezePlayer {
    pub fn new(
        mac: MacAddress,
        name: String,
        capabilities: String,
        writer: OwnedWriteHalf,
        server_ip: Ipv4Addr,
    ) -> Self {
        let display_name = friendly_name(&mac).map(|s| s.to_string()).unwrap_or(name);
        Self {
            mac,
            name: display_name,
            capabilities,
            writer: Some(writer),
            tcp_session_id: Some(NEXT_TCP_SESSION.fetch_add(1, Ordering::Relaxed)),
            state: PlayerState::Stopped,
            queue: PlayQueue::new(),
            volume: 80,
            elapsed_ms: 0,
            seek_offset_ms: 0,
            generation: 0,
            confirmed_generation: None,
            prefetched_generation: None,
            suppress_track_finished: false,
            server_ip,
            is_cometd: false,
            play_started_at: None,
            display_track: None,
            display_queue_index: None,
        }
    }

    /// Create a Cometd-based player (no TCP writer).
    pub fn new_cometd(mac: MacAddress, mac_str: String, uuid: String) -> Self {
        let display_name = friendly_name(&mac)
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("Player {}", mac_str));
        Self {
            mac,
            name: display_name,
            capabilities: format!("cometd,uuid={}", uuid),
            writer: None,
            tcp_session_id: None,
            state: PlayerState::Stopped,
            queue: PlayQueue::new(),
            volume: 80,
            elapsed_ms: 0,
            seek_offset_ms: 0,
            generation: 0,
            confirmed_generation: None,
            prefetched_generation: None,
            suppress_track_finished: false,
            server_ip: Ipv4Addr::LOCALHOST,
            is_cometd: true,
            play_started_at: None,
            display_track: None,
            display_queue_index: None,
        }
    }

    /// Attach a TCP writer to this player (e.g., when a TCP connection arrives
    /// for a player that was already registered via CometD).
    pub fn set_writer(&mut self, writer: OwnedWriteHalf, server_ip: Ipv4Addr) {
        self.writer = Some(writer);
        self.tcp_session_id = Some(NEXT_TCP_SESSION.fetch_add(1, Ordering::Relaxed));
        self.server_ip = server_ip;
    }

    /// Identity of the reader that owns the current TCP writer.
    pub fn tcp_session_id(&self) -> Option<u64> {
        self.tcp_session_id
    }

    pub fn owns_tcp_session(&self, session_id: u64) -> bool {
        self.tcp_session_id == Some(session_id)
    }

    /// Returns true if this player has an active TCP writer (i.e., the
    /// hardware player is currently connected via SlimProto). Used by
    /// the CometD disconnect / watchdog paths to know whether they are
    /// the sole owner of the player entry, or whether the TCP path
    /// should be left to manage cleanup.
    pub fn has_tcp_writer(&self) -> bool {
        self.writer.is_some()
    }

    /// Clear the TCP writer without touching anything else. Used when
    /// a TCP connection closes but a CometD session is still active
    /// for the same MAC — we don't want to drop the player entry.
    pub fn clear_writer(&mut self) {
        self.writer = None;
        self.tcp_session_id = None;
    }

    /// Reject queued reader actions while retaining the writer for final stop.
    pub(super) fn retire_tcp_session(&mut self) {
        self.tcp_session_id = None;
    }

    /// Get current elapsed time in milliseconds.
    /// Uses wall-clock time since playback started for smooth progress tracking.
    pub fn get_elapsed_ms(&self) -> u32 {
        if let Some(started) = self.play_started_at {
            if self.state == PlayerState::Playing {
                return self.elapsed_ms.saturating_add(started.elapsed().as_millis().min(u32::MAX as u128) as u32);
            }
        }
        self.elapsed_ms
    }

    /// Send raw bytes to the player (SlimProto only).
    pub async fn send(&mut self, data: &[u8]) -> Result<(), std::io::Error> {
        if let Some(ref mut writer) = self.writer {
            writer.write_all(data).await?;
            writer.flush().await?;
        }
        Ok(())
    }

    /// Send the initial handshake sequence.
    pub async fn send_handshake(&mut self) -> Result<(), std::io::Error> {
        // 1. Server version
        self.send(&codec::encode_vers("7.999.999")).await?;
        // 2. Query status (strm 'q' — also stops any previous stream)
        self.send(&codec::encode_strm_simple(codec::StrmCommand::Stop, 0)).await?;
        // 3. Query player name
        self.send(&codec::encode_setd_query_name()).await?;
        // 4. Enable both DAC and SPDIF
        self.send(&codec::encode_aude(true, true)).await?;
        // 5. Set initial volume
        let gain = if self.volume == 0 {
            0.0
        } else {
            let db = -50.0 * (1.0 - self.volume as f64 / 100.0);
            10.0_f64.powf(db / 20.0)
        };
        self.send(&codec::encode_audg(gain, gain)).await?;
        Ok(())
    }

    /// Allocate a fresh opaque stream identity shared by all player lifetimes.
    pub fn advance_stream_generation(&mut self) -> u64 {
        self.generation = NEXT_STREAM_GENERATION.fetch_add(1, Ordering::Relaxed);
        self.generation
    }

    /// Start streaming a track. Returns the current generation.
    /// Callers must call `advance_stream_generation` before queuing the file.
    pub async fn start_stream(
        &mut self,
        http_port: u16,
        flags: u8,
    ) -> Result<u64, String> {
        let track = self.queue.current().ok_or("No current track")?.clone();

        let gen = self.generation;

        if flags & 0x40 == 0 {
            // Not gapless — reset confirmations
            self.confirmed_generation = None;
            self.prefetched_generation = None;
        }

        let ext = Path::new(&track.path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("mp3");
        let format = AudioFormat::from_extension(ext);

        let http_request = format!(
            "GET /stream?player={}&gen={} HTTP/1.0\r\n\r\n",
            self.mac, gen
        );

        let params = StrmStartParams {
            format,
            server_port: http_port,
            server_ip: self.server_ip.octets(),
            http_request,
            replay_gain: 1.0,
            flags,
            transition_type: 0,
            transition_period: 0,
            threshold_kb: 40,
            output_threshold_ds: 0,
        };

        let frame = codec::encode_strm_start(&params);
        self.send(&frame).await.map_err(|e| e.to_string())?;
        if flags & 0x40 == 0 {
            self.state = PlayerState::Buffering;
        }

        tracing::info!(
            "Squeeze: started stream gen={} track=\"{}\" on player {}",
            gen, track.title, self.mac
        );

        Ok(gen)
    }

    /// Pause the player.
    pub async fn pause(&mut self) -> Result<(), String> {
        let frame = codec::encode_strm_simple(codec::StrmCommand::Pause, 0);
        self.send(&frame).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Resume (unpause) the player.
    pub async fn resume(&mut self) -> Result<(), String> {
        let frame = codec::encode_strm_simple(codec::StrmCommand::Unpause, 0);
        self.send(&frame).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Stop the player.
    pub async fn stop(&mut self) -> Result<(), String> {
        let frame = codec::encode_strm_simple(codec::StrmCommand::Stop, 0);
        self.send(&frame).await.map_err(|e| e.to_string())?;
        self.state = PlayerState::Stopped;
        Ok(())
    }

    /// Flush the player buffers.
    pub async fn flush(&mut self) -> Result<(), String> {
        let frame = codec::encode_strm_simple(codec::StrmCommand::Flush, 0);
        self.send(&frame).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Set volume (0-100).  Maps slider position linearly in dB then converts
    /// to linear gain so the perceived loudness change is uniform.
    /// Range: 0 → mute, 1 → −50 dB, 100 → 0 dB (LMS standard).
    pub async fn set_volume(&mut self, vol: u8) -> Result<(), String> {
        self.volume = vol.min(100);
        let gain = if self.volume == 0 {
            0.0
        } else {
            let db = -50.0 * (1.0 - self.volume as f64 / 100.0);
            10.0_f64.powf(db / 20.0)
        };
        let fixed = (gain * 65536.0) as u32;
        tracing::info!(
            "Squeeze: set_volume vol={} dB={:.1} gain={:.6} fixed={}",
            self.volume,
            if self.volume == 0 { f64::NEG_INFINITY } else { -50.0 * (1.0 - self.volume as f64 / 100.0) },
            gain,
            fixed
        );
        let frame = codec::encode_audg(gain, gain);
        self.send(&frame).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Handle an incoming STAT event. Returns true if a track advance is needed.
    pub fn handle_stat(&mut self, stat: &StatMessage) -> StatAction {
        match stat.event {
            StatEvent::TrackStarted => {
                self.state = PlayerState::Playing;
                self.confirmed_generation = Some(self.generation);
                self.display_track = None;
                self.elapsed_ms = self.seek_offset_ms;
                self.seek_offset_ms = 0;
                self.play_started_at = Some(Instant::now());
                tracing::debug!("Squeeze: STMs gen={} player={}", self.generation, self.mac);
                StatAction::None
            }
            StatEvent::DecoderReady => {
                // Input consumed — prefetch next track if confirmed
                if self.confirmed_generation == Some(self.generation)
                    && self.prefetched_generation != Some(self.generation)
                {
                    if self.queue.peek_next().is_some() {
                        self.prefetched_generation = Some(self.generation);
                        tracing::debug!("Squeeze: STMd prefetch gen={} player={}", self.generation, self.mac);
                        return StatAction::Prefetch;
                    }
                }
                StatAction::None
            }
            StatEvent::Underrun => {
                // Track finished
                if self.suppress_track_finished {
                    self.suppress_track_finished = false;
                    return StatAction::None;
                }
                tracing::debug!("Squeeze: STMu (track finished) player={}", self.mac);
                StatAction::TrackFinished
            }
            StatEvent::Paused => {
                let device_ms = stat.elapsed_milliseconds;
                // Always use device-reported time when available (IR handler may have
                // already set state to Paused with a wall-clock estimate)
                if device_ms > 0 {
                    self.elapsed_ms = device_ms;
                } else if self.state == PlayerState::Playing {
                    self.elapsed_ms = self.get_elapsed_ms();
                }
                self.play_started_at = None;
                self.state = PlayerState::Paused;
                tracing::info!(
                    "Squeeze: STMp paused player={} device_elapsed_ms={} device_elapsed_s={} saved_elapsed_ms={}",
                    self.mac, stat.elapsed_milliseconds, stat.elapsed_seconds, self.elapsed_ms
                );
                StatAction::None
            }
            StatEvent::Resumed => {
                let device_ms = stat.elapsed_milliseconds;
                if device_ms > 0 {
                    self.elapsed_ms = device_ms;
                }
                self.play_started_at = Some(Instant::now());
                self.state = PlayerState::Playing;
                tracing::info!(
                    "Squeeze: STMr resumed player={} device_elapsed_ms={} device_elapsed_s={} saved_elapsed_ms={}",
                    self.mac, stat.elapsed_milliseconds, stat.elapsed_seconds, self.elapsed_ms
                );
                StatAction::None
            }
            StatEvent::Flushed => {
                // Normal during transitions — do NOT set Stopped
                StatAction::None
            }
            StatEvent::Timer => {
                tracing::debug!(
                    "Squeeze: STMt timer player={} device_elapsed_ms={} device_elapsed_s={} our_elapsed_ms={} state={:?}",
                    self.mac, stat.elapsed_milliseconds, stat.elapsed_seconds, self.get_elapsed_ms(), self.state
                );
                StatAction::None
            }
            StatEvent::Connected => {
                if self.state != PlayerState::Playing {
                    self.state = PlayerState::Buffering;
                }
                StatAction::None
            }
            StatEvent::BufferThreshold => {
                StatAction::None
            }
            StatEvent::NotSupported => {
                tracing::warn!("Squeeze: format not supported by player {}", self.mac);
                self.state = PlayerState::Stopped;
                StatAction::None
            }
            StatEvent::OutputUnderrun => {
                StatAction::None
            }
            StatEvent::Unknown(_) => StatAction::None,
        }
    }

    /// Validate the exact audible occurrence before any transport effect.
    pub fn seek_plan(&self, seconds: f64) -> Result<SeekPlan, String> {
        if !seconds.is_finite() || seconds < 0.0 { return Err("Invalid seek position".into()); }
        let occurrence = if self.display_track.is_some() { self.display_queue_index } else { self.queue.current_track_index() }.ok_or("No audible queue occurrence")?;
        let track=self.queue.track_at(occurrence).ok_or("Audible queue occurrence is unavailable")?;
        if self.display_track.as_ref().is_some_and(|display| display.id != track.id || display.path != track.path || display.duration != track.duration) { return Err("Stale audible queue occurrence".into()); }
        if !track.duration.is_finite() || track.duration <= 0.0 { return Err("Track has no seekable duration".into()); }
        let elapsed_ms=(seconds.min(track.duration).min(u32::MAX as f64/1000.0)*1000.0).min(u32::MAX as f64) as u32;
        Ok(SeekPlan { track:track.clone(),elapsed_ms,occurrence })
    }
    /// Caller retains the player lock through stop, flush, queue-file and restart.
    pub fn begin_seek(&mut self, plan: &SeekPlan) -> Result<(), String> {
        let current=self.seek_plan(plan.elapsed_ms as f64/1000.0)?;
        if current.occurrence != plan.occurrence || current.track.path != plan.track.path || current.track.id != plan.track.id || current.track.duration != plan.track.duration { return Err("Seek occurrence changed".into()); }
        self.queue.select_occurrence(plan.occurrence)?;
        self.display_track=None; self.display_queue_index=None;
        self.prefetched_generation=None; self.confirmed_generation=None;
        self.suppress_track_finished=true;
        self.seek_offset_ms=plan.elapsed_ms;
        self.advance_stream_generation();
        Ok(())
    }
    /// Capture the audible occurrence before the queue cursor advances for prefetch.
    pub fn retain_audible_occurrence(&mut self) {
        self.display_track = self.queue.current().cloned();
        self.display_queue_index = self.queue.current_track_index();
    }

    pub fn insert_queue(&mut self, tracks: Vec<QueueTrack>, position: usize) {
        let insert_at = position.min(self.queue.len());
        if self.display_track.is_some() {
            self.display_queue_index = self.display_queue_index.map(|index| {
                if index >= insert_at { index + tracks.len() } else { index }
            });
        }
        self.queue.insert_tracks(tracks, position);
    }

    /// Retained source indices describe occurrence identity, never inferred from duplicate IDs.
    /// All validation happens under the caller's player lock before publishing either cursor.
    pub fn replace_queue(
        &mut self,
        tracks: Vec<QueueTrack>,
        current_track_id: i64,
        current_index: Option<usize>,
        source_indices: Option<Vec<Option<usize>>>,
    ) -> Result<(), String> {
        let busy = || "SQUEEZE_QUEUE_BUSY: buffered occurrence cannot be resolved or removed; retry after transition".to_string();
        if let Some(sources) = &source_indices {
            if sources.len() != tracks.len() { return Err("Queue source map length is invalid".into()); }
            let mut seen = std::collections::HashSet::new();
            for (track, source) in tracks.iter().zip(sources) {
                if let Some(source) = source {
                    if !seen.insert(*source) || self.queue.track_at(*source).map(|old| old.id) != Some(track.id) {
                        return Err("Queue source occurrence is invalid".into());
                    }
                }
            }
        }
        let (cursor_id, cursor_index, display_index) = if let Some(display) = &self.display_track {
            let sources = source_indices.as_ref().ok_or_else(busy)?;
            let audible = self.display_queue_index.ok_or_else(busy)?;
            let buffered = self.queue.current_track_index().ok_or_else(busy)?;
            let new_audible = sources.iter().position(|index| *index == Some(audible)).ok_or_else(busy)?;
            let new_buffered = sources.iter().position(|index| *index == Some(buffered)).ok_or_else(busy)?;
            if current_index != Some(new_audible) || display.id != current_track_id {
                return Err("Current audible queue occurrence is invalid".into());
            }
            (tracks[new_buffered].id, Some(new_buffered), Some(new_audible))
        } else {
            if let (Some(sources), Some(index), Some(previous)) = (&source_indices, current_index, self.queue.current_track_index()) {
                if sources.get(index) != Some(&Some(previous)) {
                    return Err("Current queue source occurrence is invalid".into());
                }
            }
            (current_track_id, current_index, None)
        };
        // Work on a clone so even queue validation cannot leave half of the pair changed.
        let mut replacement = self.queue.clone();
        replacement.replace_queue_keep_current_at(tracks, cursor_id, cursor_index)?;
        self.queue = replacement;
        if self.display_track.is_some() { self.display_queue_index = display_index; }
        Ok(())
    }

    /// Get info for the frontend, keeping displayed metadata and occurrence paired.
    pub fn info(&self) -> PlayerInfo {
        PlayerInfo {
            mac: self.mac.to_string(),
            name: self.name.clone(),
            state: self.state,
            capabilities: self.capabilities.clone(),
            current_track: self.display_track.as_ref().or_else(|| self.queue.current()).cloned(),
            elapsed_ms: self.get_elapsed_ms(),
            volume: self.volume,
            repeat: self.queue.repeat,
            shuffle: self.queue.shuffle,
            queue_length: self.queue.len(),
            queue_position: self.queue.current_position(),
            current_queue_index: if self.display_track.is_some() { self.display_queue_index } else { self.queue.current_track_index() },
        }
    }
}

/// Action to take after processing a STAT event.
#[derive(Debug, PartialEq)]
pub enum StatAction {
    None,
    Prefetch,
    TrackFinished,
}

/// Thread-safe map of connected players.
pub type PlayerMap = Arc<Mutex<HashMap<MacAddress, SqueezePlayer>>>;

pub fn new_player_map() -> PlayerMap {
    Arc::new(Mutex::new(HashMap::new()))
}

#[cfg(test)]
mod audible_occurrence_tests {
    use super::*;
    fn track(id: i64) -> QueueTrack {
        QueueTrack { id, title: id.to_string(), artist: String::new(), album: String::new(), path: String::new(), duration: 100.0, format: "flac".into() }
    }
    fn player(shuffle: bool) -> SqueezePlayer {
        let mut p = SqueezePlayer::new_cometd(MacAddress([1, 2, 3, 4, 5, 6]), "fixture".into(), "fixture".into());
        p.queue.shuffle = shuffle;
        p.queue.set_tracks(vec![track(7), track(7), track(7)], 2);
        p
    }
    fn confirm(p: &mut SqueezePlayer) {
        p.handle_stat(&StatMessage { event: StatEvent::TrackStarted, buffer_size: 0, buffer_fullness: 0, bytes_received: 0, signal_strength: 0, jiffies: 0, output_buffer_size: 0, output_buffer_fullness: 0, elapsed_seconds: 0, elapsed_milliseconds: 0, timestamp: 0 });
    }
    #[test]
    fn seek_plan_uses_audible_duplicate_occurrence_and_preserves_shuffle_order() {
        for shuffled in [false, true] {
            let mut p=player(shuffled); p.queue.repeat=RepeatMode::All;
            p.retain_audible_occurrence(); p.queue.next(); p.prefetched_generation=Some(12);
            let plan=p.seek_plan(200.0).unwrap(); assert_eq!(plan.elapsed_ms,100000);
            p.begin_seek(&plan).unwrap();
            assert_eq!(p.queue.current_track_index(),Some(2)); assert_eq!(p.info().current_queue_index,Some(2));
            assert!(p.prefetched_generation.is_none()); assert!(p.display_track.is_none());
            assert_eq!(p.queue.shuffle,shuffled); assert_eq!(p.queue.repeat,RepeatMode::All);
        }
    }
    #[test]
    fn seek_rejects_invalid_bounds_and_stale_pair_without_mutation() {
        let mut p=player(false);
        for value in [f64::NAN,f64::INFINITY,-1.0] { assert!(p.seek_plan(value).is_err()); }
        p.retain_audible_occurrence(); p.display_queue_index=Some(99);
        let old=p.queue.current_track_index(); assert!(p.seek_plan(1.0).is_err()); assert_eq!(p.queue.current_track_index(),old);
    }
    #[test]
    fn seek_retains_shifted_occurrence_not_first_duplicate_id() {
        let mut p=player(false);p.queue.repeat=RepeatMode::All;p.retain_audible_occurrence();p.queue.next();
        p.insert_queue(vec![track(8)],0);let plan=p.seek_plan(1.5).unwrap();p.begin_seek(&plan).unwrap();
        assert_eq!(p.queue.current_track_index(),Some(3));assert_eq!(plan.elapsed_ms,1500);
    }
    #[test]
    fn audible_occurrence_is_retained_until_actual_start_including_shuffle() {
        for shuffled in [false, true] {
            let mut p = player(shuffled);
            p.queue.repeat = RepeatMode::All;
            assert_eq!(p.info().current_queue_index, Some(2));
            p.retain_audible_occurrence();
            p.queue.next();
            let prefetched = p.queue.current_track_index();
            assert_ne!(prefetched, Some(2));
            assert_eq!(p.info().current_queue_index, Some(2));
            assert_eq!(p.info().current_track.unwrap().id, 7);
            assert_eq!(p.info().queue_position, p.queue.current_position());
            confirm(&mut p);
            assert_eq!(p.info().current_queue_index, prefetched);
        }
    }
    #[test]
    fn replacement_maps_both_retained_occurrences_for_duplicate_shuffle() {
        for shuffled in [false, true] {
            let mut p = player(shuffled);
            p.queue.repeat = RepeatMode::All;
            p.retain_audible_occurrence(); p.queue.next();
            let prefetched = p.queue.current_track_index().unwrap();
            let sources = vec![Some(2), Some(0), Some(1)];
            let expected_prefetched = sources.iter().position(|i| *i == Some(prefetched));
            p.replace_queue(vec![track(7), track(7), track(7)], 7, Some(0), Some(sources)).unwrap();
            assert_eq!(p.info().current_queue_index, Some(0));
            assert_eq!(p.queue.current_track_index(), expected_prefetched);
            confirm(&mut p);
            assert_eq!(p.info().current_queue_index, expected_prefetched);
        }
    }
    #[test]
    fn prefetch_replacement_rejects_ambiguous_or_removed_buffered_occurrence_before_mutation() {
        let mut p = player(false); p.queue.repeat = RepeatMode::All;
        p.retain_audible_occurrence(); p.queue.next(); // audible2, buffered0
        let before = serde_json::to_value(p.info()).unwrap();
        for sources in [None, Some(vec![Some(2)]), Some(vec![Some(2), None]), Some(vec![Some(2), Some(2)]), Some(vec![Some(2), Some(9)]), Some(vec![Some(2), Some(1)])] {
            assert!(p.replace_queue(vec![track(7), track(7)], 7, Some(0), sources).is_err());
            assert_eq!(serde_json::to_value(p.info()).unwrap(), before);
            assert_eq!(p.queue.current_track_index(), Some(0));
        }
        assert!(p.replace_queue(vec![track(7), track(8)], 7, Some(0), Some(vec![Some(2), Some(0)])).is_err());
        assert_eq!(serde_json::to_value(p.info()).unwrap(), before);
        // Non-buffered repeated occurrence can be removed normally.
        p.replace_queue(vec![track(7), track(7)], 7, Some(0), Some(vec![Some(2), Some(0)])).unwrap();
        assert_eq!(p.info().current_queue_index, Some(0));
        assert_eq!(p.queue.current_track_index(), Some(1));
    }
    #[test]
    fn insertion_shifts_retained_audible_and_prefetched_indices_together() {
        let mut p = player(false); p.queue.repeat = RepeatMode::All;
        p.retain_audible_occurrence(); p.queue.next();
        p.insert_queue(vec![track(9)], 0);
        assert_eq!(p.info().current_queue_index, Some(3));
        assert_eq!(p.queue.current_track_index(), Some(1));
        confirm(&mut p);
        assert_eq!(p.info().current_queue_index, Some(1));
    }
}
