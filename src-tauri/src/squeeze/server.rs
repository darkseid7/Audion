// TCP SlimProto server — accepts player connections on port 3483,
// handles the binary protocol, and orchestrates playback.

use crate::squeeze::codec::{self, ClientMessage, MacAddress};
use crate::squeeze::cometd::CometdState;
use crate::squeeze::player::{PlayerMap, PlayerState, SqueezePlayer, StatAction, friendly_name};
use crate::squeeze::streaming::StreamingState;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;
use tokio::task::{JoinHandle, JoinSet};

/// HTTP port for the streaming server.
pub const HTTP_PORT: u16 = 9000;

/// Detect our local IP relative to the client.
fn detect_local_ip(client_addr: &SocketAddr) -> Ipv4Addr {
    let sock = match std::net::UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(_) => return Ipv4Addr::LOCALHOST,
    };
    let _ = sock.connect(client_addr);
    match sock.local_addr() {
        Ok(SocketAddr::V4(v4)) => *v4.ip(),
        _ => Ipv4Addr::LOCALHOST,
    }
}

/// Start the TCP SlimProto server with a pre-bound listener.
pub fn start_slimproto_server_with_listener(
    listener: TcpListener,
    players: PlayerMap,
    streaming: StreamingState,
    cometd: CometdState,
    shutdown: Arc<AtomicBool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        tracing::info!("Squeeze TCP: listening on port 3483");

        let mut connections = JoinSet::new();
        let mut shutdown_poll = tokio::time::interval(std::time::Duration::from_millis(100));
        loop {
            tokio::select! {
                biased;
                _ = shutdown_poll.tick() => {
                    if shutdown.load(Ordering::Relaxed) {
                        break;
                    }
                }
                _ = connections.join_next(), if !connections.is_empty() => {}
                result = listener.accept() => {
                    let (stream, addr) = match result {
                        Ok(r) => r,
                        Err(e) => {
                            tracing::warn!("Squeeze TCP: accept error: {}", e);
                            continue;
                        }
                    };
                    tracing::info!("Squeeze TCP: connection from {}", addr);
                    connections.spawn(handle_connection(
                        stream, addr, players.clone(), streaming.clone(),
                        cometd.clone(), shutdown.clone(),
                    ));
                }
            }
        }
        // JoinSet also aborts these readers if the listener task is aborted.
        connections.abort_all();
        while connections.join_next().await.is_some() {}

        tracing::info!("Squeeze TCP: server stopped");
    })
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
    addr: SocketAddr,
    players: PlayerMap,
    streaming: StreamingState,
    cometd: CometdState,
    shutdown: Arc<AtomicBool>,
) {
    let (mut reader, writer) = stream.into_split();
    let server_ip = detect_local_ip(&addr);

    // Read the first message — must be HELO
    let (mac, session_id) = match read_and_parse_helo(&mut reader, writer, server_ip, &players).await {
        Some(session) => session,
        None => {
            tracing::warn!("Squeeze TCP: connection from {} did not send valid HELO", addr);
            return;
        }
    };

    tracing::info!("Squeeze TCP: player {} connected ({})", mac, addr);

    // Send handshake
    {
        let mut map = players.lock().await;
        if let Some(player) = map.get_mut(&mac).filter(|p| p.owns_tcp_session(session_id)) {
            if let Err(e) = player.send_handshake().await {
                tracing::error!("Squeeze TCP: handshake failed for {}: {}", mac, e);
                map.remove(&mac);
                return;
            }
        }
    }

    // Main read loop
    let mut tag_buf = [0u8; 4];
    let mut len_buf = [0u8; 4];

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        // Read 4-byte tag
        match reader.read_exact(&mut tag_buf).await {
            Ok(_) => {}
            Err(e) => {
                tracing::info!("Squeeze TCP: player {} disconnected: {}", mac, e);
                break;
            }
        }

        // Read 4-byte payload length
        if reader.read_exact(&mut len_buf).await.is_err() {
            break;
        }
        let payload_len = u32::from_be_bytes(len_buf) as usize;

        // Sanity check
        if payload_len > 1_000_000 {
            tracing::warn!("Squeeze TCP: absurd payload size {} from {}, dropping", payload_len, mac);
            break;
        }

        // Read payload
        let mut payload = vec![0u8; payload_len];
        if payload_len > 0 {
            if reader.read_exact(&mut payload).await.is_err() {
                break;
            }
        }

        let msg = codec::parse_client_message(&tag_buf, &payload);
        if !players.lock().await.get(&mac).is_some_and(|p| p.owns_tcp_session(session_id)) {
            break;
        }


        match msg {
            ClientMessage::Stat(stat) => {
                let (action, state_changed) = {
                    let mut map = players.lock().await;
                    if let Some(player) = map.get_mut(&mac).filter(|p| p.owns_tcp_session(session_id)) {
                        let old_state = player.state;
                        let action = player.handle_stat(&stat);
                        (action, player.state != old_state)
                    } else {
                        (StatAction::None, false)
                    }
                };

                // Notify CometD subscribers if playback state changed or track started
                // Also notify on Paused/Resumed to push device-reported elapsed time
                if state_changed
                    || stat.event == codec::StatEvent::TrackStarted
                    || stat.event == codec::StatEvent::Paused
                    || stat.event == codec::StatEvent::Resumed
                {
                    cometd.notify_player_status(&mac.to_string()).await;
                }

                match action {
                    StatAction::Prefetch => {
                        handle_prefetch(&mac, &players, &streaming, Some(session_id)).await;
                    }
                    StatAction::TrackFinished => {
                        handle_track_finished(&mac, &players, &streaming, Some(session_id)).await;
                        cometd.notify_player_status(&mac.to_string()).await;
                    }
                    StatAction::None => {}
                }

                // Respond to heartbeat with status request
                if stat.event == codec::StatEvent::Timer {
                    let frame = codec::encode_strm_simple(codec::StrmCommand::Status, stat.jiffies);
                    let mut map = players.lock().await;
                    if let Some(player) = map.get_mut(&mac).filter(|p| p.owns_tcp_session(session_id)) {
                        let _ = player.send(&frame).await;
                    }
                }
            }
            ClientMessage::Bye(_) => {
                tracing::info!("Squeeze TCP: player {} sent BYE", mac);
                break;
            }
            ClientMessage::Dsco(_) => {
                tracing::debug!("Squeeze TCP: player {} stream disconnected", mac);
            }
            ClientMessage::Setd(data) => {
                // Player name response: byte 0 = id (0x00 = name), rest = name string
                if !data.is_empty() && data[0] == 0x00 && data.len() > 1 {
                    let name = String::from_utf8_lossy(&data[1..]).trim_end_matches('\0').to_string();
                    let mut map = players.lock().await;
                    if let Some(player) = map.get_mut(&mac).filter(|p| p.owns_tcp_session(session_id)) {
                        // Only update if we don't already have a friendly name
                        if friendly_name(&mac).is_none() {
                            player.name = name.clone();
                        }
                        tracing::info!("Squeeze TCP: player {} SETD name = \"{}\" (using \"{}\")", mac, name, player.name);
                    }
                }
            }
            ClientMessage::Helo(_) => {
                // Duplicate HELO, ignore
            }
            ClientMessage::Resp(_) | ClientMessage::Meta(_) => {
                // HTTP headers / metadata from stream — we don't need these
            }
            ClientMessage::Butn(btn) => {
                tracing::info!("Squeeze TCP: BUTN from {} code=0x{:08x}", mac, btn.button_code);
                let mac_str = mac.to_string();
                // IR codes observed from Eversolo + standard Squeezebox codes:
                //   Pause/play toggle: 0x768920df (Eversolo pause), 0x768910ef (Eversolo resume)
                //   Next/fwd:          0x7689a05f (Eversolo), 0x7689e01f (Squeezebox fwd), 0x7689a25d (Squeezebox fwd.single)
                //   Prev/rew:          0x7689c03f (Eversolo), 0x7689d02f (Squeezebox rew), 0x7689c23d (Squeezebox rew.single)
                //   Volume up:         0x768940bf
                //   Volume down:       0x7689c43b
                //   Power:             0x76898877
                match btn.button_code {
                    0x768920df | 0x768910ef => {
                        // Pause/Play toggle
                        let mut map = players.lock().await;
                        if let Some(player) = map.get_mut(&mac).filter(|p| p.owns_tcp_session(session_id)) {
                            if player.state == PlayerState::Playing {
                                player.elapsed_ms = player.get_elapsed_ms();
                                player.play_started_at = None;
                                player.state = PlayerState::Paused;
                                let _ = player.pause().await;
                                tracing::info!("Squeeze IR: PAUSE sent to {} elapsed_ms={}", mac, player.elapsed_ms);
                            } else if player.state == PlayerState::Paused {
                                player.play_started_at = Some(std::time::Instant::now());
                                player.state = PlayerState::Playing;
                                let _ = player.resume().await;
                                tracing::info!("Squeeze IR: RESUME sent to {} elapsed_ms={}", mac, player.elapsed_ms);
                            }
                        }
                        drop(map);
                        cometd.notify_player_status(&mac_str).await;
                    }
                    0x7689a05f | 0x7689e01f | 0x7689a25d => {
                        // Next track
                        control_next_for_session(&mac, &players, &streaming, Some(session_id)).await;
                        cometd.notify_player_status(&mac_str).await;
                    }
                    0x7689c03f | 0x7689d02f | 0x7689c23d => {
                        // Previous track
                        control_previous_for_session(&mac, &players, &streaming, Some(session_id)).await;
                        cometd.notify_player_status(&mac_str).await;
                    }
                    _ => {
                        tracing::info!("Squeeze TCP: unhandled BUTN code 0x{:08x} from {}", btn.button_code, mac);
                    }
                }
            }
            ClientMessage::Unknown(tag, data) => {
                tracing::info!("Squeeze TCP: unknown message '{}' from {} ({} bytes)", tag, mac, data.len());
            }
        }
    }

    // Cleanup. Only drop the player entry if no CometD session is
    // currently bound to the same MAC — otherwise the WebUI control
    // plane would lose its registration just because the hardware
    // player disconnected.
    let cometd_mac_str = mac.to_string();
    {
        let players_lock = players.clone();
        let cometd_for_check = cometd.clone();
        let mac_for_check = cometd_mac_str.clone();
        // Keep the binding stable through cleanup (mac -> players lock order).
        let mac_map = cometd_for_check.mac_to_client.lock().await;
        let has_cometd = mac_map.contains_key(&mac_for_check);
        let mut map = players_lock.lock().await;
        if !map.get(&mac).is_some_and(|p| p.owns_tcp_session(session_id)) {
            return;
        }
        if has_cometd {
            // Keep the player entry; just clear the TCP writer so the
            // CometD path takes over (state polling now relies solely
            // on the long-poll updates pushed from the Eversolo WebUI).
            if let Some(p) = map.get_mut(&mac).filter(|p| p.owns_tcp_session(session_id)) {
                p.clear_writer();
            }
            tracing::info!(
                "Squeeze TCP: player {} TCP closed; kept registration (CometD still active)",
                mac
            );
        } else {
            map.remove(&mac);
            tracing::info!("Squeeze TCP: player {} removed", mac);
        }
    }
}

/// Read the initial HELO message and register the player.
async fn read_and_parse_helo(
    reader: &mut tokio::net::tcp::OwnedReadHalf,
    writer: tokio::net::tcp::OwnedWriteHalf,
    server_ip: Ipv4Addr,
    players: &PlayerMap,
) -> Option<(MacAddress, u64)> {
    let mut tag_buf = [0u8; 4];
    let mut len_buf = [0u8; 4];

    // Set a timeout for the initial HELO
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        reader.read_exact(&mut tag_buf).await?;
        reader.read_exact(&mut len_buf).await?;
        let payload_len = u32::from_be_bytes(len_buf) as usize;
        if payload_len > 10_000 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "HELO too large"));
        }
        let mut payload = vec![0u8; payload_len];
        if payload_len > 0 {
            reader.read_exact(&mut payload).await?;
        }
        Ok::<_, std::io::Error>(payload)
    });

    let payload = match timeout.await {
        Ok(Ok(p)) => p,
        _ => return None,
    };

    if &tag_buf != b"HELO" {
        tracing::warn!("Squeeze TCP: expected HELO, got {:?}", String::from_utf8_lossy(&tag_buf));
        return None;
    }

    let msg = codec::parse_client_message(&tag_buf, &payload);
    match msg {
        ClientMessage::Helo(helo) => {
            let mac = helo.mac;
            let name = extract_model_name(&helo.capabilities)
                .unwrap_or_else(|| format!("Squeeze Player {}", mac));
            let mut map = players.lock().await;

            if let Some(existing) = map.get_mut(&mac) {
                // Player already registered (e.g., via CometD). Merge TCP writer.
                existing.set_writer(writer, server_ip);
                existing.capabilities = helo.capabilities;
                // Update name if it's still the generic placeholder
                if existing.name.starts_with("Player ") || existing.name.starts_with("Squeeze Player ") {
                    existing.name = name.clone();
                }
                tracing::info!("Squeeze TCP: merged TCP writer into existing player {} (\"{}\")", mac, existing.name);
            } else {
                let player = SqueezePlayer::new(
                    mac,
                    name,
                    helo.capabilities,
                    writer,
                    server_ip,
                );
                map.insert(mac, player);
            }

            Some((mac, map.get(&mac)?.tcp_session_id()?))
        }
        _ => None,
    }
}

/// Extract a human-readable model name from the SlimProto capabilities string.
fn extract_model_name(capabilities: &str) -> Option<String> {
    for part in capabilities.split(',') {
        let part = part.trim();
        if let Some(name) = part.strip_prefix("ModelName:").or_else(|| part.strip_prefix("ModelName=")) {
            let name = name.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Handle gapless prefetch: advance queue, queue file, send strm with NO_RESTART_DECODER.
async fn handle_prefetch(
    mac: &MacAddress,
    players: &PlayerMap,
    streaming: &StreamingState,
    session_id: Option<u64>,
) {
    let (next_track, gen) = {
        let mut map = players.lock().await;
        let player = match map.get_mut(mac).filter(|p| session_id.is_none_or(|id| p.owns_tcp_session(id))) {
            Some(p) => p,
            None => return,
        };

        // Save current track for display before advancing queue
        player.display_track = player.queue.current().cloned();

        // Advance queue to next track
        let next = match player.queue.next() {
            Some(t) => t.clone(),
            None => return,
        };

        // Queue the file for HTTP streaming
        let path = PathBuf::from(&next.path);
        player.advance_stream_generation();
        let gen = player.generation;
        streaming.queue_file(mac, path, gen, 0).await;

        (next, gen)
    };

    // Send strm 's' with NO_RESTART_DECODER flag
    {
        let mut map = players.lock().await;
        if let Some(player) = map.get_mut(mac).filter(|p| session_id.is_none_or(|id| p.owns_tcp_session(id))) {
            player.seek_offset_ms = 0;
            if let Err(e) = player.start_stream(HTTP_PORT, 0x40).await {
                tracing::error!("Squeeze: prefetch start_stream failed: {}", e);
            }
        }
    }

    tracing::info!("Squeeze: prefetch: \"{}\" by {} (gen={})", next_track.title, next_track.artist, gen);
}

/// Handle track finished (STMu): if prefetch happened, just confirm. Otherwise, play next.
async fn handle_track_finished(
    mac: &MacAddress,
    players: &PlayerMap,
    streaming: &StreamingState,
    session_id: Option<u64>,
) {
    let needs_start = {
        let map = players.lock().await;
        let player = match map.get(mac).filter(|p| session_id.is_none_or(|id| p.owns_tcp_session(id))) {
            Some(p) => p,
            None => return,
        };

        // If prefetch already sent the next stream, we're done (stream is running)
        player.prefetched_generation.is_none()
    };

    if needs_start {
        // No prefetch — need to advance and start fresh
        let has_next = {
            let mut map = players.lock().await;
            if let Some(player) = map.get_mut(mac).filter(|p| session_id.is_none_or(|id| p.owns_tcp_session(id))) {
                player.queue.next().is_some()
            } else {
                false
            }
        };

        if has_next {
            let (path, title) = {
                let map = players.lock().await;
                map.get(mac).filter(|p| session_id.is_none_or(|id| p.owns_tcp_session(id)))
                    .and_then(|p| p.queue.current().map(|t| (PathBuf::from(&t.path), t.title.clone())))
                    .unzip()
            };

            if let (Some(path), Some(title)) = (path, title) {
                tracing::info!("Squeeze: track finished -> now playing: \"{}\"", title);
                {
                    let mut map = players.lock().await;
                    if let Some(player) = map.get_mut(mac).filter(|p| session_id.is_none_or(|id| p.owns_tcp_session(id))) {
                        player.advance_stream_generation();
                        streaming.queue_file(mac, path, player.generation, 0).await;
                    } else {
                        return;
                    }
                };

                let mut map = players.lock().await;
                if let Some(player) = map.get_mut(mac).filter(|p| session_id.is_none_or(|id| p.owns_tcp_session(id))) {
                    player.seek_offset_ms = 0;
                    player.elapsed_ms = 0;
                    player.play_started_at = Some(Instant::now());
                    if let Err(e) = player.start_stream(HTTP_PORT, 0).await {
                        tracing::error!("Squeeze: track-finished start_stream failed: {}", e);
                    }
                }
            }
        } else {
            // Queue exhausted
            tracing::info!("Squeeze: queue exhausted — stopping");
            let mut map = players.lock().await;
            if let Some(player) = map.get_mut(mac).filter(|p| session_id.is_none_or(|id| p.owns_tcp_session(id))) {
                player.state = PlayerState::Stopped;
            }
        }
    } else {
        // Prefetch already handled it — just reset the prefetch flag
        tracing::debug!("Squeeze: track finished (prefetch already active)");
        let mut map = players.lock().await;
        if let Some(player) = map.get_mut(mac).filter(|p| session_id.is_none_or(|id| p.owns_tcp_session(id))) {
            player.display_track = None;
            player.prefetched_generation = None;
            player.seek_offset_ms = 0;
            player.elapsed_ms = 0;
            player.play_started_at = Some(Instant::now());
        }
    }
}

/// Start streaming the current track for a player (used by BUTN handler).
async fn handle_butn_start_track(
    mac: &MacAddress,
    players: &PlayerMap,
    streaming: &StreamingState,
    session_id: Option<u64>,
) {
    let path = {
        let map = players.lock().await;
        match map.get(mac).filter(|p| session_id.is_none_or(|id| p.owns_tcp_session(id))).and_then(|p| p.queue.current().map(|t| PathBuf::from(&t.path))) {
            Some(p) => p,
            None => return,
        }
    };

    {
        let mut map = players.lock().await;
        if let Some(player) = map.get_mut(mac).filter(|p| session_id.is_none_or(|id| p.owns_tcp_session(id))) {
            player.advance_stream_generation();
            streaming.queue_file(mac, path, player.generation, 0).await;
        } else {
            return;
        }
    };

    let mut map = players.lock().await;
    if let Some(player) = map.get_mut(mac).filter(|p| session_id.is_none_or(|id| p.owns_tcp_session(id))) {
        player.seek_offset_ms = 0;
        player.elapsed_ms = 0;
        player.play_started_at = Some(Instant::now());
        if let Err(e) = player.start_stream(HTTP_PORT, 0).await {
            tracing::error!("Squeeze: BUTN start_stream failed: {}", e);
        }
    }
}

/// Advance the queue to the next track and start streaming it.
/// Shared between the IR BUTN handler and HTTP control endpoints.
pub async fn control_next(
    mac: &MacAddress,
    players: &PlayerMap,
    streaming: &StreamingState,
) {
    control_next_for_session(mac, players, streaming, None).await;
}

async fn control_next_for_session(
    mac: &MacAddress,
    players: &PlayerMap,
    streaming: &StreamingState,
    session_id: Option<u64>,
) {
    {
        let mut map = players.lock().await;
        if let Some(player) = map.get_mut(mac).filter(|p| session_id.is_none_or(|id| p.owns_tcp_session(id))) {
            player.display_track = None;
            let _ = player.stop().await;
            let _ = player.flush().await;
            player.suppress_track_finished = true;
            if player.prefetched_generation.is_some() {
                player.prefetched_generation = None;
            }
        }
    }
    let has_next = {
        let mut map = players.lock().await;
        match map.get_mut(mac).filter(|p| session_id.is_none_or(|id| p.owns_tcp_session(id))) {
            Some(player) => player.queue.next().is_some(),
            None => false,
        }
    };
    if has_next {
        handle_butn_start_track(mac, players, streaming, session_id).await;
    }
}

/// Rewind the queue to the previous track and start streaming it.
/// Shared between the IR BUTN handler and HTTP control endpoints.
pub async fn control_previous(
    mac: &MacAddress,
    players: &PlayerMap,
    streaming: &StreamingState,
) {
    control_previous_for_session(mac, players, streaming, None).await;
}

async fn control_previous_for_session(
    mac: &MacAddress,
    players: &PlayerMap,
    streaming: &StreamingState,
    session_id: Option<u64>,
) {
    {
        let mut map = players.lock().await;
        if let Some(player) = map.get_mut(mac).filter(|p| session_id.is_none_or(|id| p.owns_tcp_session(id))) {
            player.display_track = None;
            let _ = player.stop().await;
            let _ = player.flush().await;
            player.suppress_track_finished = true;
            if player.prefetched_generation.is_some() {
                player.prefetched_generation = None;
            }
        }
    }
    let has_prev = {
        let mut map = players.lock().await;
        match map.get_mut(mac).filter(|p| session_id.is_none_or(|id| p.owns_tcp_session(id))) {
            Some(player) => player.queue.previous().is_some(),
            None => false,
        }
    };
    if has_prev {
        handle_butn_start_track(mac, players, streaming, session_id).await;
    }
}

/// Explicitly disconnect a player: tell it to stop (best effort), drop
/// its TCP writer — which closes the SlimProto connection — remove the
/// player entry, and clear its queued stream so any in-flight
/// `/stream?player=<mac>` request 404s immediately.
///
/// Unlike the implicit TCP-cleanup path, this removes the entry even
/// when a CometD session is bound: the user asked for a hard disconnect.
pub async fn disconnect_player(
    mac: &MacAddress,
    players: &PlayerMap,
    streaming: &StreamingState,
    cometd: &CometdState,
) {
    let mut map = players.lock().await;
    if let Some(player) = map.get_mut(mac) {
        player.display_track = None;
        player.prefetched_generation = None;
        player.suppress_track_finished = false;
        // Best effort: tell the hardware to stop before we cut the wire.
        let _ = player.stop().await;
        let _ = player.flush().await;
        player.state = PlayerState::Stopped;
    }
    // Keep removal and stream cleanup in one critical section: a new HELO
    // must not register between stopping the old session and removing it.
    map.remove(mac);
    streaming.clear_player(mac).await;
    drop(map);

    cometd.notify_player_status(&mac.to_string()).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::squeeze::player::new_player_map;
    use crate::squeeze::streaming::StreamingState;
    use std::sync::atomic::AtomicBool;
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;

    /// Build a minimal client→server HELO frame: [tag][len u32 BE][payload].
    /// Payload: device_id(1) + revision(1) + mac(6) + uuid(24) + wlan(4),
    /// then the capabilities string at offset 36 (matches `parse_helo`).
    fn helo_frame(mac: [u8; 6]) -> Vec<u8> {
        let mut payload = vec![0u8; 60];
        payload[0] = 1; // device_id
        payload[1] = 1; // revision
        payload[2..8].copy_from_slice(&mac);
        let caps = b"ModelName:TestPlayer,";
        payload[36..36 + caps.len()].copy_from_slice(caps);
        let mut frame = b"HELO".to_vec();
        frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(&payload);
        frame
    }

    fn client_frame(tag: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut frame = tag.to_vec();
        frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(payload);
        frame
    }

    async fn connect_test_player(
        mac: [u8; 6],
        players: &PlayerMap,
        streaming: &StreamingState,
        cometd: &CometdState,
    ) -> (tokio::net::TcpStream, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut client = tokio::net::TcpStream::connect(listener.local_addr().unwrap()).await.unwrap();
        let (socket, addr) = listener.accept().await.unwrap();
        let task = tokio::spawn(handle_connection(
            socket, addr, players.clone(), streaming.clone(), cometd.clone(),
            Arc::new(AtomicBool::new(false)),
        ));
        client.write_all(&helo_frame(mac)).await.unwrap();
        // Receipt of all five handshake frames proves the new writer is attached.
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            for _ in 0..5 {
                let mut len = [0; 2];
                client.read_exact(&mut len).await.unwrap();
                let mut payload = vec![0; u16::from_be_bytes(len) as usize];
                client.read_exact(&mut payload).await.unwrap();
            }
        }).await.expect("handshake timed out");
        (client, task)
    }

    #[tokio::test]
    async fn superseded_tcp_cleanup_preserves_replacement() {
        for has_cometd in [false, true] {
            let players = new_player_map();
            let streaming = StreamingState::new();
            let cometd = CometdState::new(players.clone(), streaming.clone());
            let mac = [1, 2, 3, 4, 5, 6];
            let mac_addr = MacAddress(mac);
            if has_cometd {
                cometd.mac_to_client.lock().await.insert(mac_addr.to_string(), "test-client".into());
            }
            let (mut old, old_task) = connect_test_player(mac, &players, &streaming, &cometd).await;
            let (_replacement, replacement_task) = connect_test_player(mac, &players, &streaming, &cometd).await;
            old.write_all(&client_frame(b"BYE!", &[])).await.unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(2), old_task).await.unwrap().unwrap();
            let map = players.lock().await;
            assert!(map.get(&mac_addr).is_some_and(|p| p.has_tcp_writer()),
                "superseded TCP cleanup removed or closed replacement (CometD={has_cometd})");
            replacement_task.abort();
        }
    }

    #[tokio::test]
    async fn superseded_tcp_messages_do_not_mutate_replacement() {
        let mut button = vec![0; 8];
        button[4..].copy_from_slice(&0x768920dfu32.to_be_bytes());
        for (tag, payload) in [
            (*b"SETD", b"\0Stale name\0".to_vec()),
            (*b"STAT", b"STMp".to_vec()),
            (*b"BUTN", button),
        ] {
            let players = new_player_map();
            let streaming = StreamingState::new();
            let cometd = CometdState::new(players.clone(), streaming.clone());
            let mac = [1, 2, 3, 4, 5, 6];
            let mac_addr = MacAddress(mac);
            // Retain the registration during old-session cleanup so assertions
            // independently catch late message mutation, not just removal.
            cometd.mac_to_client.lock().await.insert(mac_addr.to_string(), "test-client".into());
            let (mut old, old_task) = connect_test_player(mac, &players, &streaming, &cometd).await;
            let (_replacement, replacement_task) = connect_test_player(mac, &players, &streaming, &cometd).await;
            players.lock().await.get_mut(&mac_addr).unwrap().state = PlayerState::Playing;
            old.write_all(&client_frame(&tag, &payload)).await.unwrap();
            old.write_all(&client_frame(b"BYE!", &[])).await.unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(2), old_task).await.unwrap().unwrap();
            let map = players.lock().await;
            let player = map.get(&mac_addr).expect("replacement disappeared");
            assert_eq!(player.name, "TestPlayer", "late {:?} changed replacement name", tag);
            assert_eq!(player.state, PlayerState::Playing, "late {:?} changed replacement state", tag);
            replacement_task.abort();
        }
    }

    #[tokio::test]
    async fn explicit_disconnect_does_not_remove_queued_takeover() {
        let players = new_player_map();
        let streaming = StreamingState::new();
        let cometd = CometdState::new(players.clone(), streaming.clone());
        let mac = [1, 2, 3, 4, 5, 6];
        let mac_addr = MacAddress(mac);
        let (_old, old_task) = connect_test_player(mac, &players, &streaming, &cometd).await;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut replacement = tokio::net::TcpStream::connect(listener.local_addr().unwrap()).await.unwrap();
        let (socket, _) = listener.accept().await.unwrap();
        replacement.write_all(&helo_frame(mac)).await.unwrap();
        let (mut reader, writer) = socket.into_split();

        // Tokio's mutex is FIFO: queue disconnect first, then registration.
        // Releasing and reacquiring the guard mid-disconnect lets HELO win.
        let guard = players.lock().await;
        let disconnect = tokio::spawn({
            let players = players.clone();
            let streaming = streaming.clone();
            let cometd = cometd.clone();
            async move { disconnect_player(&mac_addr, &players, &streaming, &cometd).await }
        });
        tokio::task::yield_now().await;
        let takeover = tokio::spawn({
            let players = players.clone();
            async move { read_and_parse_helo(&mut reader, writer, Ipv4Addr::LOCALHOST, &players).await }
        });
        tokio::task::yield_now().await;
        drop(guard);
        tokio::time::timeout(std::time::Duration::from_secs(2), disconnect).await.unwrap().unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), takeover).await.unwrap().unwrap().unwrap();
        assert!(players.lock().await.get(&mac_addr).is_some_and(|p| p.has_tcp_writer()),
            "disconnect removed a TCP session registered after it began");
        old_task.abort();
    }

    #[tokio::test]
    async fn slimproto_shutdown_closes_idle_listener_and_connection() {
        let players = new_player_map();
        let streaming = StreamingState::new();
        let cometd = CometdState::new(players.clone(), streaming.clone());
        let shutdown = Arc::new(AtomicBool::new(false));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        let task = start_slimproto_server_with_listener(listener, players, streaming, cometd, shutdown.clone());
        tokio::task::yield_now().await;
        shutdown.store(true, Ordering::Relaxed);
        tokio::time::timeout(std::time::Duration::from_secs(2), task).await
            .expect("idle SlimProto listener ignored shutdown").unwrap();
        let mut byte = [0];
        assert_eq!(tokio::time::timeout(std::time::Duration::from_secs(1), client.read(&mut byte)).await
            .expect("idle HELO task retained its socket").unwrap(), 0);
        TcpListener::bind(addr).await.expect("SlimProto listener retained its port");
    }

    /// Register a fake SlimProto player over a real TCP connection, then
    /// verify that `disconnect_player` removes the entry, clears the
    /// queued stream, and actually closes the connection (the user-visible
    /// "disconnect and that's it" behavior).
    #[tokio::test]
    async fn disconnect_player_drops_entry_stream_and_connection() {
        let players = new_player_map();
        let streaming = StreamingState::new();
        let cometd = CometdState::new(players.clone(), streaming.clone());
        let shutdown = Arc::new(AtomicBool::new(false));

        let listener = crate::squeeze::bind_tcp_reuse("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = start_slimproto_server_with_listener(
            listener,
            players.clone(),
            streaming.clone(),
            cometd.clone(),
            shutdown.clone(),
        );

        // Fake player connects and sends HELO.
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let mac = [0x80, 0x0a, 0x80, 0x5e, 0xd6, 0xa5];
        stream.write_all(&helo_frame(mac)).await.unwrap();

        // Wait for registration (server sends handshake right after HELO).
        let mac_addr = MacAddress(mac);
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if players.lock().await.contains_key(&mac_addr) {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "player never registered"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }

        // Queue a stream entry, as a playing track would leave behind.
        streaming
            .queue_file(&mac_addr, PathBuf::from("C:\\fake\\track.mp3"), 1, 0)
            .await;

        // Drain the handshake frames so we can assert a clean EOF later.
        let mut tmp = [0u8; 512];
        loop {
            match tokio::time::timeout(std::time::Duration::from_millis(100), stream.read(&mut tmp)).await {
                Ok(Ok(0)) | Ok(Err(_)) | Err(_) => break,
                Ok(Ok(_)) => {}
            }
        }

        disconnect_player(&mac_addr, &players, &streaming, &cometd).await;

        // Player entry and stream entry are gone.
        assert!(!players.lock().await.contains_key(&mac_addr));
        assert!(streaming.get_entry(&mac_addr.to_string()).await.is_none());

        // The TCP connection is actually closed: read until EOF (control
        // frames sent right before the close may precede the FIN).
        let mut saw_eof = false;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match tokio::time::timeout(remaining, stream.read(&mut tmp)).await {
                Ok(Ok(0)) => {
                    saw_eof = true;
                    break;
                }
                Ok(Ok(_)) => {}
                Ok(Err(_)) | Err(_) => break,
            }
        }
        assert!(saw_eof, "TCP connection remained open after disconnect");

        shutdown.store(true, Ordering::Relaxed);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;
    }
}
