// Tauri IPC commands for Squeeze Connect.

use crate::db::Database;
use crate::squeeze::codec::MacAddress;
use crate::squeeze::player::{PlayerInfo, PlayerState};
use crate::squeeze::queue::{QueueTrack, RepeatMode};
use crate::squeeze::server::HTTP_PORT;
use crate::squeeze::streaming::StreamingState;
use crate::squeeze::SqueezeServer;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tauri::State;
use tokio::sync::Mutex;

/// Tauri-managed state wrapper.
pub struct SqueezeState(pub Arc<Mutex<SqueezeServer>>);

impl SqueezeState {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(SqueezeServer::new())))
    }
}

fn parse_mac(mac_str: &str) -> Result<MacAddress, String> {
    let parts: Vec<&str> = mac_str.split(':').collect();
    if parts.len() != 6 {
        return Err(format!("Invalid MAC address: {}", mac_str));
    }
    let mut bytes = [0u8; 6];
    for (i, part) in parts.iter().enumerate() {
        bytes[i] = u8::from_str_radix(part, 16)
            .map_err(|_| format!("Invalid MAC byte: {}", part))?;
    }
    Ok(MacAddress(bytes))
}

// ── Server lifecycle ────────────────────────────────────────────────────────

#[tauri::command]
pub async fn squeeze_start_server(state: State<'_, SqueezeState>) -> Result<(), String> {
    let mut server = state.0.lock().await;
    server.start().await
}

#[tauri::command]
pub async fn squeeze_stop_server(state: State<'_, SqueezeState>) -> Result<(), String> {
    let mut server = state.0.lock().await;
    server.stop().await;
    Ok(())
}

#[tauri::command]
pub async fn squeeze_is_running(state: State<'_, SqueezeState>) -> Result<bool, String> {
    let server = state.0.lock().await;
    Ok(server.is_running())
}

// ── Player info ─────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn squeeze_get_players(state: State<'_, SqueezeState>) -> Result<Vec<PlayerInfo>, String> {
    let server = state.0.lock().await;
    let map = server.players.lock().await;
    let infos: Vec<PlayerInfo> = map.values().map(|p| p.info()).collect();
    Ok(infos)
}

#[tauri::command]
pub async fn squeeze_get_player_state(
    mac: String,
    state: State<'_, SqueezeState>,
) -> Result<PlayerInfo, String> {
    let mac = parse_mac(&mac)?;
    let server = state.0.lock().await;
    let map = server.players.lock().await;
    let player = map.get(&mac).ok_or("Player not found")?;
    Ok(player.info())
}

/// Hard-disconnect a player: stop it, close its TCP connection, remove
/// it from the player map, and clear its queued stream. Idempotent —
/// disconnecting an already-gone player is a no-op success.
#[tauri::command]
pub async fn squeeze_disconnect_player(
    mac: String,
    state: State<'_, SqueezeState>,
) -> Result<(), String> {
    let mac_addr = parse_mac(&mac)?;
    let server = state.0.lock().await;
    crate::squeeze::server::disconnect_player(
        &mac_addr,
        &server.players,
        &server.streaming,
        &server.cometd,
    )
    .await;
    Ok(())
}

// ── Playback control ────────────────────────────────────────────────────────

#[tauri::command]
pub async fn squeeze_play(
    mac: String,
    track_ids: Vec<i64>,
    start_index: usize,
    state: State<'_, SqueezeState>,
    db: State<'_, Database>,
) -> Result<(), String> {
    let mac_addr = parse_mac(&mac)?;
    eprintln!("[SQUEEZE] play: {} track IDs, start_index={}", track_ids.len(), start_index);

    // Look up tracks from DB
    let tracks: Vec<QueueTrack> = {
        let conn = db.conn.lock().unwrap();
        track_ids.iter().filter_map(|&id| {
            crate::db::queries::get_track_by_id(&conn, id)
                .ok()
                .flatten()
                .map(|t| QueueTrack {
                    id: t.id,
                    title: t.title.unwrap_or_else(|| "Unknown".to_string()),
                    artist: t.artist.unwrap_or_default(),
                    album: t.album.unwrap_or_default(),
                    path: t.path,
                    duration: t.duration.map(|d| d as f64).unwrap_or(0.0),
                    format: t.format.unwrap_or_else(|| "mp3".to_string()),
                })
        }).collect()
    };

    eprintln!("[SQUEEZE] resolved {} tracks from DB", tracks.len());

    if tracks.is_empty() {
        return Err("No valid tracks found".into());
    }

    let server = state.0.lock().await;

    // Stop current stream before starting new one
    {
        let mut map = server.players.lock().await;
        let player = map.get_mut(&mac_addr).ok_or("Player not found")?;
        player.display_track = None;
        player.prefetched_generation = None;
        player.suppress_track_finished = true;
        player.stop().await?;
        player.flush().await?;
    }

    // Set the queue
    {
        let mut map = server.players.lock().await;
        let player = map.get_mut(&mac_addr).ok_or("Player not found")?;
        player.queue.set_tracks(tracks, start_index);
        player.seek_offset_ms = 0;
    }

    // Queue the file for HTTP streaming
    let path = {
        let map = server.players.lock().await;
        let player = map.get(&mac_addr).ok_or("Player not found")?;
        let track = player.queue.current().ok_or("No track in queue")?;
        eprintln!("[SQUEEZE] starting: \"{}\" by {}", track.title, track.artist);
        PathBuf::from(&track.path)
    };

    let gen = {
        let mut map = server.players.lock().await;
        let player = map.get_mut(&mac_addr).ok_or("Player not found")?;
        player.advance_stream_generation();
        player.generation
    };

    server.streaming.queue_file(&mac_addr, path, gen, 0).await;

    // Start streaming
    {
        let mut map = server.players.lock().await;
        let player = map.get_mut(&mac_addr).ok_or("Player not found")?;
        player.start_stream(HTTP_PORT, 0).await?;
        player.elapsed_ms = 0;
        player.play_started_at = Some(Instant::now());
        if player.is_cometd {
            player.state = PlayerState::Playing;
        }
    }

    // Notify CometD subscribers
    server.cometd.notify_player_status(&mac).await;

    eprintln!("[SQUEEZE] playback started successfully");
    Ok(())
}

#[tauri::command]
pub async fn squeeze_pause(mac: String, state: State<'_, SqueezeState>) -> Result<(), String> {
    let mac_addr = parse_mac(&mac)?;
    let server = state.0.lock().await;
    {
        let mut map = server.players.lock().await;
        let player = map.get_mut(&mac_addr).ok_or("Player not found")?;
        player.elapsed_ms = player.get_elapsed_ms();
        player.play_started_at = None;
        if player.is_cometd {
            player.state = PlayerState::Paused;
        }
        player.pause().await?;
    }
    server.cometd.notify_player_status(&mac).await;
    Ok(())
}

#[tauri::command]
pub async fn squeeze_resume(mac: String, state: State<'_, SqueezeState>) -> Result<(), String> {
    let mac_addr = parse_mac(&mac)?;
    let server = state.0.lock().await;
    {
        let mut map = server.players.lock().await;
        let player = map.get_mut(&mac_addr).ok_or("Player not found")?;
        player.play_started_at = Some(Instant::now());
        if player.is_cometd {
            player.state = PlayerState::Playing;
        }
        player.resume().await?;
    }
    server.cometd.notify_player_status(&mac).await;
    Ok(())
}

#[tauri::command]
pub async fn squeeze_stop(mac: String, state: State<'_, SqueezeState>) -> Result<(), String> {
    let mac_addr = parse_mac(&mac)?;
    let server = state.0.lock().await;
    {
        let mut map = server.players.lock().await;
        let player = map.get_mut(&mac_addr).ok_or("Player not found")?;
        player.elapsed_ms = 0;
        player.play_started_at = None;
        player.stop().await?;
    }
    server.cometd.notify_player_status(&mac).await;
    Ok(())
}

#[tauri::command]
pub async fn squeeze_set_volume(
    mac: String,
    volume: u8,
    state: State<'_, SqueezeState>,
) -> Result<(), String> {
    let mac_addr = parse_mac(&mac)?;
    let server = state.0.lock().await;
    {
        let mut map = server.players.lock().await;
        let player = map.get_mut(&mac_addr).ok_or("Player not found")?;
        player.set_volume(volume).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn squeeze_next(mac: String, state: State<'_, SqueezeState>) -> Result<(), String> {
    let mac_addr = parse_mac(&mac)?;
    let server = state.0.lock().await;

    // Stop current stream
    {
        let mut map = server.players.lock().await;
        let player = map.get_mut(&mac_addr).ok_or("Player not found")?;
        player.display_track = None;
        player.stop().await?;
        player.flush().await?;
        player.suppress_track_finished = true;
    }

    // Advance queue (skip if prefetch already advanced it)
    let has_next = {
        let mut map = server.players.lock().await;
        let player = map.get_mut(&mac_addr).ok_or("Player not found")?;
        if player.prefetched_generation.is_some() {
            // Prefetch already advanced the queue
            player.prefetched_generation = None;
            player.queue.current().is_some()
        } else {
            player.queue.next().is_some()
        }
    };

    if !has_next {
        return Err("No next track".into());
    }

    // Queue and start
    let path = {
        let map = server.players.lock().await;
        let player = map.get(&mac_addr).ok_or("Player not found")?;
        let track = player.queue.current().ok_or("No track")?;
        PathBuf::from(&track.path)
    };

    let gen = {
        let mut map = server.players.lock().await;
        let player = map.get_mut(&mac_addr).ok_or("Player not found")?;
        player.advance_stream_generation();
        player.generation
    };

    server.streaming.queue_file(&mac_addr, path, gen, 0).await;

    {
        let mut map = server.players.lock().await;
        let player = map.get_mut(&mac_addr).ok_or("Player not found")?;
        player.seek_offset_ms = 0;
        player.elapsed_ms = 0;
        player.play_started_at = Some(Instant::now());
        player.start_stream(HTTP_PORT, 0).await?;
        if player.is_cometd {
            player.state = PlayerState::Playing;
        }
    }

    // Notify CometD subscribers
    server.cometd.notify_player_status(&mac).await;

    Ok(())
}

#[tauri::command]
pub async fn squeeze_previous(mac: String, state: State<'_, SqueezeState>) -> Result<(), String> {
    let mac_addr = parse_mac(&mac)?;
    let server = state.0.lock().await;

    // Stop current stream
    {
        let mut map = server.players.lock().await;
        let player = map.get_mut(&mac_addr).ok_or("Player not found")?;
        player.display_track = None;
        player.stop().await?;
        player.flush().await?;
        player.suppress_track_finished = true;
    }

    // Go back in queue (undo prefetch advance first if needed)
    {
        let mut map = server.players.lock().await;
        let player = map.get_mut(&mac_addr).ok_or("Player not found")?;
        if player.prefetched_generation.is_some() {
            player.queue.previous(); // undo prefetch advance
            player.prefetched_generation = None;
        }
        if player.queue.previous().is_none() {
            return Err("No previous track".into());
        }
    }

    // Queue and start
    let path = {
        let map = server.players.lock().await;
        let player = map.get(&mac_addr).ok_or("Player not found")?;
        let track = player.queue.current().ok_or("No track")?;
        PathBuf::from(&track.path)
    };

    let gen = {
        let mut map = server.players.lock().await;
        let player = map.get_mut(&mac_addr).ok_or("Player not found")?;
        player.advance_stream_generation();
        player.generation
    };

    server.streaming.queue_file(&mac_addr, path, gen, 0).await;

    {
        let mut map = server.players.lock().await;
        let player = map.get_mut(&mac_addr).ok_or("Player not found")?;
        player.seek_offset_ms = 0;
        player.elapsed_ms = 0;
        player.play_started_at = Some(Instant::now());
        player.start_stream(HTTP_PORT, 0).await?;
        if player.is_cometd {
            player.state = PlayerState::Playing;
        }
    }

    // Notify CometD subscribers
    server.cometd.notify_player_status(&mac).await;

    Ok(())
}

#[tauri::command]
pub async fn squeeze_seek(
    mac: String,
    position_seconds: f64,
    state: State<'_, SqueezeState>,
) -> Result<(), String> {
    let mac_addr = parse_mac(&mac)?;
    let server = state.0.lock().await;

    // Retain one player lock across restart: STAT/prefetch, replacement and
    // reconnect cannot replace the captured audible occurrence mid-seek.
    {
        let mut map = server.players.lock().await;
        let player = map.get_mut(&mac_addr).ok_or("Player not found")?;
        let plan = player.seek_plan(position_seconds)?;
        let path = PathBuf::from(&plan.track.path);
        let file_size = std::fs::metadata(&path).map_err(|e| format!("Seek file unavailable: {e}"))?.len();
        let seconds = plan.elapsed_ms as f64 / 1000.0;
        let byte_offset = ((seconds / plan.track.duration) * file_size as f64) as u64;
        // A failed stop has an unknown outcome and is never followed by a start.
        player.stop().await?;
        let partial = |e| format!("SQUEEZE_SEEK_PARTIAL: output stopped; {e}");
        player.flush().await.map_err(partial)?;
        player.begin_seek(&plan).map_err(partial)?;
        server.streaming.queue_file(&mac_addr, path, player.generation, byte_offset).await;
        player.start_stream(HTTP_PORT, 0).await.map_err(partial)?;
        player.elapsed_ms = plan.elapsed_ms;
        player.play_started_at = Some(Instant::now());
        if player.is_cometd { player.state = PlayerState::Playing; }
    }

    server.cometd.notify_player_status(&mac).await;

    Ok(())
}

// ── Queue management ────────────────────────────────────────────────────────

#[tauri::command]
pub async fn squeeze_set_repeat(
    mac: String,
    mode: String,
    state: State<'_, SqueezeState>,
) -> Result<(), String> {
    let mac_addr = parse_mac(&mac)?;
    let repeat = match mode.as_str() {
        "off" => RepeatMode::Off,
        "one" => RepeatMode::One,
        "all" => RepeatMode::All,
        _ => return Err(format!("Invalid repeat mode: {}", mode)),
    };
    let server = state.0.lock().await;
    let mut map = server.players.lock().await;
    let player = map.get_mut(&mac_addr).ok_or("Player not found")?;
    player.queue.repeat = repeat;
    Ok(())
}

#[tauri::command]
pub async fn squeeze_set_shuffle(
    mac: String,
    enabled: bool,
    state: State<'_, SqueezeState>,
) -> Result<(), String> {
    let mac_addr = parse_mac(&mac)?;
    let server = state.0.lock().await;
    let mut map = server.players.lock().await;
    let player = map.get_mut(&mac_addr).ok_or("Player not found")?;
    player.queue.set_shuffle(enabled);
    Ok(())
}

#[tauri::command]
pub async fn squeeze_get_queue(
    mac: String,
    state: State<'_, SqueezeState>,
) -> Result<Vec<QueueTrack>, String> {
    let mac_addr = parse_mac(&mac)?;
    let server = state.0.lock().await;
    let map = server.players.lock().await;
    let player = map.get(&mac_addr).ok_or("Player not found")?;
    Ok(player.queue.ordered_tracks().into_iter().cloned().collect())
}

#[tauri::command]
pub async fn squeeze_insert_queue(
    mac: String,
    track_ids: Vec<i64>,
    position: i64,
    state: State<'_, SqueezeState>,
    db: State<'_, Database>,
) -> Result<(), String> {
    let mac_addr = parse_mac(&mac)?;

    let tracks: Vec<QueueTrack> = {
        let conn = db.conn.lock().unwrap();
        track_ids.iter().filter_map(|&id| {
            crate::db::queries::get_track_by_id(&conn, id)
                .ok()
                .flatten()
                .map(|t| QueueTrack {
                    id: t.id,
                    title: t.title.unwrap_or_else(|| "Unknown".to_string()),
                    artist: t.artist.unwrap_or_default(),
                    album: t.album.unwrap_or_default(),
                    path: t.path,
                    duration: t.duration.map(|d| d as f64).unwrap_or(0.0),
                    format: t.format.unwrap_or_else(|| "mp3".to_string()),
                })
        }).collect()
    };

    if tracks.is_empty() {
        return Err("No valid tracks found".into());
    }

    let server = state.0.lock().await;
    let mut map = server.players.lock().await;
    let player = map.get_mut(&mac_addr).ok_or("Player not found")?;
    // -1 means "append to end". Anything ≥ 0 is a real position, clamped
    // to the queue length inside insert_tracks.
    let position: usize = if position < 0 {
        player.queue.len()
    } else {
        position as usize
    };
    player.insert_queue(tracks, position);

    Ok(())
}

#[tauri::command]
pub async fn squeeze_update_queue(
    mac: String,
    track_ids: Vec<i64>,
    current_track_id: i64,
    current_index: Option<usize>,
    source_indices: Option<Vec<Option<usize>>>,
    state: State<'_, SqueezeState>,
    db: State<'_, Database>,
) -> Result<(), String> {
    let mac_addr = parse_mac(&mac)?;

    let tracks: Vec<QueueTrack> = {
        let conn = db.conn.lock().unwrap();
        resolve_queue_replacement(&track_ids, current_index, |id| {
            crate::db::queries::get_track_by_id(&conn, id)
                .map_err(|error| error.to_string())
                .map(|track| track.map(|t| QueueTrack {
                    id: t.id,
                    title: t.title.unwrap_or_else(|| "Unknown".to_string()),
                    artist: t.artist.unwrap_or_default(),
                    album: t.album.unwrap_or_default(),
                    path: t.path,
                    duration: t.duration.map(|d| d as f64).unwrap_or(0.0),
                    format: t.format.unwrap_or_else(|| "mp3".to_string()),
                }))
        })?
    };

    let server = state.0.lock().await;
    let mut map = server.players.lock().await;
    let player = map.get_mut(&mac_addr).ok_or("Player not found")?;
    player.replace_queue(tracks, current_track_id, current_index, source_indices)?;

    Ok(())
}

/// Explicit occurrence indices require a complete resolution; filtering would shift identity.
fn resolve_queue_replacement(
    ids: &[i64],
    current_index: Option<usize>,
    mut load: impl FnMut(i64) -> Result<Option<QueueTrack>, String>,
) -> Result<Vec<QueueTrack>, String> {
    let mut tracks = Vec::with_capacity(ids.len());
    for &id in ids {
        match load(id) {
            Ok(Some(track)) => tracks.push(track),
            Ok(None) if current_index.is_some() => return Err(format!("Track {id} is unavailable")),
            Err(error) if current_index.is_some() => return Err(error),
            _ => {}, // Retain the legacy omitted-index filtering behavior.
        }
    }
    Ok(tracks)
}

#[cfg(test)]
mod occurrence_resolution_tests {
    use super::*;
    #[test]
    fn explicit_occurrence_rejects_missing_tracks_instead_of_shifting_indices() {
        let resolved = resolve_queue_replacement(&[7, 9, 7], Some(2), |_| Ok(None));
        assert!(resolved.is_err());
    }
    #[test]
    fn legacy_resolution_keeps_filtering_missing_tracks() {
        let resolved = resolve_queue_replacement(&[7, 9], None, |_| Ok(None)).unwrap();
        assert!(resolved.is_empty());
    }
}
