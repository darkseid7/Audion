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
            handles: Vec::new(),
        }
    }

    /// Start all Squeeze services (UDP discovery, TCP SlimProto, HTTP streaming).
    /// Pre-binds all sockets so failures are reported immediately.
    pub async fn start(&mut self) -> Result<(), String> {
        if self.running.load(Ordering::Relaxed) {
            return Err("Squeeze server already running".into());
        }

        self.shutdown_flag.store(false, Ordering::Relaxed);

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

        // ── All sockets bound — now spawn the tasks ─────────────────────

        // 1. UDP discovery (pass pre-bound socket)
        let (discovery_handle, discovery_shutdown) =
            discovery::start_discovery_with_socket(udp_socket, server::HTTP_PORT, "Audion".to_string());
        self.handles.push(discovery_handle);
        let sf = self.shutdown_flag.clone();
        self.handles.push(tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                if sf.load(Ordering::Relaxed) {
                    discovery_shutdown.store(true, Ordering::Relaxed);
                    break;
                }
            }
        }));

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
        let http_handle = tokio::spawn(async move {
            if let Err(e) = streaming::start_streaming_server_with_listener(http_listener, streaming_state, cometd_state).await {
                tracing::error!("Squeeze HTTP server failed: {}", e);
            }
        });
        self.handles.push(http_handle);

        // 4. CLI server (port 9090) — minimal handler for Squeezer-type controllers
        let cli_shutdown = self.shutdown_flag.clone();
        let cli_handle = tokio::spawn(async move {
            tracing::info!("Squeeze CLI: listening on port 9090");
            loop {
                tokio::select! {
                    result = cli_listener.accept() => {
                        match result {
                            Ok((mut socket, addr)) => {
                                tracing::info!("Squeeze CLI: connection from {}", addr);
                                tokio::spawn(async move {
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
                    _ = async {
                        while !cli_shutdown.load(Ordering::Relaxed) {
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        }
                    } => {
                        break;
                    }
                }
            }
        });
        self.handles.push(cli_handle);

        self.running.store(true, Ordering::Relaxed);
        tracing::info!("Squeeze server started (UDP 3483, TCP 3483, HTTP {}, CLI 9090)", server::HTTP_PORT);

        Ok(())
    }

    /// Stop all Squeeze services.
    pub async fn stop(&mut self) {
        self.shutdown_flag.store(true, Ordering::Relaxed);

        // Give tasks a moment to notice the shutdown flag and exit gracefully,
        // dropping their listeners so ports are freed immediately.
        for handle in self.handles.drain(..) {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;
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

/// Bind a TCP listener with SO_REUSEADDR so the port can be reused immediately after stop.
async fn bind_tcp_reuse(addr: &str) -> std::io::Result<TcpListener> {
    let addr: std::net::SocketAddr = addr.parse().unwrap();
    let socket = TcpSocket::new_v4()?;
    socket.set_reuseaddr(true)?;
    socket.bind(addr)?;
    socket.listen(128)
}
