// =============================================================================
// WEB UI
// =============================================================================
// Serves a self-contained Now Playing page at `/` and exposes a tiny REST
// API for control commands. The page is what the Eversolo's "Squeeze" tab
// shows — a small WebView that loads whatever the server returns at the
// root. Without this it would just see the fallback's "OK" string.
//
// We deliberately do NOT try to serve the LMS Default skin — that would
// require implementing a large subset of the LMS JSON-RPC API. Instead
// we render a focused, dark, single-page Now Playing UI that uses
// CometD for live updates and the /api/control/* endpoints for commands.
// =============================================================================

use crate::squeeze::player::PlayerState;
use crate::squeeze::server::{control_next, control_previous};
use crate::squeeze::streaming::HttpState;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

/// Re-export HttpState as the state type for the webui router so both
/// routers share the same state and can be merged.
pub type WebUiState = HttpState;

/// Returns the self-contained Now Playing page. Embedded as a string so we
/// don't need to ship a static asset directory. Sends no-cache headers so
/// the Eversolo's WebView never serves a stale copy after an app update.
pub async fn now_playing_page() -> impl IntoResponse {
    (
        [
            (axum::http::header::CACHE_CONTROL, "no-store, no-cache, must-revalidate"),
            (axum::http::header::PRAGMA, "no-cache"),
            (axum::http::header::EXPIRES, "0"),
        ],
        axum::response::Html(NOW_PLAYING_HTML),
    )
}

/// Router exposing the Now Playing page and the `/api/*` control endpoints.
/// Merged into the main HTTP router by `start_streaming_server_with_listener`.
pub fn webui_router(state: WebUiState) -> axum::Router {
    use axum::routing::{get, post};

    axum::Router::new()
        .route("/", get(now_playing_page))
        .route("/api/players", get(list_players))
        .route("/api/status", get(player_status))
        .route("/api/control/play", post(player_play))
        .route("/api/control/pause", post(player_pause))
        .route("/api/control/toggle", post(player_toggle))
        .route("/api/control/stop", post(player_stop))
        .route("/api/control/next", post(player_next))
        .route("/api/control/previous", post(player_previous))
        .route("/api/control/volume", post(player_volume))
        .with_state(state)
}

// -----------------------------------------------------------------------------
// Control API
// -----------------------------------------------------------------------------
//
// All endpoints accept an optional `mac` query param to target a specific
// player. If omitted, we use the first connected player (the typical case:
// the Eversolo is the only Squeeze client).
//
//   POST /api/control/play       → resume from pause
//   POST /api/control/pause      → pause
//   POST /api/control/toggle     → play/pause toggle
//   POST /api/control/next       → next track
//   POST /api/control/previous   → previous track
//   POST /api/control/stop       → stop
//   POST /api/control/volume?v=N → set volume 0..100
//   GET  /api/players            → list connected players (for the UI)
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
pub struct MacQuery {
    #[serde(default)]
    pub mac: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct VolumeQuery {
    #[serde(default)]
    pub mac: Option<String>,
    pub v: u8,
}

/// Pick the player MAC to operate on. Prefers an explicit `mac` query param;
/// otherwise returns the first connected player we can find. Returns `None`
/// if no players are connected.
async fn pick_player(state: &WebUiState, explicit: Option<String>) -> Option<String> {
    if let Some(mac) = explicit {
        return Some(mac);
    }
    let players = state.cometd.players.lock().await;
    players.keys().next().map(|m| m.to_string())
}

async fn parse_mac(s: &str) -> Option<crate::squeeze::codec::MacAddress> {
    let m = crate::squeeze::cometd::parse_mac_address(s);
    // parse_mac_address falls back to all-zeros on parse failure, which
    // would silently match a real (bogus) player. Detect that here.
    if m.0 == [0; 6] && s != "00:00:00:00:00:00" {
        return None;
    }
    Some(m)
}

async fn player_play(
    State(state): State<WebUiState>,
    Query(q): Query<MacQuery>,
) -> impl IntoResponse {
    let mac = match pick_player(&state, q.mac).await {
        Some(m) => m,
        None => return Err((StatusCode::NOT_FOUND, "No player connected")),
    };
    let mac_addr = match parse_mac(&mac).await {
        Some(m) => m,
        None => return Err((StatusCode::BAD_REQUEST, "Invalid MAC")),
    };
    let mut map = state.cometd.players.lock().await;
    if let Some(p) = map.get_mut(&mac_addr) {
        let _ = p.resume().await;
        Ok(Json(serde_json::json!({"ok": true, "action": "play"})))
    } else {
        Err((StatusCode::NOT_FOUND, "Player not found"))
    }
}

async fn player_pause(
    State(state): State<WebUiState>,
    Query(q): Query<MacQuery>,
) -> impl IntoResponse {
    let mac = match pick_player(&state, q.mac).await {
        Some(m) => m,
        None => return Err((StatusCode::NOT_FOUND, "No player connected")),
    };
    let mac_addr = match parse_mac(&mac).await {
        Some(m) => m,
        None => return Err((StatusCode::BAD_REQUEST, "Invalid MAC")),
    };
    let mut map = state.cometd.players.lock().await;
    if let Some(p) = map.get_mut(&mac_addr) {
        p.elapsed_ms = p.get_elapsed_ms();
        p.play_started_at = None;
        p.state = PlayerState::Paused;
        let _ = p.pause().await;
        Ok(Json(serde_json::json!({"ok": true, "action": "pause"})))
    } else {
        Err((StatusCode::NOT_FOUND, "Player not found"))
    }
}

async fn player_toggle(
    State(state): State<WebUiState>,
    Query(q): Query<MacQuery>,
) -> impl IntoResponse {
    let mac = match pick_player(&state, q.mac).await {
        Some(m) => m,
        None => return Err((StatusCode::NOT_FOUND, "No player connected")),
    };
    let mac_addr = match parse_mac(&mac).await {
        Some(m) => m,
        None => return Err((StatusCode::BAD_REQUEST, "Invalid MAC")),
    };
    let mut map = state.cometd.players.lock().await;
    if let Some(p) = map.get_mut(&mac_addr) {
        match p.state {
            PlayerState::Playing => {
                p.elapsed_ms = p.get_elapsed_ms();
                p.play_started_at = None;
                p.state = PlayerState::Paused;
                let _ = p.pause().await;
            }
            PlayerState::Paused => {
                p.play_started_at = Some(std::time::Instant::now());
                p.state = PlayerState::Playing;
                let _ = p.resume().await;
            }
            _ => {
                // Stopped: no current track to resume from the server side
                return Ok(Json(serde_json::json!({
                    "ok": true,
                    "action": "toggle",
                    "note": "player stopped, nothing to resume"
                })));
            }
        }
        Ok(Json(serde_json::json!({"ok": true, "action": "toggle"})))
    } else {
        Err((StatusCode::NOT_FOUND, "Player not found"))
    }
}

async fn player_stop(
    State(state): State<WebUiState>,
    Query(q): Query<MacQuery>,
) -> impl IntoResponse {
    let mac = match pick_player(&state, q.mac).await {
        Some(m) => m,
        None => return Err((StatusCode::NOT_FOUND, "No player connected")),
    };
    let mac_addr = match parse_mac(&mac).await {
        Some(m) => m,
        None => return Err((StatusCode::BAD_REQUEST, "Invalid MAC")),
    };
    let mut map = state.cometd.players.lock().await;
    if let Some(p) = map.get_mut(&mac_addr) {
        let _ = p.stop().await;
        Ok(Json(serde_json::json!({"ok": true, "action": "stop"})))
    } else {
        Err((StatusCode::NOT_FOUND, "Player not found"))
    }
}

async fn player_next(
    State(state): State<WebUiState>,
    Query(q): Query<MacQuery>,
) -> impl IntoResponse {
    let mac = match pick_player(&state, q.mac).await {
        Some(m) => m,
        None => return Err((StatusCode::NOT_FOUND, "No player connected")),
    };
    let mac_addr = match parse_mac(&mac).await {
        Some(m) => m,
        None => return Err((StatusCode::BAD_REQUEST, "Invalid MAC")),
    };
    let mac_str = mac_addr.to_string();
    control_next(&mac_addr, &state.cometd.players, &state.streaming).await;
    state.cometd.notify_player_status(&mac_str).await;
    Ok(Json(serde_json::json!({"ok": true, "action": "next"})))
}

async fn player_previous(
    State(state): State<WebUiState>,
    Query(q): Query<MacQuery>,
) -> impl IntoResponse {
    let mac = match pick_player(&state, q.mac).await {
        Some(m) => m,
        None => return Err((StatusCode::NOT_FOUND, "No player connected")),
    };
    let mac_addr = match parse_mac(&mac).await {
        Some(m) => m,
        None => return Err((StatusCode::BAD_REQUEST, "Invalid MAC")),
    };
    let mac_str = mac_addr.to_string();
    control_previous(&mac_addr, &state.cometd.players, &state.streaming).await;
    state.cometd.notify_player_status(&mac_str).await;
    Ok(Json(serde_json::json!({"ok": true, "action": "previous"})))
}

async fn player_volume(
    State(state): State<WebUiState>,
    Query(q): Query<VolumeQuery>,
) -> impl IntoResponse {
    let mac = match pick_player(&state, q.mac).await {
        Some(m) => m,
        None => return Err((StatusCode::NOT_FOUND, "No player connected")),
    };
    let mac_addr = match parse_mac(&mac).await {
        Some(m) => m,
        None => return Err((StatusCode::BAD_REQUEST, "Invalid MAC")),
    };
    let v = q.v.min(100);
    let mut map = state.cometd.players.lock().await;
    if let Some(p) = map.get_mut(&mac_addr) {
        let _ = p.set_volume(v).await;
        Ok(Json(serde_json::json!({"ok": true, "action": "volume", "v": v})))
    } else {
        Err((StatusCode::NOT_FOUND, "Player not found"))
    }
}

/// Snapshot of all connected players for the UI's player selector.
async fn list_players(State(state): State<WebUiState>) -> impl IntoResponse {
    let players = state.cometd.players.lock().await;
    let mut out: Vec<serde_json::Value> = Vec::new();
    for (mac, p) in players.iter() {
        let mode = match p.state {
            PlayerState::Playing => "play",
            PlayerState::Paused => "pause",
            _ => "stop",
        };
        out.push(serde_json::json!({
            "mac": mac.to_string(),
            "name": p.name,
            "mode": mode,
            "volume": p.volume,
        }));
    }
    Json(serde_json::json!({"players": out}))
}

/// GET /api/status[?mac=...] — single-shot full player status for the page
/// to use on first paint (before the CometD subscription is established).
async fn player_status(
    State(state): State<WebUiState>,
    Query(q): Query<MacQuery>,
) -> impl IntoResponse {
    let mac = match pick_player(&state, q.mac).await {
        Some(m) => m,
        None => return Err((StatusCode::NOT_FOUND, "No player connected")),
    };
    let status =
        crate::squeeze::cometd::build_player_status(&state.cometd, &mac).await;
    Ok(Json(status))
}

// -----------------------------------------------------------------------------
// Page source
// -----------------------------------------------------------------------------

const NOW_PLAYING_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8" />
<meta name="viewport" content="width=device-width, initial-scale=1.0" />
<title>Audion</title>
<style>
  :root {
    --bg: #121212;
    --surface: #181818;
    --surface-2: #282828;
    --text: #ffffff;
    --text-2: #b3b3b3;
    --text-3: #6a6a6a;
    --accent: #1DB954;
    --accent-2: #1ed760;
    --border: rgba(255, 255, 255, 0.08);
  }
  * { box-sizing: border-box; margin: 0; padding: 0; }
  html, body { height: 100%; }
  body {
    background: var(--bg);
    color: var(--text);
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto,
                 "Helvetica Neue", Arial, sans-serif;
    -webkit-font-smoothing: antialiased;
    overflow: hidden;
    user-select: none;
    -webkit-user-select: none;
  }
  .app {
    height: 100vh;
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 24px 20px 28px;
    gap: 16px;
  }
  .brand {
    font-size: 11px;
    letter-spacing: 3px;
    text-transform: uppercase;
    color: var(--accent);
    font-weight: 600;
    opacity: 0.85;
  }
  .art-wrap {
    position: relative;
    width: min(72vw, 320px);
    aspect-ratio: 1 / 1;
    border-radius: 12px;
    overflow: hidden;
    background: var(--surface-2);
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.55),
                0 0 0 1px var(--border);
    flex-shrink: 0;
  }
  .art {
    width: 100%; height: 100%;
    object-fit: cover;
    display: block;
  }
  .art-fallback {
    position: absolute; inset: 0;
    display: flex; align-items: center; justify-content: center;
    color: var(--text-3);
    background: linear-gradient(135deg, #1f1f1f, #2a2a2a);
  }
  .art-fallback svg { width: 38%; height: 38%; opacity: 0.45; }

  .info {
    width: 100%;
    max-width: 420px;
    text-align: center;
    min-height: 64px;
  }
  .title {
    font-size: 19px;
    font-weight: 700;
    line-height: 1.25;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .artist {
    font-size: 14px;
    color: var(--text-2);
    margin-top: 4px;
    line-height: 1.3;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .album {
    font-size: 12px;
    color: var(--text-3);
    margin-top: 2px;
    line-height: 1.3;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .empty { color: var(--text-3); font-size: 14px; padding: 12px 0; }

  .progress {
    width: 100%;
    max-width: 420px;
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 11px;
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
  }
  .bar {
    flex: 1;
    height: 4px;
    background: var(--surface-2);
    border-radius: 2px;
    overflow: hidden;
    position: relative;
  }
  .bar-fill {
    position: absolute; top: 0; left: 0; bottom: 0;
    width: 0%;
    background: var(--accent);
    border-radius: 2px;
    transition: width 0.4s linear;
  }

  .controls {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 22px;
  }
  .btn {
    background: none;
    border: 0;
    color: var(--text);
    cursor: pointer;
    padding: 8px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: background 0.15s, transform 0.1s, color 0.15s;
  }
  .btn:active { transform: scale(0.92); }
  .btn svg { width: 26px; height: 26px; fill: currentColor; }
  .btn.side { color: var(--text-2); }
  .btn.side:active { color: var(--text); }
  .btn.play {
    background: var(--text);
    color: #000;
    width: 64px; height: 64px;
    box-shadow: 0 6px 20px rgba(0, 0, 0, 0.45);
  }
  .btn.play svg { width: 30px; height: 30px; }
  .btn.play:hover { background: #fff; transform: scale(1.04); }
  .btn.play:active { transform: scale(0.96); }
  .btn[disabled] { opacity: 0.35; cursor: not-allowed; }
  .btn[disabled]:active { transform: none; }

  .volume {
    width: 100%;
    max-width: 420px;
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--text-2);
    margin-top: 4px;
  }
  .volume svg { width: 16px; height: 16px; fill: currentColor; flex-shrink: 0; }
  .vbar {
    flex: 1;
    height: 4px;
    background: var(--surface-2);
    border-radius: 2px;
    position: relative;
    cursor: pointer;
  }
  .vbar-fill {
    position: absolute; top: 0; left: 0; bottom: 0;
    background: var(--text);
    border-radius: 2px;
    width: 80%;
  }

  .meta {
    font-size: 10px;
    color: var(--text-3);
    letter-spacing: 1px;
    text-transform: uppercase;
    margin-top: auto;
    text-align: center;
  }
  .pulse {
    display: inline-block;
    width: 6px; height: 6px;
    border-radius: 50%;
    background: var(--accent);
    margin-right: 6px;
    box-shadow: 0 0 8px var(--accent);
    animation: pulse 1.6s ease-in-out infinite;
  }
  @keyframes pulse {
    0%, 100% { opacity: 0.4; }
    50% { opacity: 1; }
  }
  .connected .pulse { animation: none; opacity: 1; }
  .disconnected { color: var(--text-3); }
  .disconnected .pulse {
    background: var(--text-3);
    box-shadow: none;
    animation: none;
    opacity: 0.6;
  }

  .status {
    font-size: 11px;
    color: var(--text-2);
    text-align: center;
    min-height: 14px;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
    opacity: 0.85;
    margin-top: 2px;
  }
  .status.err { color: var(--error-color, #f15e6c); }
  .status.ok { color: var(--accent); }

  .btn.flash { animation: btnflash 0.35s ease-out; }
  @keyframes btnflash {
    0%   { background: rgba(29, 185, 84, 0.35); }
    100% { background: transparent; }
  }
  .btn.play.flash { background: rgba(29, 185, 84, 0.5); }
</style>
</head>
<body>
<div class="app" id="app">
  <div class="brand" id="brand">Audion</div>

  <div class="art-wrap">
    <img id="art" class="art" alt="" style="display:none" />
    <div id="art-fallback" class="art-fallback">
      <svg viewBox="0 0 24 24"><path d="M12 3v10.55A4 4 0 1 0 14 17V7h4V3z"/></svg>
    </div>
  </div>

  <div class="info" id="info">
    <div class="empty" id="empty">Sin reproducción activa</div>
  </div>

  <div class="progress">
    <span id="elapsed">0:00</span>
    <div class="bar"><div class="bar-fill" id="bar-fill"></div></div>
    <span id="total">0:00</span>
  </div>

  <div class="controls">
    <button class="btn side" id="btn-prev" title="Anterior" aria-label="Anterior">
      <svg viewBox="0 0 24 24"><path d="M6 6h2v12H6zM9.5 12l8.5 6V6z"/></svg>
    </button>
    <button class="btn play" id="btn-play" title="Reproducir/Pausa" aria-label="Reproducir/Pausa">
      <svg id="icon-play" viewBox="0 0 24 24"><path d="M8 5v14l11-7z"/></svg>
      <svg id="icon-pause" viewBox="0 0 24 24" style="display:none"><path d="M6 5h4v14H6zM14 5h4v14h-4z"/></svg>
    </button>
    <button class="btn side" id="btn-next" title="Siguiente" aria-label="Siguiente">
      <svg viewBox="0 0 24 24"><path d="M16 6h2v12h-2zM6 6v12l8.5-6z"/></svg>
    </button>
  </div>

  <div class="volume">
    <svg viewBox="0 0 24 24"><path d="M3 9v6h4l5 5V4L7 9H3z"/></svg>
    <div class="vbar" id="vbar"><div class="vbar-fill" id="vbar-fill"></div></div>
  </div>

  <div class="meta disconnected" id="meta">
    <span class="pulse"></span><span id="meta-text">Conectando…</span>
  </div>

  <div class="status" id="status"></div>
</div>

<script>
(() => {
  const $ = (id) => document.getElementById(id);
  const art = $('art'), artFallback = $('art-fallback');
  const info = $('info'), emptyEl = $('empty');
  const elapsedEl = $('elapsed'), totalEl = $('total'), barFill = $('bar-fill');
  const btnPlay = $('btn-play'), btnPrev = $('btn-prev'), btnNext = $('btn-next');
  const iconPlay = $('icon-play'), iconPause = $('icon-pause');
  const vbar = $('vbar'), vbarFill = $('vbar-fill');
  const meta = $('meta'), metaText = $('meta-text');
  const app = $('app');

  let currentMac = null;
  let currentArtwork = null;
  let lastElapsed = 0, lastElapsedAt = 0, lastDuration = 0, lastMode = 'stop';

  const fmt = (s) => {
    if (!s || !isFinite(s) || s < 0) s = 0;
    s = Math.floor(s);
    const m = Math.floor(s / 60), r = s % 60;
    return m + ':' + (r < 10 ? '0' : '') + r;
  };

  function setMeta(state) {
    meta.classList.remove('connected', 'disconnected');
    meta.classList.add(state);
    if (state === 'connected') metaText.textContent = 'En vivo';
    else if (state === 'connecting') metaText.textContent = 'Conectando…';
    else metaText.textContent = 'Desconectado';
  }

  function applyArtwork(url) {
    if (url === currentArtwork) return;
    currentArtwork = url;
    if (url) {
      art.src = url;
      art.style.display = '';
      artFallback.style.display = 'none';
    } else {
      art.removeAttribute('src');
      art.style.display = 'none';
      artFallback.style.display = '';
    }
  }

  function applyTrack(t) {
    if (!t || (!t.title && !t.artist)) {
      info.innerHTML = '<div class="empty">Sin reproducción activa</div>';
      applyArtwork(null);
      return;
    }
    const title = t.title || '';
    const artist = t.artist || '';
    const album = t.album || '';
    info.innerHTML =
      '<div class="title">' + esc(title) + '</div>' +
      (artist ? '<div class="artist">' + esc(artist) + '</div>' : '') +
      (album ? '<div class="album">' + esc(album) + '</div>' : '');
    applyArtwork(t.artwork_url || null);
  }

  function esc(s) {
    return String(s).replace(/[&<>"']/g, (c) => ({
      '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;'
    })[c]);
  }

  function applyStatus(d) {
    if (!d) return;
    const mode = d.mode || 'stop';
    const duration = Number(d.duration) || 0;
    const elapsed = Number(d.time) || 0;
    lastMode = mode;
    lastDuration = duration;
    lastElapsed = elapsed;
    lastElapsedAt = performance.now();
    if (d.player_connected !== 1) {
      applyTrack(null);
      setMode('stop', false);
      totalEl.textContent = '0:00';
      elapsedEl.textContent = '0:00';
      barFill.style.width = '0%';
      return;
    }
    applyTrack(d);
    setMode(mode, !!d.can_seek);
    totalEl.textContent = fmt(duration);
    elapsedEl.textContent = fmt(elapsed);
    const pct = duration > 0 ? Math.min(100, (elapsed / duration) * 100) : 0;
    barFill.style.width = pct + '%';
    if (typeof d['mixer volume'] === 'number') {
      const v = Math.max(0, Math.min(100, d['mixer volume']));
      vbarFill.style.width = v + '%';
    }
  }

  function setMode(mode, canSeek) {
    if (mode === 'play') {
      iconPlay.style.display = 'none';
      iconPause.style.display = '';
    } else {
      iconPlay.style.display = '';
      iconPause.style.display = 'none';
    }
    const interactive = mode !== 'stop' || canSeek;
    btnPlay.disabled = !interactive;
    btnPrev.disabled = false;
    btnNext.disabled = false;
  }

  // Smooth progress: interpolate between CometD updates using wall clock.
  function tickProgress() {
    if (lastMode === 'play' && lastDuration > 0) {
      const dt = (performance.now() - lastElapsedAt) / 1000;
      const e = Math.min(lastDuration, lastElapsed + dt);
      elapsedEl.textContent = fmt(e);
      const pct = (e / lastDuration) * 100;
      barFill.style.width = pct + '%';
    }
    requestAnimationFrame(tickProgress);
  }
  requestAnimationFrame(tickProgress);

  // ── Control API ──────────────────────────────────────────────────────
  const statusEl = $('status');
  let statusTimer = null;
  function showStatus(text, kind) {
    if (!statusEl) return;
    statusEl.textContent = text;
    statusEl.classList.remove('ok', 'err');
    if (kind) statusEl.classList.add(kind);
    if (statusTimer) clearTimeout(statusTimer);
    if (kind) {
      statusTimer = setTimeout(() => {
        statusEl.textContent = '';
        statusEl.classList.remove('ok', 'err');
      }, 2500);
    }
  }

  function flashBtn(btn) {
    btn.classList.remove('flash');
    // Force reflow so the animation re-triggers on rapid presses.
    void btn.offsetWidth;
    btn.classList.add('flash');
    setTimeout(() => btn.classList.remove('flash'), 400);
  }

  async function control(action, params, srcBtn) {
    const qs = new URLSearchParams();
    if (currentMac) qs.set('mac', currentMac);
    if (params) Object.entries(params).forEach(([k, v]) => qs.set(k, v));
    const url = '/api/control/' + action + '?' + qs.toString();
    showStatus('→ ' + action + (currentMac ? '' : ' (auto)'), '');
    try {
      const r = await fetch(url, { method: 'POST', cache: 'no-store' });
      if (!r.ok) {
        showStatus('✗ ' + action + ' ' + r.status, 'err');
        return;
      }
      showStatus('✓ ' + action, 'ok');
      // Force an immediate re-fetch so the UI updates without waiting
      // for the next CometD push — helps on slower WebViews.
      try {
        const sr = await fetch('/api/status', { cache: 'no-store' });
        if (sr.ok) applyStatus(await sr.json());
      } catch {}
    } catch (e) {
      showStatus('✗ ' + action + ': ' + (e && e.message ? e.message : e), 'err');
    }
  }

  btnPlay.addEventListener('click', (e) => {
    e.preventDefault();
    flashBtn(btnPlay);
    control('toggle', null, btnPlay);
  });
  btnPrev.addEventListener('click', (e) => {
    e.preventDefault();
    flashBtn(btnPrev);
    control('previous', null, btnPrev);
  });
  btnNext.addEventListener('click', (e) => {
    e.preventDefault();
    flashBtn(btnNext);
    control('next', null, btnNext);
  });

  // Volume drag
  let dragging = false;
  function vbarSeek(e) {
    const rect = vbar.getBoundingClientRect();
    const x = (e.touches ? e.touches[0].clientX : e.clientX) - rect.left;
    const pct = Math.max(0, Math.min(100, (x / rect.width) * 100));
    vbarFill.style.width = pct + '%';
    control('volume', { v: Math.round(pct) });
  }
  vbar.addEventListener('mousedown', (e) => { dragging = true; vbarSeek(e); });
  window.addEventListener('mousemove', (e) => { if (dragging) vbarSeek(e); });
  window.addEventListener('mouseup', () => { dragging = false; });
  vbar.addEventListener('touchstart', (e) => { dragging = true; vbarSeek(e); e.preventDefault(); }, { passive: false });
  vbar.addEventListener('touchmove', (e) => { if (dragging) vbarSeek(e); e.preventDefault(); }, { passive: false });
  vbar.addEventListener('touchend', () => { dragging = false; });

  // ── CometD / Bayeux client (LMS-style /slim/subscribe + /slim/request) ──
  let clientId = null;
  let subscribedMacs = new Set();
  let reconnectTimer = null;

  async function cometdPost(messages, signal) {
    const r = await fetch('/cometd', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(messages),
      signal,
    });
    if (!r.ok) throw new Error('cometd ' + r.status);
    return r.json();
  }

  async function cometdHandshake() {
    const arr = await cometdPost([{
      channel: '/meta/handshake',
      version: '1.0',
      minimumVersion: '1.0',
      supportedConnectionTypes: ['long-polling', 'callback-polling'],
    }]);
    const resp = arr[0];
    if (!resp.successful) throw new Error('handshake failed');
    clientId = resp.clientId;
  }

  // Subscribe to status updates for a specific player MAC. Uses the LMS
  // /slim/subscribe channel which the server interprets as "register this
  // response channel for this player's status pushes".
  async function cometdSubscribePlayer(mac) {
    if (subscribedMacs.has(mac)) return;
    const channel = '/squeeze/playerstatus/' + mac;
    const arr = await cometdPost([{
      channel: '/slim/subscribe',
      clientId,
      subscription: channel,
      data: {
        response: channel,
        request: [mac, ['status']],
      },
    }]);
    const ack = arr[arr.length - 1];
    if (!ack || !ack.successful) {
      throw new Error('slim subscribe failed for ' + mac);
    }
    // The first non-ack item carries the initial status snapshot.
    for (let i = 0; i < arr.length - 1; i++) {
      const evt = arr[i];
      if (evt && evt.channel === channel && evt.data) applyStatus(evt.data);
    }
    subscribedMacs.add(mac);
    if (!currentMac) currentMac = mac;
  }

  // Long-poll for status updates. Each /meta/connect returns immediately
  // with any queued events; we re-issue it right away to keep listening.
  async function cometdConnectLoop() {
    if (reconnectTimer) { clearTimeout(reconnectTimer); reconnectTimer = null; }
    while (clientId) {
      let arr;
      try {
        arr = await cometdPost([{
          channel: '/meta/connect',
          clientId,
          connectionType: 'long-polling',
        }]);
      } catch (e) {
        scheduleReconnect();
        return;
      }
      const resp = arr[0];
      if (!resp) { scheduleReconnect(); return; }
      if (resp.successful) {
        setMeta('connected');
        if (Array.isArray(resp.data)) {
          for (const evt of resp.data) handleEvent(evt);
        }
      } else if (resp.error && resp.error.indexOf('+clientId') >= 0) {
        // Server invalidated our client — re-handshake.
        clientId = null;
        subscribedMacs.clear();
        connect();
        return;
      } else {
        scheduleReconnect();
        return;
      }
    }
  }

  function handleEvent(evt) {
    if (!evt || !evt.channel) return;
    if (evt.channel.startsWith('/squeeze/playerstatus/') && evt.data) {
      applyStatus(evt.data);
    }
  }

  function scheduleReconnect() {
    setMeta('connecting');
    if (reconnectTimer) return;
    reconnectTimer = setTimeout(() => { reconnectTimer = null; connect(); }, 2000);
  }

  async function connect() {
    try {
      setMeta('connecting');
      if (!clientId) await cometdHandshake();
      const players = await fetch('/api/players').then((r) => r.json()).catch(() => ({}));
      const list = (players && players.players) || [];
      if (list.length === 0) {
        // No players yet — keep retrying.
        setTimeout(connect, 1500);
        return;
      }
      for (const p of list) {
        try { await cometdSubscribePlayer(p.mac); } catch (e) { console.warn('sub', e); }
      }
      cometdConnectLoop();
    } catch (e) {
      console.warn('connect failed', e);
      scheduleReconnect();
    }
  }

  // Initial snapshot so the page has data even before the first CometD push.
  (async () => {
    try {
      const r = await fetch('/api/status');
      if (r.ok) {
        const d = await r.json();
        applyStatus(d);
      }
    } catch {}
    setMeta('connecting');
    connect();
  })();
})();
</script>
</body>
</html>"##;
