use crate::lms_client::types::*;
use reqwest::Client;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

pub struct CometdClient {
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl CometdClient {
    pub fn new() -> Self {
        Self {
            shutdown: Arc::new(AtomicBool::new(false)),
            handle: None,
        }
    }

    pub fn start(
        &mut self,
        http_client: Client,
        base_url: String,
        player_id: String,
        status_tx: broadcast::Sender<LmsPlayerStatus>,
    ) {
        self.stop();
        self.shutdown.store(false, Ordering::Relaxed);

        let shutdown = self.shutdown.clone();
        let handle = tokio::spawn(async move {
            cometd_loop(http_client, base_url, player_id, status_tx, shutdown).await;
        });

        self.handle = Some(handle);
    }

    pub fn stop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }

    pub fn is_running(&self) -> bool {
        self.handle
            .as_ref()
            .map(|h| !h.is_finished())
            .unwrap_or(false)
    }
}

impl Drop for CometdClient {
    fn drop(&mut self) {
        self.stop();
    }
}

async fn cometd_loop(
    client: Client,
    base_url: String,
    player_id: String,
    status_tx: broadcast::Sender<LmsPlayerStatus>,
    shutdown: Arc<AtomicBool>,
) {
    let cometd_url = format!("{}/cometd", base_url);
    let mut backoff = Duration::from_secs(2);
    let max_backoff = Duration::from_secs(30);

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        tracing::info!("Cometd: starting session for player {}", player_id);

        match run_session(&client, &cometd_url, &player_id, &status_tx, &shutdown).await {
            Ok(()) => break,
            Err(e) => {
                if shutdown.load(Ordering::Relaxed) {
                    break;
                }
                tracing::warn!("Cometd session error: {}. Reconnecting in {:?}", e, backoff);
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(max_backoff);
            }
        }
    }
}

async fn run_session(
    client: &Client,
    cometd_url: &str,
    player_id: &str,
    status_tx: &broadcast::Sender<LmsPlayerStatus>,
    shutdown: &Arc<AtomicBool>,
) -> Result<(), LmsError> {
    // Handshake
    let handshake_msg = json!([{
        "channel": "/meta/handshake",
        "version": "1.0",
        "supportedConnectionTypes": ["long-polling"]
    }]);

    let resp: Vec<Value> = client
        .post(cometd_url)
        .json(&handshake_msg)
        .send()
        .await?
        .json()
        .await?;

    let client_id = resp
        .first()
        .and_then(|r| r.get("clientId"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| LmsError::Parse("No clientId in handshake response".into()))?
        .to_string();

    tracing::info!("Cometd handshake successful, clientId: {}", client_id);

    // Subscribe to player status
    let subscribe_msg = json!([{
        "channel": "/slim/subscribe",
        "clientId": client_id,
        "data": {
            "response": format!("/{}/status", client_id),
            "request": [player_id, ["status", "-", "1", "subscribe:30", "tags:aAlcdegiIJKlLNorstuxyY"]]
        }
    }]);

    let _: Vec<Value> = client
        .post(cometd_url)
        .json(&subscribe_msg)
        .send()
        .await?
        .json()
        .await?;

    tracing::info!("Cometd subscribed to player: {}", player_id);

    // Long-poll connect loop
    loop {
        if shutdown.load(Ordering::Relaxed) {
            // Disconnect
            let disconnect_msg = json!([{
                "channel": "/meta/disconnect",
                "clientId": client_id
            }]);
            let _ = client.post(cometd_url).json(&disconnect_msg).send().await;
            return Ok(());
        }

        let connect_msg = json!([{
            "channel": "/meta/connect",
            "clientId": client_id,
            "connectionType": "long-polling"
        }]);

        let resp: Vec<Value> = client
            .post(cometd_url)
            .json(&connect_msg)
            .timeout(Duration::from_secs(60))
            .send()
            .await?
            .json()
            .await?;

        for msg in &resp {
            let channel = msg.get("channel").and_then(|c| c.as_str()).unwrap_or("");

            // Check for connection failure
            if channel == "/meta/connect" {
                let successful = msg.get("successful").and_then(|s| s.as_bool()).unwrap_or(true);
                if !successful {
                    return Err(LmsError::Network("Connect rejected by server".into()));
                }
            }

            // Parse status updates (channel matches our subscription response path)
            if channel.contains("/status") && channel != "/meta/subscribe" {
                if let Some(data) = msg.get("data") {
                    if let Ok(status) = parse_cometd_status(player_id, data) {
                        let _ = status_tx.send(status);
                    }
                }
            }
        }
    }
}

fn parse_cometd_status(player_id: &str, data: &Value) -> Result<LmsPlayerStatus, LmsError> {
    let mode_str = data
        .get("mode")
        .and_then(|m| m.as_str())
        .unwrap_or("stop");

    let mode = match mode_str {
        "play" => PlayMode::Playing,
        "pause" => PlayMode::Paused,
        _ => PlayMode::Stopped,
    };

    let repeat_val = data
        .get("playlist repeat")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let repeat = match repeat_val {
        1 => RepeatMode::One,
        2 => RepeatMode::All,
        _ => RepeatMode::Off,
    };

    let shuffle_val = data
        .get("playlist shuffle")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let shuffle = match shuffle_val {
        1 => ShuffleMode::Songs,
        2 => ShuffleMode::Albums,
        _ => ShuffleMode::Off,
    };

    let playlist_loop = data
        .get("playlist_loop")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let playlist: Vec<LmsTrack> = playlist_loop.iter().filter_map(parse_track).collect();

    let playlist_index = data
        .get("playlist_cur_index")
        .map(|v| match v {
            Value::Number(n) => n.as_u64().unwrap_or(0) as usize,
            Value::String(s) => s.parse().unwrap_or(0),
            _ => 0,
        })
        .unwrap_or(0);

    let current_track = playlist.get(playlist_index).cloned();

    Ok(LmsPlayerStatus {
        player_id: player_id.to_string(),
        player_name: data
            .get("player_name")
            .and_then(|n| n.as_str())
            .unwrap_or("")
            .to_string(),
        mode,
        time: data
            .get("time")
            .and_then(|t| t.as_f64())
            .unwrap_or(0.0),
        duration: data
            .get("duration")
            .and_then(|d| d.as_f64())
            .unwrap_or(0.0),
        volume: data
            .get("mixer volume")
            .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(0)
            .clamp(0, 100) as u8,
        repeat,
        shuffle,
        playlist_tracks: data
            .get("playlist_tracks")
            .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(0) as usize,
        playlist_index,
        current_track,
        playlist,
    })
}

fn parse_track(v: &Value) -> Option<LmsTrack> {
    Some(LmsTrack {
        id: v
            .get("id")
            .and_then(|id| id.as_i64().or_else(|| id.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(-1),
        title: v
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("Unknown")
            .to_string(),
        artist: v
            .get("artist")
            .and_then(|a| a.as_str())
            .unwrap_or("")
            .to_string(),
        album: v
            .get("album")
            .and_then(|a| a.as_str())
            .unwrap_or("")
            .to_string(),
        duration: v.get("duration").and_then(|d| d.as_f64()).unwrap_or(0.0),
        bitrate: v.get("bitrate").and_then(|b| b.as_str()).map(String::from),
        format: v
            .get("type")
            .or_else(|| v.get("content_type"))
            .and_then(|f| f.as_str())
            .map(String::from),
        artwork_url: v
            .get("artwork_url")
            .or_else(|| v.get("coverart"))
            .and_then(|u| u.as_str())
            .map(String::from),
        url: v.get("url").and_then(|u| u.as_str()).map(String::from),
    })
}
