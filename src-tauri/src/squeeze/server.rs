// TCP SlimProto server — accepts player connections on port 3483,
// handles the binary protocol, and orchestrates playback.

use crate::squeeze::codec::{self, ClientMessage, MacAddress};
use crate::squeeze::cometd::CometdState;
use crate::squeeze::player::{PlayerMap, PlayerState, SqueezePlayer, StatAction};
use crate::squeeze::streaming::StreamingState;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

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

        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }

            let (stream, addr) = match listener.accept().await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("Squeeze TCP: accept error: {}", e);
                    continue;
                }
            };

            tracing::info!("Squeeze TCP: connection from {}", addr);

            let players = players.clone();
            let streaming = streaming.clone();
            let shutdown = shutdown.clone();
            let cometd = cometd.clone();

            tokio::spawn(async move {
                handle_connection(stream, addr, players, streaming, cometd, shutdown).await;
            });
        }

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
    let mac = match read_and_parse_helo(&mut reader, writer, server_ip, &players).await {
        Some(mac) => mac,
        None => {
            tracing::warn!("Squeeze TCP: connection from {} did not send valid HELO", addr);
            return;
        }
    };

    tracing::info!("Squeeze TCP: player {} connected ({})", mac, addr);

    // Send handshake
    {
        let mut map = players.lock().await;
        if let Some(player) = map.get_mut(&mac) {
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

        match msg {
            ClientMessage::Stat(stat) => {
                let (action, state_changed) = {
                    let mut map = players.lock().await;
                    if let Some(player) = map.get_mut(&mac) {
                        let old_state = player.state;
                        let action = player.handle_stat(&stat);
                        (action, player.state != old_state)
                    } else {
                        (StatAction::None, false)
                    }
                };

                // Notify CometD subscribers if playback state changed
                if state_changed {
                    cometd.notify_player_status(&mac.to_string()).await;
                }

                match action {
                    StatAction::Prefetch => {
                        handle_prefetch(&mac, &players, &streaming).await;
                    }
                    StatAction::TrackFinished => {
                        handle_track_finished(&mac, &players, &streaming).await;
                        cometd.notify_player_status(&mac.to_string()).await;
                    }
                    StatAction::None => {}
                }

                // Respond to heartbeat with status request
                if stat.event == codec::StatEvent::Timer {
                    let frame = codec::encode_strm_simple(codec::StrmCommand::Status, stat.jiffies);
                    let mut map = players.lock().await;
                    if let Some(player) = map.get_mut(&mac) {
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
                    if let Some(player) = map.get_mut(&mac) {
                        player.name = name.clone();
                        tracing::info!("Squeeze TCP: player {} name = \"{}\"", mac, name);
                    }
                }
            }
            ClientMessage::Helo(_) => {
                // Duplicate HELO, ignore
            }
            ClientMessage::Resp(_) | ClientMessage::Meta(_) => {
                // HTTP headers / metadata from stream — we don't need these
            }
            ClientMessage::Unknown(tag, _) => {
                tracing::trace!("Squeeze TCP: unknown message '{}' from {}", tag, mac);
            }
        }
    }

    // Cleanup
    let mut map = players.lock().await;
    map.remove(&mac);
    tracing::info!("Squeeze TCP: player {} removed", mac);
}

/// Read the initial HELO message and register the player.
async fn read_and_parse_helo(
    reader: &mut tokio::net::tcp::OwnedReadHalf,
    writer: tokio::net::tcp::OwnedWriteHalf,
    server_ip: Ipv4Addr,
    players: &PlayerMap,
) -> Option<MacAddress> {
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
            let mut map = players.lock().await;

            if let Some(existing) = map.get_mut(&mac) {
                // Player already registered (e.g., via CometD). Merge TCP writer.
                existing.set_writer(writer, server_ip);
                existing.capabilities = helo.capabilities;
                tracing::info!("Squeeze TCP: merged TCP writer into existing player {}", mac);
            } else {
                let player = SqueezePlayer::new(
                    mac,
                    format!("Squeeze Player {}", mac),
                    helo.capabilities,
                    writer,
                    server_ip,
                );
                map.insert(mac, player);
            }

            Some(mac)
        }
        _ => None,
    }
}

/// Handle gapless prefetch: advance queue, queue file, send strm with NO_RESTART_DECODER.
async fn handle_prefetch(
    mac: &MacAddress,
    players: &PlayerMap,
    streaming: &StreamingState,
) {
    let (next_track, gen) = {
        let mut map = players.lock().await;
        let player = match map.get_mut(mac) {
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
        player.generation += 1;
        let gen = player.generation;
        streaming.queue_file(mac, path, gen, 0).await;

        (next, gen)
    };

    // Send strm 's' with NO_RESTART_DECODER flag
    {
        let mut map = players.lock().await;
        if let Some(player) = map.get_mut(mac) {
            player.seek_offset_ms = 0;
            if let Err(e) = player.start_stream(HTTP_PORT, 0x40).await {
                tracing::error!("Squeeze: prefetch start_stream failed: {}", e);
            }
        }
    }

    eprintln!("[SQUEEZE] prefetch: \"{}\" by {} (gen={})", next_track.title, next_track.artist, gen);
}

/// Handle track finished (STMu): if prefetch happened, just confirm. Otherwise, play next.
async fn handle_track_finished(
    mac: &MacAddress,
    players: &PlayerMap,
    streaming: &StreamingState,
) {
    let needs_start = {
        let map = players.lock().await;
        let player = match map.get(mac) {
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
            if let Some(player) = map.get_mut(mac) {
                player.queue.next().is_some()
            } else {
                false
            }
        };

        if has_next {
            let (path, title) = {
                let map = players.lock().await;
                map.get(mac)
                    .and_then(|p| p.queue.current().map(|t| (PathBuf::from(&t.path), t.title.clone())))
                    .unzip()
            };

            if let (Some(path), Some(title)) = (path, title) {
                eprintln!("[SQUEEZE] track finished -> now playing: \"{}\"", title);
                let gen = {
                    let mut map = players.lock().await;
                    if let Some(player) = map.get_mut(mac) {
                        player.generation += 1;
                        player.generation
                    } else {
                        return;
                    }
                };

                streaming.queue_file(mac, path, gen, 0).await;

                let mut map = players.lock().await;
                if let Some(player) = map.get_mut(mac) {
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
            eprintln!("[SQUEEZE] queue exhausted — stopping");
            let mut map = players.lock().await;
            if let Some(player) = map.get_mut(mac) {
                player.state = PlayerState::Stopped;
            }
        }
    } else {
        // Prefetch already handled it — just reset the prefetch flag
        eprintln!("[SQUEEZE] track finished (prefetch already active)");
        let mut map = players.lock().await;
        if let Some(player) = map.get_mut(mac) {
            player.display_track = None;
            player.prefetched_generation = None;
            player.seek_offset_ms = 0;
            player.elapsed_ms = 0;
            player.play_started_at = Some(Instant::now());
        }
    }
}
