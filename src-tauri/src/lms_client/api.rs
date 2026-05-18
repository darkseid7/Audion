use crate::lms_client::types::*;
use reqwest::Client;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};

static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

pub struct LmsApi {
    client: Client,
    base_url: String,
}

impl LmsApi {
    pub fn new(client: Client, host: &str, port: u16) -> Self {
        Self {
            client,
            base_url: format!("http://{}:{}", host, port),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    async fn jsonrpc(&self, player_id: &str, command: Vec<Value>) -> Result<Value, LmsError> {
        let id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        let body = json!({
            "id": id,
            "method": "slim.request",
            "params": [player_id, command]
        });

        let resp = self
            .client
            .post(format!("{}/jsonrpc.js", self.base_url))
            .json(&body)
            .send()
            .await?;

        let result: Value = resp.json().await?;
        Ok(result.get("result").cloned().unwrap_or(Value::Null))
    }

    // ── Server queries ─────────────────────────────────────────────────────

    pub async fn get_players(&self) -> Result<Vec<LmsPlayer>, LmsError> {
        let result = self
            .jsonrpc("", vec![json!("serverstatus"), json!("0"), json!("100")])
            .await?;

        let players_loop = result
            .get("players_loop")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let players = players_loop
            .into_iter()
            .filter_map(|p| parse_player(&p))
            .collect();

        Ok(players)
    }

    pub async fn get_player_status(&self, player_id: &str) -> Result<LmsPlayerStatus, LmsError> {
        let result = self
            .jsonrpc(
                player_id,
                vec![
                    json!("status"),
                    json!("0"),
                    json!("100"),
                    json!("tags:aAlcdegiIJKlLNorstuxyY"),
                ],
            )
            .await?;

        parse_player_status(player_id, &result)
    }

    // ── Playback control ───────────────────────────────────────────────────

    pub async fn play(&self, player_id: &str) -> Result<(), LmsError> {
        self.jsonrpc(player_id, vec![json!("play")]).await?;
        Ok(())
    }

    pub async fn pause(&self, player_id: &str) -> Result<(), LmsError> {
        self.jsonrpc(player_id, vec![json!("pause"), json!("1")])
            .await?;
        Ok(())
    }

    pub async fn unpause(&self, player_id: &str) -> Result<(), LmsError> {
        self.jsonrpc(player_id, vec![json!("pause"), json!("0")])
            .await?;
        Ok(())
    }

    pub async fn stop(&self, player_id: &str) -> Result<(), LmsError> {
        self.jsonrpc(player_id, vec![json!("stop")]).await?;
        Ok(())
    }

    pub async fn next_track(&self, player_id: &str) -> Result<(), LmsError> {
        self.jsonrpc(
            player_id,
            vec![json!("playlist"), json!("index"), json!("+1")],
        )
        .await?;
        Ok(())
    }

    pub async fn previous_track(&self, player_id: &str) -> Result<(), LmsError> {
        self.jsonrpc(
            player_id,
            vec![json!("playlist"), json!("index"), json!("-1")],
        )
        .await?;
        Ok(())
    }

    pub async fn seek(&self, player_id: &str, seconds: f64) -> Result<(), LmsError> {
        self.jsonrpc(player_id, vec![json!("time"), json!(seconds)])
            .await?;
        Ok(())
    }

    pub async fn set_volume(&self, player_id: &str, volume: u8) -> Result<(), LmsError> {
        let vol_str = volume.min(100).to_string();
        self.jsonrpc(
            player_id,
            vec![json!("mixer"), json!("volume"), json!(vol_str)],
        )
        .await?;
        Ok(())
    }

    // ── Playlist/queue management ──────────────────────────────────────────

    pub async fn playlist_play(&self, player_id: &str, url: &str, title: &str) -> Result<(), LmsError> {
        self.jsonrpc(
            player_id,
            vec![json!("playlist"), json!("play"), json!(url), json!(title)],
        )
        .await?;
        Ok(())
    }

    pub async fn playlist_add(&self, player_id: &str, url: &str, title: &str) -> Result<(), LmsError> {
        self.jsonrpc(
            player_id,
            vec![json!("playlist"), json!("add"), json!(url), json!(title)],
        )
        .await?;
        Ok(())
    }

    pub async fn playlist_insert(&self, player_id: &str, url: &str, title: &str) -> Result<(), LmsError> {
        self.jsonrpc(
            player_id,
            vec![json!("playlist"), json!("insert"), json!(url), json!(title)],
        )
        .await?;
        Ok(())
    }

    pub async fn playlist_clear(&self, player_id: &str) -> Result<(), LmsError> {
        self.jsonrpc(player_id, vec![json!("playlist"), json!("clear")])
            .await?;
        Ok(())
    }

    pub async fn playlist_jump(&self, player_id: &str, index: usize) -> Result<(), LmsError> {
        self.jsonrpc(
            player_id,
            vec![json!("playlist"), json!("index"), json!(index)],
        )
        .await?;
        Ok(())
    }

    /// Get artwork URL for a track
    pub fn artwork_url(&self, track_id: i64) -> String {
        format!("{}/music/{}/cover.jpg", self.base_url, track_id)
    }
}

// ── Response parsers ───────────────────────────────────────────────────────

fn parse_player(v: &Value) -> Option<LmsPlayer> {
    Some(LmsPlayer {
        player_id: v.get("playerid")?.as_str()?.to_string(),
        name: v
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("Unknown")
            .to_string(),
        model: v
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string(),
        connected: v.get("connected").and_then(|c| c.as_i64()).unwrap_or(0) == 1,
        power: v.get("power").and_then(|p| p.as_i64()).unwrap_or(0) == 1,
        is_playing: v.get("isplaying").and_then(|p| p.as_i64()).unwrap_or(0) == 1,
    })
}

fn parse_player_status(player_id: &str, result: &Value) -> Result<LmsPlayerStatus, LmsError> {
    let mode_str = result
        .get("mode")
        .and_then(|m| m.as_str())
        .unwrap_or("stop");

    let mode = match mode_str {
        "play" => PlayMode::Playing,
        "pause" => PlayMode::Paused,
        _ => PlayMode::Stopped,
    };

    let repeat_val = result
        .get("playlist repeat")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let repeat = match repeat_val {
        1 => RepeatMode::One,
        2 => RepeatMode::All,
        _ => RepeatMode::Off,
    };

    let shuffle_val = result
        .get("playlist shuffle")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let shuffle = match shuffle_val {
        1 => ShuffleMode::Songs,
        2 => ShuffleMode::Albums,
        _ => ShuffleMode::Off,
    };

    let playlist_loop = result
        .get("playlist_loop")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let playlist: Vec<LmsTrack> = playlist_loop.iter().filter_map(parse_track).collect();

    let playlist_index = result
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
        player_name: result
            .get("player_name")
            .and_then(|n| n.as_str())
            .unwrap_or("")
            .to_string(),
        mode,
        time: result
            .get("time")
            .and_then(|t| t.as_f64())
            .unwrap_or(0.0),
        duration: result
            .get("duration")
            .and_then(|d| d.as_f64())
            .unwrap_or(0.0),
        volume: result
            .get("mixer volume")
            .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(0)
            .clamp(0, 100) as u8,
        repeat,
        shuffle,
        playlist_tracks: result
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
