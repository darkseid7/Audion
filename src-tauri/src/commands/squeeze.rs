use crate::lms_client::discovery;
use crate::lms_client::types::*;
use crate::lms_client::LmsClient;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;

pub struct LmsState {
    pub client: Arc<Mutex<LmsClient>>,
}

impl LmsState {
    pub fn new(db: crate::db::Database) -> Self {
        Self {
            client: Arc::new(Mutex::new(LmsClient::new(db))),
        }
    }
}

// ── Discovery & connection ─────────────────────────────────────────────────

#[tauri::command]
pub async fn lms_discover_servers() -> Result<Vec<LmsServer>, String> {
    discovery::discover_servers()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn lms_connect(
    host: String,
    port: u16,
    state: State<'_, LmsState>,
) -> Result<(), String> {
    let mut client = state.client.lock().await;
    client.connect(&host, port).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn lms_disconnect(state: State<'_, LmsState>) -> Result<(), String> {
    let mut client = state.client.lock().await;
    client.disconnect();
    Ok(())
}

#[tauri::command]
pub async fn lms_is_connected(state: State<'_, LmsState>) -> Result<bool, String> {
    let client = state.client.lock().await;
    Ok(client.is_connected())
}

#[tauri::command]
pub async fn lms_get_connected_server(
    state: State<'_, LmsState>,
) -> Result<Option<LmsServer>, String> {
    let client = state.client.lock().await;
    Ok(client.connected_server().cloned())
}

// ── Players ────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn lms_get_players(state: State<'_, LmsState>) -> Result<Vec<LmsPlayer>, String> {
    let client = state.client.lock().await;
    client.get_players().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn lms_get_player_status(
    player_id: String,
    state: State<'_, LmsState>,
) -> Result<LmsPlayerStatus, String> {
    let client = state.client.lock().await;
    client
        .get_player_status(&player_id)
        .await
        .map_err(|e| e.to_string())
}

// ── Playback control ───────────────────────────────────────────────────────

#[tauri::command]
pub async fn lms_play(player_id: String, state: State<'_, LmsState>) -> Result<(), String> {
    let client = state.client.lock().await;
    client.play(&player_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn lms_pause(player_id: String, state: State<'_, LmsState>) -> Result<(), String> {
    let client = state.client.lock().await;
    client.pause(&player_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn lms_resume(player_id: String, state: State<'_, LmsState>) -> Result<(), String> {
    let client = state.client.lock().await;
    client.unpause(&player_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn lms_stop(player_id: String, state: State<'_, LmsState>) -> Result<(), String> {
    let client = state.client.lock().await;
    client.stop(&player_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn lms_next(player_id: String, state: State<'_, LmsState>) -> Result<(), String> {
    let client = state.client.lock().await;
    client
        .next_track(&player_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn lms_previous(player_id: String, state: State<'_, LmsState>) -> Result<(), String> {
    let client = state.client.lock().await;
    client
        .previous_track(&player_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn lms_seek(
    player_id: String,
    seconds: f64,
    state: State<'_, LmsState>,
) -> Result<(), String> {
    let client = state.client.lock().await;
    client
        .seek(&player_id, seconds)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn lms_set_volume(
    player_id: String,
    volume: u8,
    state: State<'_, LmsState>,
) -> Result<(), String> {
    let client = state.client.lock().await;
    client
        .set_volume(&player_id, volume)
        .await
        .map_err(|e| e.to_string())
}

// ── Queue management ───────────────────────────────────────────────────────

#[tauri::command]
pub async fn lms_play_tracks(
    player_id: String,
    track_ids: Vec<i64>,
    start_index: usize,
    state: State<'_, LmsState>,
) -> Result<(), String> {
    let client = state.client.lock().await;
    client
        .play_tracks(&player_id, &track_ids, start_index)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn lms_add_tracks(
    player_id: String,
    track_ids: Vec<i64>,
    state: State<'_, LmsState>,
) -> Result<(), String> {
    let client = state.client.lock().await;
    client
        .add_tracks(&player_id, &track_ids)
        .await
        .map_err(|e| e.to_string())
}

// ── Status subscription ────────────────────────────────────────────────────

#[tauri::command]
pub async fn lms_subscribe_status(
    player_id: String,
    app: AppHandle,
    state: State<'_, LmsState>,
) -> Result<(), String> {
    let mut client = state.client.lock().await;
    client.subscribe_status(&player_id);

    let mut rx = client.status_receiver();
    let client_arc = state.client.clone();
    tokio::spawn(async move {
        while let Ok(mut status) = rx.recv().await {
            let client = client_arc.lock().await;
            client.enrich_metadata(&mut status);
            drop(client);
            let _ = app.emit("lms://player-status", &status);
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn lms_unsubscribe_status(state: State<'_, LmsState>) -> Result<(), String> {
    let mut client = state.client.lock().await;
    client.unsubscribe_status();
    Ok(())
}
