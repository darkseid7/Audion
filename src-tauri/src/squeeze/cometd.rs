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
use crate::squeeze::queue::QueueTrack;
use axum::extract::State as AxumState;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
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
    /// Counter for generating client IDs.
    counter: Arc<Mutex<u64>>,
}

impl CometdState {
    pub fn new(players: PlayerMap) -> Self {
        Self {
            clients: Arc::new(Mutex::new(HashMap::new())),
            mac_to_client: Arc::new(Mutex::new(HashMap::new())),
            players,
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

    // Extract MAC and UUID from ext
    let (mac_str, uuid) = if let Some(ext) = &msg.ext {
        let mac = ext.get("mac").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let uuid = ext.get("uuid").and_then(|v| v.as_str()).unwrap_or("").to_string();
        (mac, uuid)
    } else {
        (String::new(), String::new())
    };

    // Parse MAC
    let mac = parse_mac_address(&mac_str);

    tracing::info!(
        "Cometd: handshake from MAC={} UUID={} → clientId={}",
        mac_str, uuid, client_id
    );

    // Register the client
    let client = CometdClient {
        client_id: client_id.clone(),
        mac,
        uuid: uuid.clone(),
        name: format!("Squeeze Player {}", mac_str),
        subscriptions: Vec::new(),
        status_channels: HashMap::new(),
        push_queue: Vec::new(),
        notify: Arc::new(Notify::new()),
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
    {
        use crate::squeeze::player::SqueezePlayer;
        let player = SqueezePlayer::new_cometd(mac, mac_str.clone(), uuid);
        let mut map = state.players.lock().await;
        map.insert(mac, player);
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
            if let Some(cmd_array) = request.get(1).and_then(|v| v.as_array()) {
                let cmd_name = cmd_array.first().and_then(|v| v.as_str()).unwrap_or("");
                tracing::info!("Cometd: slim request player={} cmd={} response_channel={}", player_id, cmd_name, response_channel);

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
                    _ => {
                        tracing::info!("Cometd: unhandled slim request cmd={}", cmd_name);
                    }
                }
            }
        }
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
    let mac_str = {
        let mut clients = state.clients.lock().await;
        if let Some(client) = clients.remove(&client_id) {
            // Wake up any pending long-poll
            client.notify.notify_one();
            Some(client.mac.to_string())
        } else {
            None
        }
    };

    if let Some(mac_str) = mac_str {
        let mut mac_map = state.mac_to_client.lock().await;
        mac_map.remove(&mac_str);
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

async fn build_player_status(state: &CometdState, player_id: &str) -> serde_json::Value {
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

    let cur_index = player.queue.current_position().unwrap_or(0);
    let cur_track = player.queue.current();
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
            "track": track.title,
            "artist": track.artist,
            "album": track.album,
            "duration": track.duration,
            "trackType": "local",
            "params": {
                "track_id": track.id,
            },
            "style": "itemplay",
            "artwork_track_id": track.id.to_string(),
            "coverid": track.id.to_string(),
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
        })
    } else {
        serde_json::json!({})
    };

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

// ── Helper ───────────────────────────────────────────────────────────────────

fn parse_mac_address(mac_str: &str) -> MacAddress {
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
