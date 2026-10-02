// Squeeze Connect module — embedded SlimProto server for streaming
// audio to Squeeze-compatible players (e.g. Eversolo, Squeezelite).

pub mod codec;
pub mod cometd;
pub mod discovery;
pub mod player;
pub mod queue;
pub mod server;
pub mod streaming;
pub mod webui;

use cometd::CometdState;
use player::PlayerMap;
use streaming::StreamingState;
use std::net::UdpSocket;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::net::{TcpListener, TcpSocket};
use tokio::task::JoinHandle;

// Parents mutate their registry exclusively; stop retains completion ownership
// and only locks it after the parent has been joined or aborted and awaited.
pub(super) type ConnectionTasks = Arc<tokio::sync::Mutex<tokio::task::JoinSet<()>>>;

pub(super) fn new_connection_tasks() -> ConnectionTasks {
    Arc::new(tokio::sync::Mutex::new(tokio::task::JoinSet::new()))
}

/// Top-level Squeeze server state, managed by Tauri.
pub struct SqueezeServer {
    pub players: PlayerMap,
    pub streaming: StreamingState,
    pub cometd: CometdState,
    pub running: Arc<AtomicBool>,
    shutdown_flag: Arc<AtomicBool>,
    discovery_shutdown: Option<Arc<AtomicBool>>,
    handles: Vec<JoinHandle<()>>,
    connection_tasks: Vec<ConnectionTasks>,
}

impl SqueezeServer {
    pub fn new() -> Self {
        let players = player::new_player_map();
        let streaming = StreamingState::new();
        Self {
            cometd: CometdState::new(players.clone(), streaming.clone()),
            players,
            streaming,
            running: Arc::new(AtomicBool::new(false)),
            shutdown_flag: Arc::new(AtomicBool::new(false)),
            discovery_shutdown: None,
            handles: Vec::new(),
            connection_tasks: Vec::new(),
        }
    }

    /// Start all Squeeze services (UDP discovery, TCP SlimProto, HTTP streaming).
    /// Pre-binds all sockets so failures are reported immediately.
    pub async fn start(&mut self) -> Result<(), String> {
        if self.running.load(Ordering::Relaxed) {
            return Err("Squeeze server already running".into());
        }

        // ── Pre-bind all sockets before spawning tasks ──────────────────
        let udp_socket = UdpSocket::bind("0.0.0.0:3483")
            .map_err(|e| format!("Cannot bind UDP port 3483 (discovery): {}. Is another Squeeze/LMS server running?", e))?;

        let tcp_listener = bind_tcp_reuse("0.0.0.0:3483").await
            .map_err(|e| format!("Cannot bind TCP port 3483 (SlimProto): {}. Is another Squeeze/LMS server running?", e))?;

        let http_listener = bind_tcp_reuse(&format!("0.0.0.0:{}", server::HTTP_PORT)).await
            .map_err(|e| format!("Cannot bind TCP port {} (HTTP streaming): {}", server::HTTP_PORT, e))?;

        // CLI port (9090) — some Squeeze controllers (e.g. Squeezer) need this
        let cli_listener = bind_tcp_reuse("0.0.0.0:9090").await
            .map_err(|e| format!("Cannot bind TCP port 9090 (CLI): {}", e))?;

        self.start_services(udp_socket, tcp_listener, http_listener, cli_listener);
        Ok(())
    }

    fn start_services(
        &mut self,
        udp_socket: UdpSocket,
        tcp_listener: TcpListener,
        http_listener: TcpListener,
        cli_listener: TcpListener,
    ) {
        // Each run owns a new flag; restarting must never revive old tasks.
        self.shutdown_flag = Arc::new(AtomicBool::new(false));

        // ── All sockets bound — now spawn the tasks ─────────────────────

        // 1. UDP discovery (pass pre-bound socket)
        let (discovery_handle, discovery_shutdown) =
            discovery::start_discovery_with_socket(udp_socket, server::HTTP_PORT, "Audion".to_string());
        self.handles.push(discovery_handle);
        self.discovery_shutdown = Some(discovery_shutdown);

        // 1b. Active re-announce broadcaster — for 60s after each server
        // start, periodically broadcast a TLV 'E' response to
        // 255.255.255.255:3483 so any player on the LAN that lost its
        // TCP connection (e.g. when the user closes+reopens Audion while
        // the Eversolo was still on) rediscover us without the user
        // having to manually trigger a rescan from the device.
        let bc_shutdown = self.shutdown_flag.clone();
        let bc_players = self.players.clone();
        let broadcaster_handle = discovery::start_discovery_broadcaster(
            server::HTTP_PORT,
            "Audion".to_string(),
            bc_players,
            bc_shutdown,
        );
        self.handles.push(broadcaster_handle);

        // 2. TCP SlimProto server (pass pre-bound listener)
        let tcp_connections = new_connection_tasks();
        self.connection_tasks.push(tcp_connections.clone());
        let tcp_handle = server::start_slimproto_server_with_listener_owned(
            tcp_listener,
            self.players.clone(),
            self.streaming.clone(),
            self.cometd.clone(),
            self.shutdown_flag.clone(),
            tcp_connections,
        );
        self.handles.push(tcp_handle);

        // 3. HTTP streaming server (pass pre-bound listener)
        let streaming_state = self.streaming.clone();
        let cometd_state = self.cometd.clone();
        let http_shutdown = self.shutdown_flag.clone();
        let http_connections = new_connection_tasks();
        self.connection_tasks.push(http_connections.clone());
        let http_handle = tokio::spawn(async move {
            if let Err(e) = streaming::start_streaming_server_with_listener_owned(http_listener, streaming_state, cometd_state, http_shutdown, http_connections).await {
                tracing::error!("Squeeze HTTP server failed: {}", e);
            }
        });
        self.handles.push(http_handle);

        // 4. CLI server (port 9090) — minimal handler for Squeezer-type controllers
        let cli_connections = new_connection_tasks();
        self.connection_tasks.push(cli_connections.clone());
        self.handles.push(start_cli_server_with_listener_owned(
            cli_listener,
            self.shutdown_flag.clone(),
            cli_connections,
        ));

        // 5. CometD watchdog — evicts sessions whose HTTP connection
        // vanished without sending /meta/disconnect (closed tab, dropped
        // network, etc) and removes their (CometD-only) player entries.
        // Without this, stale "Player" entries accumulate in the
        // player map and surface as ghost devices in the Connect panel.
        let wd_shutdown = self.shutdown_flag.clone();
        let wd_state = self.cometd.clone();
        let watchdog_handle = tokio::spawn(async move {
            cometd::run_watchdog(wd_state, wd_shutdown).await;
        });
        self.handles.push(watchdog_handle);

        self.running.store(true, Ordering::Relaxed);
        tracing::info!("Squeeze server started (UDP 3483, TCP 3483, HTTP {}, CLI 9090)", server::HTTP_PORT);

    }

    /// Stop all Squeeze services.
    pub async fn stop(&mut self) {
        // One total budget includes playback stop, status notification, and
        // transport draining; a blocked device must not get a fresh timeout.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        // Discovery runs on a blocking thread, so abort alone cannot stop it.
        // Signal it now so its read timeout overlaps the playback grace period.
        if let Some(shutdown) = self.discovery_shutdown.take() {
            shutdown.store(true, Ordering::Relaxed);
        }

        if tokio::time::timeout_at(deadline, self.prepare_shutdown_status()).await.is_err() {
            tracing::warn!("Squeeze shutdown: deadline reached preparing stopped status");
        }
        self.stop_connected_players(deadline).await;
        self.shutdown_flag.store(true, Ordering::Relaxed);

        // All services share one grace period. Keep each handle until it exits;
        // dropping a timed-out JoinHandle would detach its task instead.
        for mut handle in self.handles.drain(..) {
            if tokio::time::timeout_at(deadline, &mut handle).await.is_err() {
                handle.abort();
                let _ = handle.await;
            }
        }

        // Parent cancellation is not child cancellation completion. Retain the
        // registries after forced parent retirement, then abort and JOIN normal
        // cooperative children before clearing state or returning. This adds
        // no second I/O grace period; it acknowledges cancellation/resource drop.
        for connections in self.connection_tasks.drain(..) {
            connections.lock().await.shutdown().await;
        }

        // Owned service tasks have released their locks and sockets. If an
        // external holder outlives the deadline, isolate its old state rather
        // than hanging or reusing it on restart. Its resources cannot be
        // forcibly reclaimed, and this fallback is not peer-delivery proof.
        if tokio::time::timeout_at(deadline, async { self.players.lock().await.clear(); })
            .await.is_err() {
            tracing::warn!("Squeeze shutdown: isolating externally locked player state; old writers may remain owned externally");
            self.players = player::new_player_map();
            self.cometd.players = self.players.clone();
        }
        if tokio::time::timeout_at(deadline, async { self.cometd.clients.lock().await.clear(); })
            .await.is_err() {
            tracing::warn!("Squeeze shutdown: isolating externally locked CometD clients");
            self.cometd.clients = Arc::new(tokio::sync::Mutex::new(Default::default()));
        }
        if tokio::time::timeout_at(deadline, async { self.cometd.mac_to_client.lock().await.clear(); })
            .await.is_err() {
            tracing::warn!("Squeeze shutdown: isolating externally locked CometD routing");
            self.cometd.mac_to_client = Arc::new(tokio::sync::Mutex::new(Default::default()));
        }
        if tokio::time::timeout_at(deadline, self.streaming.clear()).await.is_err() {
            tracing::warn!("Squeeze shutdown: isolating externally locked streaming state");
            self.streaming = StreamingState::new();
            self.cometd.streaming = self.streaming.clone();
        }

        self.running.store(false, Ordering::Relaxed);
        tracing::info!("Squeeze server stopped");
    }

    async fn prepare_shutdown_status(&self) {
        let macs = {
            let mut map = self.players.lock().await;
            for player in map.values_mut() {
                player.state = player::PlayerState::Stopped;
                player.queue.set_tracks(Vec::new(), 0);
                player.display_track = None;
                player.elapsed_ms = 0;
                player.seek_offset_ms = 0;
                player.play_started_at = None;
                player.advance_stream_generation();
                player.confirmed_generation = None;
                player.prefetched_generation = None;
                player.suppress_track_finished = true;
                player.retire_tcp_session();
            }
            // Stop admission and retire reader actions at the FIRST reset,
            // while retained writers remain available for final stop/flush.
            self.shutdown_flag.store(true, Ordering::Relaxed);
            map.keys().map(ToString::to_string).collect::<Vec<_>>()
        };
        self.streaming.clear().await;
        // Wake current pending polls before network writes: a backpressured
        // TCP peer cannot consume the whole budget before subscribers wake.
        for mac in macs {
            self.cometd.notify_player_status(&mac).await;
        }
    }

    async fn stop_connected_players(&self, deadline: tokio::time::Instant) {
        if let Ok(mut map) = tokio::time::timeout_at(deadline, self.players.lock()).await {
            let writes = futures::future::join_all(map.values_mut()
                .filter(|player| player.has_tcp_writer())
                .map(|player| async move {
                    if let Err(error) = player.stop().await {
                        tracing::debug!("Squeeze shutdown: stop write failed for {}: {}", player.mac, error);
                    }
                    // Attempt flush even when stop fails; neither write is a
                    // hardware acknowledgement or a promise to drain its DAC.
                    if let Err(error) = player.flush().await {
                        tracing::debug!("Squeeze shutdown: flush write failed for {}: {}", player.mac, error);
                    }
                }));
            if tokio::time::timeout_at(deadline, writes).await.is_err() {
                tracing::warn!("Squeeze shutdown: deadline reached sending remote stop/flush");
            }
            // Remove entries before cached HTTP helpers or queued TCP actions
            // can acquire the guard again. The final streaming cleanup also
            // discards entries requeued by helpers that already cached a URL.
            map.clear();
            self.shutdown_flag.store(true, Ordering::Relaxed);
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

fn start_cli_server_with_listener(
    cli_listener: TcpListener,
    cli_shutdown: Arc<AtomicBool>,
) -> JoinHandle<()> {
    start_cli_server_with_listener_owned(cli_listener, cli_shutdown, new_connection_tasks())
}

fn start_cli_server_with_listener_owned(
    cli_listener: TcpListener,
    cli_shutdown: Arc<AtomicBool>,
    connections: ConnectionTasks,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        tracing::info!("Squeeze CLI: listening on port 9090");
        let mut connections = connections.lock().await;
        let shutdown_wait = async {
            while !cli_shutdown.load(Ordering::Relaxed) {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        };
        tokio::pin!(shutdown_wait);
        loop {
            tokio::select! {
                biased;
                _ = &mut shutdown_wait => break,
                _ = connections.join_next(), if !connections.is_empty() => {},
                result = cli_listener.accept() => {
                    match result {
                        Ok((mut socket, addr)) => {
                            tracing::info!("Squeeze CLI: connection from {}", addr);
                            connections.spawn(async move {
                                use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
                                let (reader, mut writer) = socket.split();
                                let mut lines = BufReader::new(reader).lines();
                                while let Ok(Some(line)) = lines.next_line().await {
                                    tracing::info!("Squeeze CLI: recv from {}: {}", addr, line);
                                    // Respond with empty result for now — just keeping the connection alive
                                    let _ = writer.write_all(b"\n").await;
                                }
                                tracing::info!("Squeeze CLI: disconnected {}", addr);
                            });
                        }
                        Err(e) => {
                            tracing::error!("Squeeze CLI: accept error: {}", e);
                        }
                    }
                }
            }
        }
        drop(cli_listener);
        connections.shutdown().await;
    })
}

/// Bind a TCP listener with SO_REUSEADDR so the port can be reused immediately after stop.
async fn bind_tcp_reuse(addr: &str) -> std::io::Result<TcpListener> {
    let addr: std::net::SocketAddr = addr.parse().unwrap();
    let socket = TcpSocket::new_v4()?;
    socket.set_reuseaddr(true)?;
    socket.bind(addr)?;
    socket.listen(128)
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use crate::squeeze::codec::MacAddress;
    use crate::squeeze::player::{PlayerState, SqueezePlayer};
    use crate::squeeze::queue::QueueTrack;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    use tokio::sync::oneshot;

    const MAC: MacAddress = MacAddress([2, 0, 0, 0, 0, 1]);

    async fn start_test_services(server: &mut SqueezeServer) -> [std::net::SocketAddr; 4] {
        let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
        let tcp = bind_tcp_reuse("127.0.0.1:0").await.unwrap();
        let http = bind_tcp_reuse("127.0.0.1:0").await.unwrap();
        let cli = bind_tcp_reuse("127.0.0.1:0").await.unwrap();
        let addresses = [udp.local_addr().unwrap(), tcp.local_addr().unwrap(),
            http.local_addr().unwrap(), cli.local_addr().unwrap()];
        server.start_services(udp, tcp, http, cli);
        addresses
    }

    async fn connect_hardware(addr: std::net::SocketAddr) -> TcpStream {
        connect_hardware_mac(addr, MAC).await
    }

    async fn connect_hardware_mac(addr: std::net::SocketAddr, mac: MacAddress) -> TcpStream {
        let socket = TcpSocket::new_v4().unwrap();
        socket.set_recv_buffer_size(1024).unwrap();
        let mut socket = socket.connect(addr).await.unwrap();
        let mut payload = vec![1, 1];
        payload.extend_from_slice(&mac.0);
        let mut helo = b"HELO".to_vec();
        helo.extend_from_slice(&8u32.to_be_bytes());
        helo.extend_from_slice(&payload);
        socket.write_all(&helo).await.unwrap();
        // The initial handshake already contains a stop. Drain all five frames
        // so only commands emitted by stop() can satisfy the regression test.
        tokio::time::timeout(Duration::from_secs(2), async {
            for _ in 0..5 {
                let mut len = [0; 2];
                socket.read_exact(&mut len).await.unwrap();
                let mut frame = vec![0; u16::from_be_bytes(len) as usize];
                socket.read_exact(&mut frame).await.unwrap();
            }
        }).await.expect("hardware handshake timed out");
        socket
    }

    async fn write_cometd(socket: &mut TcpStream, request: serde_json::Value) {
        let body = serde_json::to_vec(&request).unwrap();
        let headers = format!(
            "POST /cometd HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            body.len(),
        );
        socket.write_all(headers.as_bytes()).await.unwrap();
        socket.write_all(&body).await.unwrap();
    }

    async fn read_http_json(socket: &mut TcpStream) -> serde_json::Value {
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut headers = Vec::new();
            while !headers.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                socket.read_exact(&mut byte).await.expect("response ended before complete headers");
                headers.push(byte[0]);
            }
            let headers = String::from_utf8(headers).unwrap();
            assert!(headers.starts_with("HTTP/1.1 200"), "{headers}");
            let length = headers.lines().find_map(|line| {
                line.strip_prefix("Content-Length: ").map(|length| length.parse::<usize>().unwrap())
            }).expect("response must retain Content-Length framing");
            let mut body = vec![0; length];
            socket.read_exact(&mut body).await.expect("response ended before complete JSON body");
            serde_json::from_slice(&body).unwrap()
        }).await.expect("complete HTTP response timed out")
    }

    #[tokio::test]
    async fn stop_sends_hardware_stop_and_flush_before_tcp_eof() {
        let mut server = SqueezeServer::new();
        let addresses = start_test_services(&mut server).await;
        let mut hardware = connect_hardware(addresses[1]).await;

        server.stop().await;

        let mut wire = Vec::new();
        tokio::time::timeout(Duration::from_millis(200), hardware.read_to_end(&mut wire))
            .await.unwrap().unwrap();
        // Independently hand-derived SlimProto frames: u16 length, tag, command,
        // then 23 zero bytes. Successful writes are not a hardware ACK.
        let mut expected = b"\x00\x1cstrmq".to_vec();
        expected.extend_from_slice(&[0; 23]);
        expected.extend_from_slice(b"\x00\x1cstrmf");
        expected.extend_from_slice(&[0; 23]);
        assert_eq!(wire, expected, "transport closed without stop then flush");
        let _replacement = bind_tcp_reuse(&addresses[1].to_string()).await.unwrap();
    }

    #[tokio::test]
    async fn stop_completes_pending_http_poll_with_stopped_empty_status() {
        let mut server = SqueezeServer::new();
        let addresses = start_test_services(&mut server).await;
        let _hardware = connect_hardware(addresses[1]).await;
        let mut http = TcpStream::connect(addresses[2]).await.unwrap();
        write_cometd(&mut http, serde_json::json!([{
            "channel": "/meta/handshake", "id": "1", "ext": {"mac": MAC.to_string()}
        }])).await;
        let handshake = read_http_json(&mut http).await;
        let client_id = handshake[0]["clientId"].as_str().unwrap().to_string();
        {
            let mut map = server.players.lock().await;
            let player = map.get_mut(&MAC).unwrap();
            let track = QueueTrack { id: 7, title: "Buffered track".into(),
                artist: "Artist".into(), album: "Album".into(), path: "unused.flac".into(),
                duration: 120.0, format: "flac".into() };
            player.queue.set_tracks(vec![track.clone()], 0);
            player.display_track = Some(track);
            player.state = PlayerState::Playing;
            player.elapsed_ms = 30_000;
            player.seek_offset_ms = 10_000;
            player.confirmed_generation = Some(7);
            player.prefetched_generation = Some(8);
            player.play_started_at = Some(std::time::Instant::now());
        }
        server.streaming.queue_file(&MAC, "unused.flac".into(), 7, 0).await;
        write_cometd(&mut http, serde_json::json!([{
            "channel": "/slim/subscribe", "clientId": client_id, "id": "2",
            "data": {"request": [MAC.to_string(), ["status", "-", 1]], "response": "/status/test"}
        }])).await;
        let initial = read_http_json(&mut http).await;
        assert!(initial.as_array().unwrap().iter().any(|message|
            message["data"]["mode"] == "play" && message["data"]["track_id"] == 7));
        let before = server.cometd.clients.lock().await[&client_id].last_seen;
        write_cometd(&mut http, serde_json::json!([{
            "channel": "/meta/connect", "clientId": client_id, "id": "3"
        }])).await;
        // An observed liveness update proves the real HTTP handler entered.
        // Notify retains a permit if stop races the next step into long-poll.
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if server.cometd.clients.lock().await[&client_id].last_seen != before { break; }
                tokio::task::yield_now().await;
            }
        }).await.unwrap();
        assert_eq!(http.try_read(&mut [0; 1]).unwrap_err().kind(), std::io::ErrorKind::WouldBlock);

        server.stop().await;

        let response = read_http_json(&mut http).await;
        let status = response.as_array().unwrap().iter()
            .find(|message| message["channel"] == "/status/test")
            .expect("pending poll did not receive the final status");
        assert_eq!(status["data"]["mode"], "stop");
        assert_eq!(status["data"]["playlist_tracks"], 0);
        assert_eq!(status["data"]["track_id"], 0);
        assert_eq!(status["data"]["time"], 0.0);
        assert_eq!(status["data"]["duration"], 0.0);
        assert!(status["data"]["item_loop"].as_array().unwrap().is_empty());
        assert!(server.players.lock().await.is_empty());
        assert!(server.cometd.clients.lock().await.is_empty());
        assert!(server.streaming.get_entry(&MAC.to_string()).await.is_none());
        let mut remaining = Vec::new();
        tokio::time::timeout(Duration::from_millis(200), http.read_to_end(&mut remaining))
            .await.unwrap().unwrap();
        let _replacement = bind_tcp_reuse(&addresses[2].to_string()).await.unwrap();
    }

    #[tokio::test]
    async fn stop_deadline_includes_unavailable_player_state_and_restart_is_clean() {
        let mut server = SqueezeServer::new();
        server.players.lock().await.insert(MAC, SqueezePlayer::new_cometd(MAC, MAC.to_string(), "old".into()));
        let players = server.players.clone();
        let held_map = players.lock().await;

        tokio::time::timeout(Duration::from_secs(3), server.stop()).await
            .expect("stop exceeded its total deadline waiting for player state");

        drop(held_map);
        let _addresses = start_test_services(&mut server).await;
        assert!(server.players.lock().await.is_empty(), "restart reused locked stale state");
        server.stop().await;
    }

    #[tokio::test]
    async fn stop_does_not_let_backpressured_hardware_block_other_players() {
        let mut server = SqueezeServer::new();
        let addresses = start_test_services(&mut server).await;
        let slow_mac = MacAddress([2, 0, 0, 0, 0, 2]);
        let peer_listener = TcpSocket::new_v4().unwrap();
        peer_listener.set_recv_buffer_size(1024).unwrap();
        peer_listener.bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let peer_listener = peer_listener.listen(1).unwrap();
        let sender = TcpSocket::new_v4().unwrap();
        sender.set_send_buffer_size(1024).unwrap();
        let sender = sender.connect(peer_listener.local_addr().unwrap()).await.unwrap();
        let (_unread_peer, _) = peer_listener.accept().await.unwrap();
        let mut healthy = connect_hardware(addresses[1]).await;
        // Fill until write readiness itself stays blocked, not merely until a
        // chosen bulk send completes or a cooperative loop hits a timer.
        let fill_deadline = tokio::time::Instant::now() + Duration::from_secs(1);
        let payload = vec![0; 128 * 1024];
        let mut sent = 0;
        let backpressured = loop {
            if tokio::time::timeout_at(fill_deadline, sender.writable()).await.is_err() { break true; }
            match sender.try_write(&payload) {
                Ok(bytes) => {
                    sent += bytes;
                    if sent > 128 * 1024 * 1024 { break false; }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {},
                Err(error) => panic!("backpressure fixture write failed: {error}"),
            }
        };
        if !backpressured {
            server.stop().await;
            panic!("fixture did not produce TCP backpressure after {sent} bytes");
        }
        let (_sender_reader, writer) = sender.into_split();
        server.players.lock().await.insert(slow_mac, SqueezePlayer::new(
            slow_mac, "Unread peer".into(), String::new(), writer, std::net::Ipv4Addr::LOCALHOST));
        let stop_task = tokio::spawn(async move {
            server.stop().await;
            server
        });

        let mut frames = [0; 60];
        let received = tokio::time::timeout(Duration::from_millis(500), healthy.read_exact(&mut frames)).await;
        let server = tokio::time::timeout(Duration::from_secs(3), stop_task)
            .await.expect("backpressured stop exceeded the shared deadline").unwrap();
        received.expect("a blocked peer prevented the healthy peer's stop").unwrap();
        assert_eq!(&frames[..7], b"\x00\x1cstrmq");
        assert_eq!(&frames[30..37], b"\x00\x1cstrmf");
        assert!(server.players.lock().await.is_empty());
        let mut remaining = Vec::new();
        tokio::time::timeout(Duration::from_millis(200), healthy.read_to_end(&mut remaining))
            .await.unwrap().unwrap();
        let _replacement = bind_tcp_reuse(&addresses[1].to_string()).await.unwrap();
    }

    #[tokio::test]
    async fn http_shutdown_drains_an_in_flight_poll_before_closing_connection() {
        let mut server = SqueezeServer::new();
        let listener = bind_tcp_reuse("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let streaming = server.streaming.clone();
        let cometd = server.cometd.clone();
        let shutdown = server.shutdown_flag.clone();
        let mut http_task = tokio::spawn(async move {
            streaming::start_streaming_server_with_listener(listener, streaming, cometd, shutdown)
                .await.unwrap();
        });
        let mut http = TcpStream::connect(addr).await.unwrap();
        write_cometd(&mut http, serde_json::json!([{
            "channel": "/meta/handshake", "ext": {"mac": MAC.to_string()}
        }])).await;
        let client_id = read_http_json(&mut http).await[0]["clientId"].as_str().unwrap().to_string();
        write_cometd(&mut http, serde_json::json!([{
            "channel": "/slim/subscribe", "clientId": client_id,
            "data": {"request": [MAC.to_string(), ["status", "-", 1]], "response": "/status/test"}
        }])).await;
        read_http_json(&mut http).await;
        let before = server.cometd.clients.lock().await[&client_id].last_seen;
        write_cometd(&mut http, serde_json::json!([{
            "channel": "/meta/connect", "clientId": client_id
        }])).await;
        tokio::time::timeout(Duration::from_secs(2), async {
            while server.cometd.clients.lock().await[&client_id].last_seen == before {
                tokio::task::yield_now().await;
            }
        }).await.unwrap();

        server.shutdown_flag.store(true, Ordering::Relaxed);
        // A pending request must keep the owning HTTP task alive through its
        // shutdown poll. Delivery is asserted from complete response bytes below.
        assert!(tokio::time::timeout(Duration::from_millis(150), &mut http_task).await.is_err(),
            "HTTP shutdown discarded the in-flight request instead of draining it");
        server.cometd.notify_player_status(&MAC.to_string()).await;

        let response = read_http_json(&mut http).await;
        assert!(response.as_array().unwrap().iter().any(|message|
            message["channel"] == "/status/test" && message["data"]["mode"] == "stop"));
        tokio::time::timeout(Duration::from_millis(500), http_task).await
            .expect("HTTP shutdown retained an idle connection after responding").unwrap();
        server.stop().await;
    }

    #[tokio::test]
    async fn stop_aborts_and_awaits_timed_out_service_tasks() {
        let mut server = SqueezeServer::new();
        let (started_tx, started_rx) = oneshot::channel();
        let (resource_tx, mut resource_rx) = oneshot::channel::<()>();
        server.handles.push(tokio::spawn(async move {
            let _owned_resource = resource_tx;
            started_tx.send(()).unwrap();
            std::future::pending::<()>().await;
        }));
        started_rx.await.unwrap();

        server.stop().await;

        assert!(
            matches!(resource_rx.try_recv(), Err(oneshot::error::TryRecvError::Closed)),
            "stop returned while its timed-out service still owned resources"
        );
        assert!(server.handles.is_empty());
        assert!(!server.is_running());
    }

    #[tokio::test]
    async fn expired_grace_joins_pending_http_resource_drops_before_stop_returns() {
        let mut server = SqueezeServer::new();
        let addresses = start_test_services(&mut server).await;
        let mut control = TcpStream::connect(addresses[2]).await.unwrap();
        write_cometd(&mut control, serde_json::json!([{
            "channel": "/meta/handshake", "ext": {"mac": MAC.to_string()}
        }])).await;
        let client_id = read_http_json(&mut control).await[0]["clientId"].as_str().unwrap().to_string();
        let notify = server.cometd.clients.lock().await[&client_id].notify.clone();
        let mut polls = Vec::new();
        for _ in 0..32 {
            let mut poll = TcpStream::connect(addresses[2]).await.unwrap();
            write_cometd(&mut poll, serde_json::json!([{
                "channel": "/meta/connect", "clientId": client_id
            }])).await;
            polls.push(poll);
        }
        // Each entered real long-poll owns exactly one Notify clone. There is
        // no status subscription to wake these requests during normal stop.
        tokio::time::timeout(Duration::from_secs(2), async {
            while Arc::strong_count(&notify) != polls.len() + 2 {
                tokio::task::yield_now().await;
            }
        }).await.expect("not all HTTP polls entered their pending handler");

        tokio::time::timeout(Duration::from_secs(3), server.stop()).await.unwrap();

        // No await or client read before this assertion: later EOF is not
        // proof that the service's children were dropped before stop returned.
        assert_eq!(Arc::strong_count(&notify), 1,
            "stop returned while a canceled HTTP handler still owned its Notify");
    }

    #[tokio::test]
    async fn forced_stop_waits_for_owned_child_cancellation_completion() {
        let mut server = SqueezeServer::new();
        let connections = new_connection_tasks();
        server.connection_tasks.push(connections.clone());
        let (runtime_tx, runtime_rx) = std::sync::mpsc::channel();
        let (pause_tx, pause_rx) = oneshot::channel();
        let (paused_tx, paused_rx) = oneshot::channel();
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        let child_worker = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            runtime_tx.send(runtime.handle().clone()).unwrap();
            runtime.block_on(async {
                pause_rx.await.unwrap();
                paused_tx.send(()).unwrap();
                // Pause the scheduler, not the ordinary pending child future.
                // This makes cancellation request vs completion observable.
                let _ = resume_rx.recv();
                tokio::task::yield_now().await;
            });
        });
        let child_runtime = runtime_rx.recv().unwrap();
        let (entered_tx, entered_rx) = oneshot::channel();
        let (resource_tx, mut resource_rx) = oneshot::channel::<()>();
        server.handles.push(tokio::spawn(async move {
            let mut children = connections.lock().await;
            children.spawn_on(async move {
                let _owned_resource = resource_tx;
                entered_tx.send(()).unwrap();
                std::future::pending::<()>().await;
            }, &child_runtime);
            std::future::pending::<()>().await;
        }));
        entered_rx.await.unwrap();
        pause_tx.send(()).unwrap();
        paused_rx.await.unwrap();
        let mut stopping = tokio::spawn(async move { server.stop().await; });

        let returned_before_resume = tokio::time::timeout(Duration::from_millis(2100), &mut stopping)
            .await.is_ok();
        let resource_dropped_before_resume =
            matches!(resource_rx.try_recv(), Err(oneshot::error::TryRecvError::Closed));
        resume_tx.send(()).unwrap();
        if !returned_before_resume {
            tokio::time::timeout(Duration::from_secs(1), stopping).await.unwrap().unwrap();
        }
        child_worker.join().unwrap();

        assert!(!returned_before_resume && !resource_dropped_before_resume,
            "stop returned before its ordinary child could execute cancellation and drop its resource");
        assert!(matches!(resource_rx.try_recv(), Err(oneshot::error::TryRecvError::Closed)),
            "stop completed without acknowledging the child's resource drop");
    }

    #[tokio::test]
    async fn stop_uses_one_deadline_for_all_service_tasks() {
        let mut server = SqueezeServer::new();
        for _ in 0..3 {
            server.handles.push(tokio::spawn(std::future::pending::<()>()));
        }

        assert!(
            tokio::time::timeout(Duration::from_secs(3), server.stop()).await.is_ok(),
            "stop applied the full timeout separately to each service"
        );
    }

    #[tokio::test]
    async fn stop_closes_http_listener_and_idle_connection_before_returning() {
        let mut server = SqueezeServer::new();
        let listener = bind_tcp_reuse("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let streaming = server.streaming.clone();
        let cometd = server.cometd.clone();
        let shutdown = server.shutdown_flag.clone();
        server.handles.push(tokio::spawn(async move {
            streaming::start_streaming_server_with_listener(listener, streaming, cometd, shutdown)
                .await
                .unwrap();
        }));
        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client.write_all(b"GET /status HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
        // A complete response proves the connection task has accepted this socket.
        let mut byte = [0];
        let mut headers = Vec::new();
        while !headers.ends_with(b"\r\n\r\n") {
            tokio::time::timeout(Duration::from_secs(2), client.read_exact(&mut byte))
                .await.unwrap().unwrap();
            headers.push(byte[0]);
        }
        assert!(String::from_utf8_lossy(&headers).starts_with("HTTP/1.1 200"));
        assert!(String::from_utf8_lossy(&headers).contains("Content-Length:"));

        server.stop().await;

        let mut remaining = Vec::new();
        assert!(
            tokio::time::timeout(Duration::from_millis(200), client.read_to_end(&mut remaining))
                .await.is_ok(),
            "HTTP connection task survived stop"
        );
        let replacement = bind_tcp_reuse(&addr.to_string()).await.unwrap();
        drop(replacement);
    }

    #[tokio::test]
    async fn stop_closes_cli_listener_and_idle_connection_before_returning() {
        let mut server = SqueezeServer::new();
        let listener = bind_tcp_reuse("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        server.handles.push(start_cli_server_with_listener(listener, server.shutdown_flag.clone()));
        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client.write_all(b"version ?\n").await.unwrap();
        let mut response = [0];
        tokio::time::timeout(Duration::from_secs(2), client.read_exact(&mut response))
            .await.unwrap().unwrap();
        assert_eq!(response, *b"\n");

        server.stop().await;

        let mut remaining = Vec::new();
        assert!(
            tokio::time::timeout(Duration::from_millis(200), client.read_to_end(&mut remaining))
                .await.is_ok(),
            "CLI connection task survived stop"
        );
        let replacement = bind_tcp_reuse(&addr.to_string()).await.unwrap();
        drop(replacement);
    }

    #[tokio::test]
    async fn two_stop_start_cycles_release_every_service_port() {
        let mut server = SqueezeServer::new();
        let mut addresses = ["127.0.0.1:0".parse::<std::net::SocketAddr>().unwrap(); 4];
        let mut previous_shutdown: Option<Arc<AtomicBool>> = None;

        // The second cycle must reuse every port released by the first.
        for _ in 0..2 {
            let udp = UdpSocket::bind(addresses[0]).unwrap();
            let tcp = bind_tcp_reuse(&addresses[1].to_string()).await.unwrap();
            let http = bind_tcp_reuse(&addresses[2].to_string()).await.unwrap();
            let cli = bind_tcp_reuse(&addresses[3].to_string()).await.unwrap();
            addresses = [udp.local_addr().unwrap(), tcp.local_addr().unwrap(),
                http.local_addr().unwrap(), cli.local_addr().unwrap()];
            server.start_services(udp, tcp, http, cli);
            assert!(server.is_running());
            if let Some(shutdown) = &previous_shutdown {
                assert!(shutdown.load(Ordering::Relaxed), "restart revived the prior run");
            }
            previous_shutdown = Some(server.shutdown_flag.clone());

            // A real TLV reply proves the blocking discovery worker is running,
            // not merely a queued task whose socket has never been read.
            let probe = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
            probe.send_to(b"e", addresses[0]).await.unwrap();
            let mut reply = [0; 256];
            tokio::time::timeout(Duration::from_secs(2), probe.recv_from(&mut reply))
                .await.unwrap().unwrap();
            assert_eq!(reply[0], b'E');

            tokio::time::timeout(Duration::from_secs(3), server.stop()).await.unwrap();
            assert!(!server.is_running());
            assert!(previous_shutdown.as_ref().unwrap().load(Ordering::Relaxed));
        }

        // The final stop must also release UDP's non-abortable blocking worker.
        let _udp = UdpSocket::bind(addresses[0]).unwrap();
        let _tcp = bind_tcp_reuse(&addresses[1].to_string()).await.unwrap();
        let _http = bind_tcp_reuse(&addresses[2].to_string()).await.unwrap();
        let _cli = bind_tcp_reuse(&addresses[3].to_string()).await.unwrap();
    }
}
