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

/// Top-level Squeeze server state, managed by Tauri.
pub struct SqueezeServer {
    pub players: PlayerMap,
    pub streaming: StreamingState,
    pub cometd: CometdState,
    pub running: Arc<AtomicBool>,
    shutdown_flag: Arc<AtomicBool>,
    discovery_shutdown: Option<Arc<AtomicBool>>,
    handles: Vec<JoinHandle<()>>,
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
        let tcp_handle = server::start_slimproto_server_with_listener(
            tcp_listener,
            self.players.clone(),
            self.streaming.clone(),
            self.cometd.clone(),
            self.shutdown_flag.clone(),
        );
        self.handles.push(tcp_handle);

        // 3. HTTP streaming server (pass pre-bound listener)
        let streaming_state = self.streaming.clone();
        let cometd_state = self.cometd.clone();
        let http_shutdown = self.shutdown_flag.clone();
        let http_handle = tokio::spawn(async move {
            if let Err(e) = streaming::start_streaming_server_with_listener(http_listener, streaming_state, cometd_state, http_shutdown).await {
                tracing::error!("Squeeze HTTP server failed: {}", e);
            }
        });
        self.handles.push(http_handle);

        // 4. CLI server (port 9090) — minimal handler for Squeezer-type controllers
        self.handles.push(start_cli_server_with_listener(
            cli_listener,
            self.shutdown_flag.clone(),
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
        self.shutdown_flag.store(true, Ordering::Relaxed);
        // Discovery runs on a blocking thread, so abort alone cannot stop it.
        if let Some(shutdown) = self.discovery_shutdown.take() {
            shutdown.store(true, Ordering::Relaxed);
        }

        // All services share one grace period. Keep each handle until it exits;
        // dropping a timed-out JoinHandle would detach its task instead.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        for mut handle in self.handles.drain(..) {
            if tokio::time::timeout_at(deadline, &mut handle).await.is_err() {
                handle.abort();
                let _ = handle.await;
            }
        }

        // Clear all player state
        let mut map = self.players.lock().await;
        map.clear();
        drop(map);

        // Clear CometD client state so restart is clean
        self.cometd.clients.lock().await.clear();
        self.cometd.mac_to_client.lock().await.clear();

        // Clear streaming queue
        self.streaming.clear().await;

        self.running.store(false, Ordering::Relaxed);
        tracing::info!("Squeeze server stopped");
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

fn start_cli_server_with_listener(
    cli_listener: TcpListener,
    cli_shutdown: Arc<AtomicBool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        tracing::info!("Squeeze CLI: listening on port 9090");
        let mut connections = tokio::task::JoinSet::new();
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
        connections.abort_all();
        while connections.join_next().await.is_some() {}
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
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::oneshot;

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
