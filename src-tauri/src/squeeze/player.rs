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
use std::sync::Arc;
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
}

/// A connected Squeeze player.
pub struct SqueezePlayer {
    pub mac: MacAddress,
    pub name: String,
    pub capabilities: String,
    writer: Option<OwnedWriteHalf>,
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
}

impl SqueezePlayer {
    pub fn new(
        mac: MacAddress,
        name: String,
        capabilities: String,
        writer: OwnedWriteHalf,
        server_ip: Ipv4Addr,
    ) -> Self {
        Self {
            mac,
            name,
            capabilities,
            writer: Some(writer),
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
        }
    }

    /// Create a Cometd-based player (no TCP writer).
    pub fn new_cometd(mac: MacAddress, mac_str: String, uuid: String) -> Self {
        Self {
            mac,
            name: format!("Player {}", mac_str),
            capabilities: format!("cometd,uuid={}", uuid),
            writer: None,
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
        }
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
        let vol = self.volume as f64 / 100.0;
        self.send(&codec::encode_audg(vol, vol)).await?;
        Ok(())
    }

    /// Start streaming a track. Returns the new generation.
    pub async fn start_stream(
        &mut self,
        http_port: u16,
        flags: u8,
    ) -> Result<u64, String> {
        let track = self.queue.current().ok_or("No current track")?.clone();

        self.generation += 1;
        let gen = self.generation;

        if flags & 0x40 == 0 {
            // Not gapless — reset confirmations
            self.confirmed_generation = None;
        }
        self.prefetched_generation = None;

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
        self.state = PlayerState::Buffering;

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

    /// Set volume (0-100).
    pub async fn set_volume(&mut self, vol: u8) -> Result<(), String> {
        self.volume = vol.min(100);
        let v = self.volume as f64 / 100.0;
        let frame = codec::encode_audg(v, v);
        self.send(&frame).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Handle an incoming STAT event. Returns true if a track advance is needed.
    pub fn handle_stat(&mut self, stat: &StatMessage) -> StatAction {
        match stat.event {
            StatEvent::TrackStarted => {
                self.state = PlayerState::Playing;
                self.confirmed_generation = Some(self.generation);
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
                self.state = PlayerState::Paused;
                StatAction::None
            }
            StatEvent::Resumed => {
                self.state = PlayerState::Playing;
                StatAction::None
            }
            StatEvent::Flushed => {
                // Normal during transitions — do NOT set Stopped
                StatAction::None
            }
            StatEvent::Timer => {
                self.elapsed_ms = stat.elapsed_milliseconds + self.seek_offset_ms;
                StatAction::None
            }
            StatEvent::Connected => {
                self.state = PlayerState::Buffering;
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

    /// Get info for the frontend.
    pub fn info(&self) -> PlayerInfo {
        PlayerInfo {
            mac: self.mac.to_string(),
            name: self.name.clone(),
            state: self.state,
            capabilities: self.capabilities.clone(),
            current_track: self.queue.current().cloned(),
            elapsed_ms: self.elapsed_ms,
            volume: self.volume,
            repeat: self.queue.repeat,
            shuffle: self.queue.shuffle,
            queue_length: self.queue.len(),
            queue_position: self.queue.current_position(),
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
