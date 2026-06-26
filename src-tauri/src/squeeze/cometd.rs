// Cometd/Bayeux protocol handler for modern Squeeze players (e.g. Eversolo).
//
// These players use HTTP long-polling on port 9000 instead of the binary
// SlimProto TCP protocol. The protocol flow is:
//   1. POST /cometd  { channel: "/meta/handshake", ... }  → get clientId
//   2. POST /cometd  { channel: "/meta/connect", clientId }  → long-poll
//   3. POST /cometd  { channel: "/meta/subscribe", ... }  → subscribe to events
//   4. Server pushes commands via the long-poll response

use crate::squeeze::codec::MacAddress;
use crate::squeeze::player::{PlayerMap, PlayerState};
use crate::squeeze::streaming::StreamingState;
use crate::squeeze::server::HTTP_PORT;
use axum::extract::State as AxumState;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use tokio::sync::{Mutex, Notify};

// ── Bayeux message types ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct BayeuxRequest {
    pub channel: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(rename = "clientId")]
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(rename = "connectionType")]
    #[serde(default)]
    pub connection_type: Option<String>,
    #[serde(rename = "supportedConnectionTypes")]
    #[serde(default)]
    pub supported_connection_types: Option<Vec<String>>,
    #[serde(default)]
    pub subscription: Option<String>,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
    #[serde(default)]
    pub ext: Option<serde_json::Value>,
    #[serde(default)]
    pub advice: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BayeuxResponse {
    pub channel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(rename = "clientId")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub successful: bool,
    #[serde(rename = "supportedConnectionTypes")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported_connection_types: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advice: Option<BayeuxAdvice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ext: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BayeuxAdvice {
    pub reconnect: String,
    pub interval: u32,
    pub timeout: u32,
}

impl BayeuxResponse {
    fn success(channel: &str, id: Option<String>, client_id: Option<String>) -> Self {
        Self {
            channel: channel.to_string(),
            id,
            version: None,
            client_id,
            successful: true,
            supported_connection_types: None,
            advice: None,
            data: None,
            ext: None,
            subscription: None,
        }
    }
}

// ── Cometd state ─────────────────────────────────────────────────────────────

/// A connected Cometd player client.
#[derive(Debug)]
pub struct CometdClient {
    pub client_id: String,
    pub mac: MacAddress,
    pub uuid: String,
    pub name: String,
    pub subscriptions: Vec<String>,
    /// Map of player MAC → response channel for status subscriptions.
    pub status_channels: HashMap<String, String>,
    /// Queue of messages to push to this client via the long-poll connect.
    pub push_queue: Vec<serde_json::Value>,
    /// Notify when there's data to push.
    pub notify: Arc<Notify>,
    /// Last time we received any HTTP request from this client. Updated
    /// on every cometd_handler call so a background watchdog can evict
    /// sessions whose long-poll just timed out without ever sending a
    /// proper /meta/disconnect (e.g. the user closed the Eversolo WebUI
    /// tab or the network dropped).
    pub last_seen: Instant,
}

/// Shared Cometd state.
#[derive(Clone)]
pub struct CometdState {
    /// Map from clientId → CometdClient.
    pub clients: Arc<Mutex<HashMap<String, CometdClient>>>,
    /// Map from MAC string → clientId (for command routing).
    pub mac_to_client: Arc<Mutex<HashMap<String, String>>>,
    /// Player map shared with the rest of the squeeze system.
    pub players: PlayerMap,
    /// Streaming state for queueing audio files.
    pub streaming: StreamingState,
    /// Counter for generating client IDs.
    counter: Arc<Mutex<u64>>,
}

impl CometdState {
    pub fn new(players: PlayerMap, streaming: StreamingState) -> Self {
        Self {
            clients: Arc::new(Mutex::new(HashMap::new())),
            mac_to_client: Arc::new(Mutex::new(HashMap::new())),
            players,
            streaming,
            counter: Arc::new(Mutex::new(1)),
        }
    }

    async fn next_client_id(&self) -> String {
        let mut c = self.counter.lock().await;
        let id = format!("{:08x}", *c);
        *c += 1;
        id
    }
}

// ── Handler ──────────────────────────────────────────────────────────────────

pub async fn cometd_handler(
    AxumState(state): AxumState<CometdState>,
    body: axum::body::Bytes,
) -> axum::response::Response {
    let body_str = String::from_utf8_lossy(&body);
    tracing::info!("Cometd: raw request ({} bytes): {}", body.len(), &body_str[..body_str.len().min(500)]);

    // Try parsing as array first, then as single object
    let messages: Vec<BayeuxRequest> = match serde_json::from_slice(&body) {
        Ok(msgs) => msgs,
        Err(_) => {
            // Try parsing as a single message wrapped in array
            match serde_json::from_slice::<BayeuxRequest>(&body) {
                Ok(msg) => vec![msg],
                Err(e) => {
                    tracing::error!("Cometd: JSON parse error: {} body={}", e, body_str);
                    let err_resp = vec![serde_json::to_value(BayeuxResponse {
                        channel: "/meta/handshake".to_string(),
                        id: None,
                        version: None,
                        client_id: None,
                        successful: false,
                        supported_connection_types: None,
                        advice: Some(BayeuxAdvice {
                            reconnect: "handshake".to_string(),
                            interval: 1000,
                            timeout: 30000,
                        }),
                        data: None,
                        ext: None,
                        subscription: None,
                    }).unwrap()];
                    return build_cometd_response(&err_resp);
                }
            }
        }
    };

    let mut responses: Vec<serde_json::Value> = Vec::new();

    for msg in &messages {
        tracing::info!("Cometd: channel={} id={:?} clientId={:?}", msg.channel, msg.id, msg.client_id);

        // Refresh liveness for any clientId we can identify — this is
        // what the watchdog uses to evict sessions that vanished without
        // sending a /meta/disconnect (closed tab, dropped network, etc).
        // Skip the handshake itself: a brand-new clientId isn't in the
        // map yet, and the handshake handler sets last_seen itself.
        if msg.channel != "/meta/handshake" {
            if let Some(cid) = msg.client_id.as_deref() {
                let mut clients = state.clients.lock().await;
                if let Some(c) = clients.get_mut(cid) {
                    c.last_seen = Instant::now();
                }
            }
        }

        match msg.channel.as_str() {
            "/meta/handshake" => {
                responses.push(serde_json::to_value(handle_handshake(&state, msg).await).unwrap());
            }
            "/meta/connect" => {
                let resp = handle_connect(&state, msg).await;
                responses.extend(resp);
            }
            "/meta/subscribe" => {
                responses.push(serde_json::to_value(handle_subscribe(&state, msg).await).unwrap());
            }
            "/meta/disconnect" => {
                responses.push(serde_json::to_value(handle_disconnect(&state, msg).await).unwrap());
            }
            "/slim/subscribe" => {
                let resp = handle_slim_subscribe(&state, msg).await;
                responses.extend(resp);
            }
            _ => {
                // Could be a slim request or player command
                if msg.channel.contains("/slim/request") {
                    let resp = handle_slim_request(&state, msg).await;
                    responses.extend(resp);
                } else {
                    tracing::info!("Cometd: unknown channel {}", msg.channel);
                    responses.push(serde_json::to_value(BayeuxResponse::success(&msg.channel, msg.id.clone(), msg.client_id.clone())).unwrap());
                }
            }
        }
    }

    build_cometd_response(&responses)
}

/// Build an HTTP response matching LMS Cometd behavior:
/// - Content-Type: application/json
/// - Connection: keep-alive (explicit for HTTP/1.1 reuse)
/// - Cache-Control/Pragma/Expires headers (LMS compat)
fn build_cometd_response(responses: &[serde_json::Value]) -> axum::response::Response {
    let json = serde_json::to_string(responses).unwrap_or_else(|_| "[]".to_string());
    tracing::info!("Cometd: response ({} bytes): {}", json.len(), &json[..json.len().min(300)]);
    axum::http::Response::builder()
        .header("Content-Type", "application/json")
        .header("Connection", "keep-alive")
        .header("Cache-Control", "no-cache")
        .header("Pragma", "no-cache")
        .header("Expires", "-1")
        .body(axum::body::Body::from(json))
        .unwrap()
}

// ── Handshake ────────────────────────────────────────────────────────────────

async fn handle_handshake(state: &CometdState, msg: &BayeuxRequest) -> BayeuxResponse {
    let client_id = state.next_client_id().await;

    // Extract MAC, UUID, and optionally name from ext
    let (mac_str, uuid, ext_name) = if let Some(ext) = &msg.ext {
        tracing::info!("Cometd: handshake ext = {:?}", ext);
        let mac = ext.get("mac").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let uuid = ext.get("uuid").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let name = ext.get("name").and_then(|v| v.as_str())
            .or_else(|| ext.get("devicename").and_then(|v| v.as_str()))
            .or_else(|| ext.get("deviceName").and_then(|v| v.as_str()))
            .map(|s| s.to_string());
        (mac, uuid, name)
    } else {
        (String::new(), String::new(), None)
    };

    // Parse MAC
    let mac = parse_mac_address(&mac_str);

    tracing::info!(
        "Cometd: handshake from MAC={} UUID={} name={:?} → clientId={}",
        mac_str, uuid, ext_name, client_id
    );

    // Use device name from ext if available, otherwise fall back to MAC
    let player_name = ext_name.clone().unwrap_or_else(|| format!("Player {}", mac_str));

    // Register the client
    let client = CometdClient {
        client_id: client_id.clone(),
        mac,
        uuid: uuid.clone(),
        name: player_name.clone(),
        subscriptions: Vec::new(),
        status_channels: HashMap::new(),
        push_queue: Vec::new(),
        notify: Arc::new(Notify::new()),
        last_seen: Instant::now(),
    };

    {
        let mut clients = state.clients.lock().await;
        clients.insert(client_id.clone(), client);
    }
    {
        let mut mac_map = state.mac_to_client.lock().await;
        mac_map.insert(mac_str.clone(), client_id.clone());
    }

    // Register in the player map (so the frontend sees this player)
    // Don't overwrite existing players — they may already have a real name from SETD
    {
        use crate::squeeze::player::SqueezePlayer;
        let mut map = state.players.lock().await;
        if let Some(existing) = map.get_mut(&mac) {
            // Player already registered (e.g., via TCP). Update CometD flag but keep name.
            existing.is_cometd = true;
            if let Some(ref n) = ext_name {
                // Only update name if CometD handshake sent a real name
                existing.name = n.clone();
            }
            tracing::info!("Cometd: merged into existing player {} (\"{}\")", mac_str, existing.name);
        } else {
            let mut player = SqueezePlayer::new_cometd(mac, mac_str.clone(), uuid);
            // Only override the name if the handshake sent a real name;
            // new_cometd already applies friendly_name() lookup.
            if let Some(ref n) = ext_name {
                player.name = n.clone();
            }
            map.insert(mac, player);
        }
    }

    BayeuxResponse {
        channel: "/meta/handshake".to_string(),
        id: msg.id.clone(),
        version: Some("1.0".to_string()),
        client_id: Some(client_id),
        successful: true,
        supported_connection_types: Some(vec!["long-polling".to_string(), "streaming".to_string()]),
        advice: Some(BayeuxAdvice {
            reconnect: "retry".to_string(),
            interval: 0,
            timeout: 30000,
        }),
        data: None,
        ext: None,
        subscription: None,
    }
}

// ── Connect (long-poll) ──────────────────────────────────────────────────────

async fn handle_connect(state: &CometdState, msg: &BayeuxRequest) -> Vec<serde_json::Value> {
    let client_id = msg.client_id.clone().unwrap_or_default();
    let mut responses: Vec<serde_json::Value> = Vec::new();

    // Parse client-side timeout hint (advice.timeout).
    // If the client sends timeout:0, it means "respond immediately, don't hold".
    let client_timeout: u64 = msg.advice
        .as_ref()
        .and_then(|a| a.get("timeout"))
        .and_then(|t| t.as_u64())
        .unwrap_or(30000);

    let connect_response = || -> serde_json::Value {
        serde_json::to_value(BayeuxResponse {
            channel: "/meta/connect".to_string(),
            id: msg.id.clone(),
            version: None,
            client_id: Some(client_id.clone()),
            successful: true,
            supported_connection_types: None,
            advice: Some(BayeuxAdvice {
                reconnect: "retry".to_string(),
                interval: 0,
                timeout: 30000,
            }),
            data: None,
            ext: None,
            subscription: None,
        }).unwrap()
    };

    // Check if there are queued messages for this client
    let notify = {
        let mut clients = state.clients.lock().await;
        if let Some(client) = clients.get_mut(&client_id) {
            if !client.push_queue.is_empty() {
                // Drain queued messages immediately (raw serde_json::Value)
                let queued: Vec<serde_json::Value> = client.push_queue.drain(..).collect();
                responses.extend(queued);
                responses.push(connect_response());
                return responses;
            }
            Some(client.notify.clone())
        } else {
            None
        }
    };

    // If client requests timeout:0, return immediately (don't block other messages in batch)
    if client_timeout == 0 {
        tracing::info!("Cometd: connect with timeout:0 for clientId={}, returning immediately", client_id);
        responses.push(connect_response());
        return responses;
    }

    // Long-poll: wait for data or server timeout (use the lesser of client hint and 30s)
    let poll_secs = (client_timeout / 1000).min(30);
    if let Some(notify) = notify {
        tracing::info!("Cometd: connect long-poll started for clientId={} ({}s)", client_id, poll_secs);
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(poll_secs),
            notify.notified(),
        ).await;
        tracing::info!("Cometd: connect long-poll ended for clientId={}", client_id);

        // Check for queued messages after wait
        let mut clients = state.clients.lock().await;
        if let Some(client) = clients.get_mut(&client_id) {
            let queued: Vec<serde_json::Value> = client.push_queue.drain(..).collect();
            responses.extend(queued);
        }
    }

    // Always include the connect response
    responses.push(connect_response());
    responses
}

// ── Subscribe ────────────────────────────────────────────────────────────────

async fn handle_subscribe(state: &CometdState, msg: &BayeuxRequest) -> BayeuxResponse {
    let client_id = msg.client_id.clone().unwrap_or_default();
    let subscription = msg.subscription.clone().unwrap_or_default();

    tracing::info!("Cometd: subscribe clientId={} subscription={}", client_id, subscription);

    let mut clients = state.clients.lock().await;
    if let Some(client) = clients.get_mut(&client_id) {
        if !client.subscriptions.contains(&subscription) {
            client.subscriptions.push(subscription.clone());
        }
    }

    BayeuxResponse {
        channel: "/meta/subscribe".to_string(),
        id: msg.id.clone(),
        version: None,
        client_id: Some(client_id),
        successful: true,
        supported_connection_types: None,
        advice: None,
        data: None,
        ext: None,
        subscription: Some(subscription),
    }
}

// ── Slim Subscribe (LMS-specific) ────────────────────────────────────────────

async fn handle_slim_subscribe(state: &CometdState, msg: &BayeuxRequest) -> Vec<serde_json::Value> {
    let client_id = msg.client_id.clone().unwrap_or_default();
    let mut result = Vec::new();

    tracing::info!("Cometd: slim/subscribe clientId={} data={:?}", client_id, msg.data);

    if let Some(data) = &msg.data {
        let response_channel = data.get("response").and_then(|v| v.as_str()).unwrap_or("");
        if let Some(request) = data.get("request").and_then(|v| v.as_array()) {
            let player_id = request.first().and_then(|v| v.as_str()).unwrap_or("");
            if let Some(cmd_array) = request.get(1).and_then(|v| v.as_array()) {
                let cmd_name = cmd_array.first().and_then(|v| v.as_str()).unwrap_or("");

                match cmd_name {
                    "serverstatus" => {
                        if !response_channel.is_empty() {
                            let status_data = build_serverstatus(state).await;
                            result.push(serde_json::json!({
                                "channel": response_channel,
                                "data": status_data,
                            }));
                        }
                    }
                    "status" => {
                        // Track the subscription response channel for future pushes
                        if !player_id.is_empty() && !response_channel.is_empty() {
                            {
                                let mut clients = state.clients.lock().await;
                                if let Some(client) = clients.get_mut(&client_id) {
                                    client.status_channels.insert(player_id.to_string(), response_channel.to_string());
                                }
                            }
                            // Include initial status inline
                            let status_data = build_player_status(state, player_id).await;
                            result.push(serde_json::json!({
                                "channel": response_channel,
                                "data": status_data,
                            }));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // Ack comes after the data events
    result.push(serde_json::to_value(
        BayeuxResponse::success("/slim/subscribe", msg.id.clone(), Some(client_id))
    ).unwrap());

    result
}

// ── Slim Request (player → server commands) ──────────────────────────────────

async fn handle_slim_request(state: &CometdState, msg: &BayeuxRequest) -> Vec<serde_json::Value> {
    let client_id = msg.client_id.clone().unwrap_or_default();
    let mut result = Vec::new();

    tracing::info!("Cometd: slim/request clientId={} data={:?}", client_id, msg.data);

    if let Some(data) = &msg.data {
        let response_channel = data.get("response").and_then(|v| v.as_str()).unwrap_or("");

        if let Some(request) = data.get("request").and_then(|v| v.as_array()) {
            let player_id = request.first().and_then(|v| v.as_str()).unwrap_or("");
            tracing::info!("Cometd: slim/request raw request array ({} elements): {:?}", request.len(), request);
            if let Some(cmd_array) = request.get(1).and_then(|v| v.as_array()) {
                let cmd_name = cmd_array.first().and_then(|v| v.as_str()).unwrap_or("");
                tracing::info!("Cometd: slim request player={} cmd={} full_cmd={:?} response_channel={}", player_id, cmd_name, cmd_array, response_channel);

                match cmd_name {
                    "serverstatus" => {
                        if !response_channel.is_empty() {
                            let status_data = build_serverstatus(state).await;
                            result.push(serde_json::json!({
                                "channel": response_channel,
                                "id": msg.id,
                                "data": status_data,
                            }));
                        }
                    }
                    "status" => {
                        if !player_id.is_empty() && !response_channel.is_empty() {
                            let status_data = build_player_status(state, player_id).await;
                            result.push(serde_json::json!({
                                "channel": response_channel,
                                "id": msg.id,
                                "data": status_data,
                            }));
                        }
                    }
                    "mixer" => {
                        if !player_id.is_empty() {
                            let sub_cmd = cmd_array.get(1).and_then(|v| v.as_str()).unwrap_or("");
                            if sub_cmd == "volume" {
                                if let Some(vol_str) = cmd_array.get(2).and_then(|v| v.as_str()) {
                                    let mac = parse_mac_address(player_id);
                                    if vol_str == "?" {
                                        // Volume query — return just _volume
                                        let vol = {
                                            let players = state.players.lock().await;
                                            players.get(&mac).map(|p| p.volume).unwrap_or(50)
                                        };
                                        if !response_channel.is_empty() {
                                            result.push(serde_json::json!({
                                                "channel": response_channel,
                                                "id": msg.id,
                                                "data": { "_volume": vol },
                                            }));
                                        }
                                    } else {
                                        // Volume set (absolute or relative)
                                        let mut players = state.players.lock().await;
                                        if let Some(player) = players.get_mut(&mac) {
                                            let new_vol = if vol_str.starts_with('+') || vol_str.starts_with('-') {
                                                let delta: i16 = vol_str.parse().unwrap_or(0);
                                                (player.volume as i16 + delta).clamp(0, 100) as u8
                                            } else {
                                                vol_str.parse::<u8>().unwrap_or(player.volume).min(100)
                                            };
                                            player.volume = new_vol;
                                            tracing::info!("Cometd: mixer volume for {} → {}", player_id, new_vol);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    "play" => {
                        tracing::info!("Cometd: >>> PLAY handler entered for player={}", player_id);
                        if !player_id.is_empty() {
                            let mac = parse_mac_address(player_id);
                            let mut players = state.players.lock().await;
                            if let Some(player) = players.get_mut(&mac) {
                                tracing::info!("Cometd: play: current state={:?}", player.state);
                                if player.state == PlayerState::Paused {
                                    player.play_started_at = Some(Instant::now());
                                    player.state = PlayerState::Playing;
                                    let _ = player.resume().await;
                                }
                            }
                            drop(players);
                            state.notify_player_status(player_id).await;
                        }
                    }
                    "pause" => {
                        tracing::info!("Cometd: >>> PAUSE handler entered for player={}", player_id);
                        if !player_id.is_empty() {
                            let mac = parse_mac_address(player_id);
                            // LMS semantics: pause (no arg) = toggle, pause 0 = unpause, pause 1 = force pause
                            let sub_val = cmd_array.get(1).and_then(|v| {
                                v.as_str().map(|s| s.to_string())
                                    .or_else(|| v.as_u64().map(|n| n.to_string()))
                            });
                            tracing::info!("Cometd: pause sub_val={:?}", sub_val);
                            let mut players = state.players.lock().await;
                            if let Some(player) = players.get_mut(&mac) {
                                tracing::info!("Cometd: pause: current state={:?}, sub_val={:?}", player.state, sub_val);
                                match sub_val.as_deref() {
                                    Some("0") => {
                                        tracing::info!("Cometd: pause -> explicit UNPAUSE");
                                        // Explicit unpause
                                        if player.state == PlayerState::Paused {
                                            player.play_started_at = Some(Instant::now());
                                            player.state = PlayerState::Playing;
                                            let _ = player.resume().await;
                                        }
                                    }
                                    Some("1") => {
                                        tracing::info!("Cometd: pause -> explicit FORCE PAUSE");
                                        // Explicit pause
                                        if player.state == PlayerState::Playing {
                                            player.elapsed_ms = player.get_elapsed_ms();
                                            player.play_started_at = None;
                                            player.state = PlayerState::Paused;
                                            let _ = player.pause().await;
                                        }
                                    }
                                    _ => {
                                        tracing::info!("Cometd: pause -> TOGGLE (no arg)");
                                        // No argument or unknown = toggle
                                        if player.state == PlayerState::Playing {
                                            player.elapsed_ms = player.get_elapsed_ms();
                                            player.play_started_at = None;
                                            player.state = PlayerState::Paused;
                                            let _ = player.pause().await;
                                        } else if player.state == PlayerState::Paused {
                                            player.play_started_at = Some(Instant::now());
                                            player.state = PlayerState::Playing;
                                            let _ = player.resume().await;
                                        }
                                    }
                                }
                            }
                            drop(players);
                            state.notify_player_status(player_id).await;
                        }
                    }
                    "stop" => {
                        if !player_id.is_empty() {
                            let mac = parse_mac_address(player_id);
                            let mut players = state.players.lock().await;
                            if let Some(player) = players.get_mut(&mac) {
                                player.elapsed_ms = 0;
                                player.play_started_at = None;
                                player.state = PlayerState::Stopped;
                                let _ = player.stop().await;
                            }
                            drop(players);
                            state.notify_player_status(player_id).await;
                        }
                    }
                    "playlist" => {
                        tracing::info!("Cometd: >>> PLAYLIST handler entered for player={}", player_id);
                        if !player_id.is_empty() {
                            let sub_cmd = cmd_array.get(1).and_then(|v| v.as_str()).unwrap_or("");
                            let mac = parse_mac_address(player_id);
                            tracing::info!("Cometd: playlist sub_cmd={} raw_arg2={:?}", sub_cmd, cmd_array.get(2));
                            match sub_cmd {
                                "index" | "jump" => {
                                    // Handle both string ("+1") and numeric (1) values
                                    let idx_val = cmd_array.get(2);
                                    let idx_str = idx_val.and_then(|v| {
                                        v.as_str().map(|s| s.to_string())
                                            .or_else(|| v.as_i64().map(|n| {
                                                if n > 0 { format!("+{}", n) } else { n.to_string() }
                                            }))
                                    }).unwrap_or_else(|| "0".to_string());
                                    if idx_str == "+1" {
                                        cometd_skip_track(state, player_id, &mac, true).await;
                                    } else if idx_str == "-1" {
                                        cometd_skip_track(state, player_id, &mac, false).await;
                                    } else if let Ok(abs_idx) = idx_str.parse::<usize>() {
                                        cometd_jump_to_index(state, player_id, &mac, abs_idx).await;
                                    }
                                }
                                "repeat" => {
                                    let val = cmd_array.get(2).and_then(|v| v.as_str().or_else(|| v.as_u64().map(|_| ""))).unwrap_or("0");
                                    let repeat = match val {
                                        "1" => crate::squeeze::queue::RepeatMode::One,
                                        "2" => crate::squeeze::queue::RepeatMode::All,
                                        _ => crate::squeeze::queue::RepeatMode::Off,
                                    };
                                    let mut players = state.players.lock().await;
                                    if let Some(player) = players.get_mut(&mac) {
                                        player.queue.repeat = repeat;
                                    }
                                    drop(players);
                                    state.notify_player_status(player_id).await;
                                }
                                "shuffle" => {
                                    let val = cmd_array.get(2).and_then(|v| v.as_str().or_else(|| v.as_u64().map(|_| ""))).unwrap_or("0");
                                    let enabled = val != "0";
                                    let mut players = state.players.lock().await;
                                    if let Some(player) = players.get_mut(&mac) {
                                        player.queue.set_shuffle(enabled);
                                    }
                                    drop(players);
                                    state.notify_player_status(player_id).await;
                                }
                                _ => {
                                    tracing::info!("Cometd: unhandled playlist sub-command: {}", sub_cmd);
                                }
                            }
                        }
                    }
                    "button" => {
                        tracing::info!("Cometd: >>> BUTTON handler entered for player={}", player_id);
                        if !player_id.is_empty() {
                            let btn = cmd_array.get(1).and_then(|v| v.as_str()).unwrap_or("");
                            tracing::info!("Cometd: button name={}", btn);
                            let mac = parse_mac_address(player_id);
                            match btn {
                                "fwd" | "fwd.single" | "fwd.hold" => {
                                    cometd_skip_track(state, player_id, &mac, true).await;
                                }
                                "rew" | "rew.single" | "rew.hold" => {
                                    cometd_skip_track(state, player_id, &mac, false).await;
                                }
                                "pause" | "pause.single" => {
                                    let mut players = state.players.lock().await;
                                    if let Some(player) = players.get_mut(&mac) {
                                        if player.state == PlayerState::Playing {
                                            player.elapsed_ms = player.get_elapsed_ms();
                                            player.play_started_at = None;
                                            player.state = PlayerState::Paused;
                                            let _ = player.pause().await;
                                        } else if player.state == PlayerState::Paused {
                                            player.play_started_at = Some(Instant::now());
                                            player.state = PlayerState::Playing;
                                            let _ = player.resume().await;
                                        }
                                    }
                                    drop(players);
                                    state.notify_player_status(player_id).await;
                                }
                                "play" | "play.single" => {
                                    let mut players = state.players.lock().await;
                                    if let Some(player) = players.get_mut(&mac) {
                                        if player.state == PlayerState::Paused {
                                            player.play_started_at = Some(Instant::now());
                                            player.state = PlayerState::Playing;
                                            let _ = player.resume().await;
                                        }
                                    }
                                    drop(players);
                                    state.notify_player_status(player_id).await;
                                }
                                _ => {
                                    tracing::info!("Cometd: unhandled button: {}", btn);
                                }
                            }
                        }
                    }
                    "time" => {
                        // Seek command: time <seconds>
                        if !player_id.is_empty() {
                            if let Some(secs_val) = cmd_array.get(1) {
                                let secs: f64 = secs_val.as_f64()
                                    .or_else(|| secs_val.as_str().and_then(|s| s.parse().ok()))
                                    .unwrap_or(0.0);
                                let mac = parse_mac_address(player_id);
                                cometd_seek(state, player_id, &mac, secs).await;
                            }
                        }
                    }
                    "power" => {
                        // Power on/off — we treat power off as stop
                        if !player_id.is_empty() {
                            let val = cmd_array.get(1).and_then(|v| v.as_str().or_else(|| v.as_u64().map(|_| ""))).unwrap_or("1");
                            if val == "0" {
                                let mac = parse_mac_address(player_id);
                                let mut players = state.players.lock().await;
                                if let Some(player) = players.get_mut(&mac) {
                                    player.elapsed_ms = 0;
                                    player.play_started_at = None;
                                    player.state = PlayerState::Stopped;
                                    let _ = player.stop().await;
                                }
                                drop(players);
                                state.notify_player_status(player_id).await;
                            }
                        }
                    }
                    "name" => {
                        // Player name set/query: ["MAC", ["name", "?"]]
                        // or ["MAC", ["name", "New Name"]]
                        if !player_id.is_empty() {
                            let name_val = cmd_array.get(1).and_then(|v| v.as_str()).unwrap_or("?");
                            let mac = parse_mac_address(player_id);
                            if name_val != "?" && !name_val.is_empty() {
                                // Set player name only if no hardcoded friendly name
                                use crate::squeeze::player::friendly_name;
                                if friendly_name(&mac).is_none() {
                                    let mut players = state.players.lock().await;
                                    if let Some(player) = players.get_mut(&mac) {
                                        player.name = name_val.to_string();
                                        tracing::info!("Cometd: player {} name set to '{}'", player_id, name_val);
                                    }
                                }
                            }
                            // Respond with current name
                            if !response_channel.is_empty() {
                                let players = state.players.lock().await;
                                let current_name = players.get(&mac)
                                    .map(|p| p.name.clone())
                                    .unwrap_or_else(|| player_id.to_string());
                                result.push(serde_json::json!({
                                    "channel": response_channel,
                                    "id": msg.id,
                                    "data": { "_name": current_name },
                                }));
                            }
                        }
                    }
                    "playerpref" => {
                        // Player preferences: ["MAC", ["playerpref", "playername", "New Name"]]
                        if !player_id.is_empty() {
                            let pref_key = cmd_array.get(1).and_then(|v| v.as_str()).unwrap_or("");
                            if pref_key == "playername" {
                                if let Some(name_val) = cmd_array.get(2).and_then(|v| v.as_str()) {
                                    if !name_val.is_empty() && name_val != "?" {
                                        let mac = parse_mac_address(player_id);
                                        use crate::squeeze::player::friendly_name;
                                        if friendly_name(&mac).is_none() {
                                            let mut players = state.players.lock().await;
                                            if let Some(player) = players.get_mut(&mac) {
                                                player.name = name_val.to_string();
                                                tracing::info!("Cometd: player {} playername set to '{}'", player_id, name_val);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {
                        tracing::info!("Cometd: unhandled slim request cmd={}", cmd_name);
                    }
                }
            } else {
                // request[1] is not an array — maybe flat command format
                tracing::info!("Cometd: request[1] is NOT an array! raw={:?}, full request={:?}", request.get(1), request);
            }
        } else {
            tracing::info!("Cometd: no 'request' key in data. full data={:?}", msg.data);
        }
    } else {
        tracing::info!("Cometd: slim/request with no data field");
    }

    // Ack comes after the data events
    result.push(serde_json::to_value(
        BayeuxResponse::success(&msg.channel, msg.id.clone(), Some(client_id))
    ).unwrap());

    result
}

// ── Disconnect ───────────────────────────────────────────────────────────────

async fn handle_disconnect(state: &CometdState, msg: &BayeuxRequest) -> BayeuxResponse {
    let client_id = msg.client_id.clone().unwrap_or_default();
    tracing::info!("Cometd: disconnect clientId={}", client_id);

    // Remove client and MAC mapping
    let removed_client = {
        let mut clients = state.clients.lock().await;
        clients.remove(&client_id)
    };

    if let Some(client) = removed_client {
        // Wake up any pending long-poll
        client.notify.notify_one();

        // Remove MAC mapping
        let mut mac_map = state.mac_to_client.lock().await;
        mac_map.remove(&client.mac.to_string());
        drop(mac_map);

        // If this was a CometD-only registration (no TCP writer), drop
        // the player entry too. Otherwise leave it for the TCP path to
        // manage — when both transports share the same MAC the TCP
        // disconnect handler will do the cleanup.
        let mut players = state.players.lock().await;
        let is_cometd_only = players
            .get(&client.mac)
            .map(|p| !p.has_tcp_writer())
            .unwrap_or(false);
        if is_cometd_only {
            if let Some(p) = players.remove(&client.mac) {
                tracing::info!(
                    "Cometd: removed CometD-only player {} (\"{}\") on disconnect",
                    client.mac,
                    p.name
                );
            }
        }
    }

    BayeuxResponse {
        channel: "/meta/disconnect".to_string(),
        id: msg.id.clone(),
        version: None,
        client_id: Some(client_id),
        successful: true,
        supported_connection_types: None,
        advice: None,
        data: None,
        ext: None,
        subscription: None,
    }
}

// ── Server status builder ────────────────────────────────────────────────────

async fn build_serverstatus(state: &CometdState) -> serde_json::Value {
    let players = state.players.lock().await;
    let mut players_loop = Vec::new();

    for (mac, player) in players.iter() {
        if player.is_cometd {
            players_loop.push(serde_json::json!({
                "playerid": mac.to_string(),
                "uuid": "",
                "name": player.name.clone(),
                "connected": 1,
                "model": "squeezelite",
                "modelname": "SqueezeLite",
                "power": 1,
                "isplaying": 0,
                "canpoweroff": 0,
                "firmware": "0.0",
                "isplayer": 1,
                "displaytype": "none",
                "player_needs_upgrade": 0,
                "player_is_upgrading": 0,
            }));
        }
    }

    serde_json::json!({
        "version": "8.5.0",
        "uuid": "audion-squeeze-server-0001",
        "lastscan": 0,
        "rescan": 0,
        "progressname": "",
        "progressdone": "",
        "progresstotal": "",
        "player count": players_loop.len(),
        "players_loop": players_loop,
        "info total albums": 0,
        "info total artists": 0,
        "info total songs": 0,
        "info total genres": 0,
    })
}

pub async fn build_player_status(state: &CometdState, player_id: &str) -> serde_json::Value {
    let mac = parse_mac_address(player_id);
    let players = state.players.lock().await;

    let Some(player) = players.get(&mac) else {
        return serde_json::json!({
            "player_name": player_id,
            "player_connected": 0,
            "power": 0,
            "mode": "stop",
        });
    };

    let mode = match player.state {
        PlayerState::Playing => "play",
        PlayerState::Paused => "pause",
        _ => "stop",
    };

    let repeat = match player.queue.repeat {
        crate::squeeze::queue::RepeatMode::Off => 0,
        crate::squeeze::queue::RepeatMode::One => 1,
        crate::squeeze::queue::RepeatMode::All => 2,
    };

    let cur_track = player.display_track.as_ref().or_else(|| player.queue.current());
    let cur_index = cur_track
        .and_then(|track| player.queue.position_of_track_id(track.id))
        .or_else(|| player.queue.current_position())
        .unwrap_or(0);
    let duration = cur_track.map(|t| t.duration).unwrap_or(0.0);
    let elapsed = player.get_elapsed_ms() as f64 / 1000.0;

    // Build item_loop with current track info (JiveItem format for Squeezer)
    let mut item_loop = Vec::new();
    let mut playlist_loop = Vec::new();
    if let Some(track) = cur_track {
        // JiveItem format: "text" for display, "icon-id" for artwork
        let display_text = format!("{}\n{}", track.title, track.artist);
        item_loop.push(serde_json::json!({
            "playlist index": cur_index,
            "id": track.id,
            "text": display_text,
            "icon-id": track.id.to_string(),
            "icon": format!("/music/{}/cover.jpg", track.id),
            "track": track.title,
            "title": track.title,
            "artist": track.artist,
            "album": track.album,
            "duration": track.duration,
            "trackType": "local",
            "url": format!("/stream?player={}", player_id),
            "params": {
                "track_id": track.id,
            },
            "style": "itemplay",
            "artwork_track_id": track.id.to_string(),
            "coverid": track.id.to_string(),
            "artwork_url": format!("/music/{}/cover.jpg", track.id),
        }));
        // Also keep playlist_loop for explicit status requests
        playlist_loop.push(serde_json::json!({
            "playlist index": cur_index,
            "id": track.id,
            "title": track.title,
            "artist": track.artist,
            "album": track.album,
            "duration": track.duration,
            "trackType": track.format,
            "artwork_track_id": track.id.to_string(),
            "coverid": track.id.to_string(),
            "artwork_url": format!("/music/{}/cover.jpg", track.id),
            "url": format!("/stream?player={}", player_id),
        }));
    }

    // remoteMeta for current track
    let remote_meta = if let Some(track) = cur_track {
        serde_json::json!({
            "title": track.title,
            "artist": track.artist,
            "album": track.album,
            "duration": track.duration,
            "artwork_track_id": track.id.to_string(),
            "coverid": track.id.to_string(),
            "artwork_url": format!("/music/{}/cover.jpg", track.id),
            "url": format!("/stream?player={}", player_id),
        })
    } else {
        serde_json::json!({})
    };

    let (track_id, title, artist, album, track_type, artwork_url, stream_url) = cur_track
        .map(|track| {
            (
                track.id,
                track.title.clone(),
                track.artist.clone(),
                track.album.clone(),
                track.format.clone(),
                format!("/music/{}/cover.jpg", track.id),
                format!("/stream?player={}", player_id),
            )
        })
        .unwrap_or_else(|| {
            (
                0,
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
            )
        });
    let coverid = if track_id != 0 { track_id.to_string() } else { String::new() };

    serde_json::json!({
        "player_name": player.name,
        "player_connected": 1,
        "player_needs_upgrade": 0,
        "player_is_upgrading": 0,
        "power": 1,
        "mode": mode,
        "time": elapsed,
        "rate": 1,
        "duration": duration,
        "id": track_id,
        "track_id": track_id,
        "title": title,
        "track": title,
        "artist": artist,
        "album": album,
        "current_title": title,
        "trackType": track_type,
        "url": stream_url,
        "coverid": coverid,
        "artwork_track_id": coverid,
        "artwork_url": artwork_url,
        "mixer volume": player.volume,
        "playlist repeat": repeat,
        "playlist shuffle": if player.queue.shuffle { 1 } else { 0 },
        "playlist_cur_index": cur_index,
        "playlist_tracks": player.queue.len(),
        "item_loop": item_loop,
        "playlist_loop": playlist_loop,
        "seq_no": 0,
        "can_seek": if duration > 0.0 { 1 } else { 0 },
        "digital_volume_control": 1,
        "playlist mode": "off",
        "remoteMeta": remote_meta,
    })
}

/// Push a message to a client by clientId (wakes long-poll).
async fn push_to_client_by_id(state: &CometdState, client_id: &str, message: serde_json::Value) {
    let mut clients = state.clients.lock().await;
    if let Some(client) = clients.get_mut(client_id) {
        client.push_queue.push(message);
        client.notify.notify_one();
    }
}

/// Push player status to all clients subscribed to the given player.
async fn notify_player_status_inner(state: &CometdState, player_mac: &str) {
    let status_data = build_player_status(state, player_mac).await;

    // Log key fields for debugging
    tracing::info!(
        "Squeeze CometD: notify_player_status mac={} mode={} time={} duration={}",
        player_mac,
        status_data.get("mode").and_then(|v| v.as_str()).unwrap_or("?"),
        status_data.get("time").and_then(|v| v.as_f64()).unwrap_or(-1.0),
        status_data.get("duration").and_then(|v| v.as_f64()).unwrap_or(-1.0),
    );

    // Collect client_id → response_channel mappings
    let subscriptions: Vec<(String, String)> = {
        let clients = state.clients.lock().await;
        clients.values()
            .filter_map(|c| {
                c.status_channels.get(player_mac)
                    .map(|ch| (c.client_id.clone(), ch.clone()))
            })
            .collect()
    };

    for (client_id, response_channel) in subscriptions {
        let event = serde_json::json!({
            "channel": response_channel,
            "data": status_data,
        });
        push_to_client_by_id(state, &client_id, event).await;
    }
}

// ── Public API for pushing commands to players ───────────────────────────────

impl CometdState {
    /// Push a message to a player identified by MAC. Wakes the long-poll connection.
    pub async fn push_to_player(&self, mac_str: &str, message: serde_json::Value) {
        let client_id = {
            let map = self.mac_to_client.lock().await;
            map.get(mac_str).cloned()
        };

        if let Some(client_id) = client_id {
            let mut clients = self.clients.lock().await;
            if let Some(client) = clients.get_mut(&client_id) {
                client.push_queue.push(message);
                client.notify.notify_one();
            }
        }
    }

    /// Notify all subscribed CometD clients about a player status change.
    pub async fn notify_player_status(&self, mac_str: &str) {
        notify_player_status_inner(self, mac_str).await;
    }

    /// Get list of connected Cometd players (MACs).
    pub async fn connected_macs(&self) -> Vec<String> {
        let map = self.mac_to_client.lock().await;
        map.keys().cloned().collect()
    }
}

// ── Playback helpers for CometD commands ─────────────────────────────────────

/// Skip to next or previous track (called from CometD handlers).
async fn cometd_skip_track(state: &CometdState, player_id: &str, mac: &MacAddress, forward: bool) {
    // Stop current stream
    {
        let mut players = state.players.lock().await;
        if let Some(player) = players.get_mut(mac) {
            player.display_track = None;
            let _ = player.stop().await;
            let _ = player.flush().await;
            player.suppress_track_finished = true;
        }
    }

    // Advance or rewind queue
    let has_track = {
        let mut players = state.players.lock().await;
        if let Some(player) = players.get_mut(mac) {
            if forward {
                if player.prefetched_generation.is_some() {
                    player.prefetched_generation = None;
                    player.queue.current().is_some()
                } else {
                    player.queue.next().is_some()
                }
            } else {
                if player.prefetched_generation.is_some() {
                    player.queue.previous();
                    player.prefetched_generation = None;
                }
                player.queue.previous().is_some()
            }
        } else {
            false
        }
    };

    if !has_track {
        tracing::info!("Cometd: no {} track for {}", if forward { "next" } else { "previous" }, player_id);
        return;
    }

    cometd_start_current_track(state, player_id, mac).await;
}

/// Jump to an absolute queue index.
async fn cometd_jump_to_index(state: &CometdState, player_id: &str, mac: &MacAddress, index: usize) {
    // Stop current stream
    {
        let mut players = state.players.lock().await;
        if let Some(player) = players.get_mut(mac) {
            player.display_track = None;
            let _ = player.stop().await;
            let _ = player.flush().await;
            player.suppress_track_finished = true;
            player.prefetched_generation = None;
            player.queue.jump_to(index);
        }
    }

    cometd_start_current_track(state, player_id, mac).await;
}

/// Seek to a position in seconds.
async fn cometd_seek(state: &CometdState, player_id: &str, mac: &MacAddress, position_seconds: f64) {
    // Stop and flush
    {
        let mut players = state.players.lock().await;
        if let Some(player) = players.get_mut(mac) {
            player.suppress_track_finished = true;
            let _ = player.stop().await;
            let _ = player.flush().await;
        }
    }

    // Calculate byte offset
    let (path, duration) = {
        let players = state.players.lock().await;
        match players.get(mac).and_then(|p| p.queue.current().map(|t| (PathBuf::from(&t.path), t.duration))) {
            Some(v) => v,
            None => return,
        }
    };

    let byte_offset = if duration > 0.0 {
        let file_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        ((position_seconds / duration) * file_size as f64) as u64
    } else {
        0
    };

    let gen = {
        let mut players = state.players.lock().await;
        if let Some(player) = players.get_mut(mac) {
            player.generation += 1;
            player.seek_offset_ms = (position_seconds * 1000.0) as u32;
            player.generation
        } else {
            return;
        }
    };

    state.streaming.queue_file(mac, path, gen, byte_offset).await;

    {
        let mut players = state.players.lock().await;
        if let Some(player) = players.get_mut(mac) {
            let _ = player.start_stream(HTTP_PORT, 0).await;
            player.elapsed_ms = (position_seconds * 1000.0) as u32;
            player.play_started_at = Some(Instant::now());
            player.state = PlayerState::Playing;
        }
    }

    state.notify_player_status(player_id).await;
}

/// Start streaming the current track in a player's queue.
async fn cometd_start_current_track(state: &CometdState, player_id: &str, mac: &MacAddress) {
    let path = {
        let players = state.players.lock().await;
        match players.get(mac).and_then(|p| p.queue.current().map(|t| PathBuf::from(&t.path))) {
            Some(p) => p,
            None => return,
        }
    };

    let gen = {
        let mut players = state.players.lock().await;
        if let Some(player) = players.get_mut(mac) {
            player.generation += 1;
            player.generation
        } else {
            return;
        }
    };

    state.streaming.queue_file(mac, path, gen, 0).await;

    {
        let mut players = state.players.lock().await;
        if let Some(player) = players.get_mut(mac) {
            player.seek_offset_ms = 0;
            player.elapsed_ms = 0;
            player.play_started_at = Some(Instant::now());
            let _ = player.start_stream(HTTP_PORT, 0).await;
            player.state = PlayerState::Playing;
        }
    }

    state.notify_player_status(player_id).await;
}

// ── Helper ───────────────────────────────────────────────────────────────────

pub fn parse_mac_address(mac_str: &str) -> MacAddress {
    let parts: Vec<&str> = mac_str.split(':').collect();
    if parts.len() != 6 {
        return MacAddress([0; 6]);
    }
    let mut bytes = [0u8; 6];
    for (i, part) in parts.iter().enumerate() {
        bytes[i] = u8::from_str_radix(part, 16).unwrap_or(0);
    }
    MacAddress(bytes)
}

// ── Watchdog ─────────────────────────────────────────────────────────────────

/// How often the watchdog scans for stale clients.
const WATCHDOG_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);
/// A CometD client that hasn't been seen for longer than this is
/// considered dead. Must be > the longest expected long-poll timeout
/// (currently 30 s) plus some slack so a slow client doesn't get
/// evicted during a normal idle period.
const WATCHDOG_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);

/// Background task that periodically evicts CometD clients whose HTTP
/// connection has died without sending a /meta/disconnect, and removes
/// their (CometD-only) player entries from the shared player map. This
/// is what stops "zombie" Player entries from accumulating forever when
/// the Eversolo's WebUI tab is closed abruptly or the network drops.
pub async fn run_watchdog(state: CometdState, shutdown: Arc<AtomicBool>) {
    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        tokio::time::sleep(WATCHDOG_INTERVAL).await;
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        let now = Instant::now();
        // Snapshot stale clientIds while only holding the clients lock.
        let stale: Vec<(String, MacAddress)> = {
            let clients = state.clients.lock().await;
            clients
                .iter()
                .filter(|(_, c)| now.duration_since(c.last_seen) > WATCHDOG_TIMEOUT)
                .map(|(id, c)| (id.clone(), c.mac))
                .collect()
        };

        if stale.is_empty() {
            continue;
        }

        let mut clients = state.clients.lock().await;
        let mut mac_map = state.mac_to_client.lock().await;
        let mut players = state.players.lock().await;

        for (client_id, mac) in stale {
            if let Some(client) = clients.remove(&client_id) {
                mac_map.remove(&client.mac.to_string());
                // Only drop the player entry if it's a CometD-only
                // registration (no TCP writer). If TCP is also active
                // for this MAC, the TCP path owns the player lifecycle.
                let is_cometd_only = players
                    .get(&mac)
                    .map(|p| !p.has_tcp_writer())
                    .unwrap_or(false);
                if is_cometd_only {
                    if let Some(p) = players.remove(&mac) {
                        tracing::info!(
                            "Cometd watchdog: evicted stale player {} (\"{}\")",
                            mac,
                            p.name
                        );
                    }
                } else {
                    tracing::info!(
                        "Cometd watchdog: evicted stale client {} for {}, kept player (has TCP writer)",
                        client_id,
                        mac
                    );
                }
            }
        }
    }
    tracing::info!("Cometd watchdog stopped");
}
