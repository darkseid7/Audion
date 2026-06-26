// UDP discovery responder for Squeeze players.
//
// Players broadcast a single byte 'e' (TLV) or 'd' (legacy) to UDP port 3483.
// We respond with our server IP so they can initiate a TCP SlimProto connection.

use crate::squeeze::player::PlayerMap;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

/// Stable UUID for this Audion Squeeze server instance.
const SERVER_UUID: &str = "audion-squeeze-server-0001";

/// Build a TLV 'E' response matching the LMS format.
/// Tags: NAME (server name), JSON (HTTP port), UUID (server id).
/// The player uses the UDP source IP to determine the server address.
fn build_tlv_response(server_name: &str, http_port: u16) -> Vec<u8> {
    let mut buf = Vec::with_capacity(128);
    buf.push(b'E'); // TLV response marker

    // NAME tag
    let name_bytes = server_name.as_bytes();
    buf.extend_from_slice(b"NAME");
    buf.push(name_bytes.len() as u8);
    buf.extend_from_slice(name_bytes);

    // JSON tag — HTTP/JSONRPC port (matches LMS convention)
    let port_str = http_port.to_string();
    let port_bytes = port_str.as_bytes();
    buf.extend_from_slice(b"JSON");
    buf.push(port_bytes.len() as u8);
    buf.extend_from_slice(port_bytes);

    // UUID tag — required by some players (e.g. Eversolo)
    let uuid_bytes = SERVER_UUID.as_bytes();
    buf.extend_from_slice(b"UUID");
    buf.push(uuid_bytes.len() as u8);
    buf.extend_from_slice(uuid_bytes);

    buf
}

/// Build a legacy 'D' response (18 bytes: 'D' + hostname padded to 17 bytes).
fn build_legacy_response(server_name: &str) -> Vec<u8> {
    let mut buf = vec![0u8; 18];
    buf[0] = b'D';
    let name_bytes = server_name.as_bytes();
    let len = name_bytes.len().min(17);
    buf[1..1 + len].copy_from_slice(&name_bytes[..len]);
    buf
}

/// Start the UDP discovery responder using a pre-bound socket.
/// Returns a handle and a shutdown flag.
pub fn start_discovery_with_socket(
    socket: UdpSocket,
    http_port: u16,
    server_name: String,
) -> (JoinHandle<()>, Arc<AtomicBool>) {
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    let handle = tokio::task::spawn_blocking(move || {
        // Non-blocking with 500ms timeout so we can check the shutdown flag
        socket
            .set_read_timeout(Some(std::time::Duration::from_millis(500)))
            .ok();

        tracing::info!("Squeeze discovery: listening on UDP 3483");

        let mut buf = [0u8; 4096];
        while !shutdown_clone.load(Ordering::Relaxed) {
            let (len, src) = match socket.recv_from(&mut buf) {
                Ok(r) => r,
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
                Err(e) => {
                    tracing::warn!("Squeeze discovery recv error: {}", e);
                    continue;
                }
            };

            if len == 0 {
                continue;
            }

            tracing::info!(
                "Squeeze discovery: packet from {} len={} first_byte=0x{:02x}",
                src, len, buf[0]
            );

            match buf[0] {
                b'e' => {
                    let resp = build_tlv_response(&server_name, http_port);
                    tracing::info!("Squeeze discovery: responding to TLV query from {} ({} bytes)", src, resp.len());
                    let _ = socket.send_to(&resp, src);
                }
                b'd' => {
                    let resp = build_legacy_response(&server_name);
                    tracing::info!("Squeeze discovery: responding to legacy query from {}", src);
                    let _ = socket.send_to(&resp, src);
                }
                other => {
                    tracing::info!("Squeeze discovery: unknown byte 0x{:02x} from {} (len={})", other, src, len);
                }
            }
        }

        tracing::info!("Squeeze discovery: stopped");
    });

    (handle, shutdown)
}

/// Periodically broadcast the TLV discovery response to 255.255.255.255:3483
/// for a short window after the squeeze server starts. This is a proactive
/// "I'm here" beacon — it lets any Squeeze player on the LAN that lost its
/// TCP connection (e.g. the user closed and reopened Audion while their
/// Eversolo was still on) rediscover us without having to manually trigger
/// a rescan from the device.
///
/// Players normally only probe the network when they boot or when the user
/// explicitly asks for a rescan. Many of them, however, accept unsolicited
/// TLV 'E' responses as a fresh announcement and initiate a reconnect on
/// their own. Sending the same 'E' TLV we use for unicast replies keeps
/// the protocol consistent.
///
/// Stops early if a player connects (so we don't spam the network) or
/// after `BROADCAST_DURATION` elapses. Uses a separate UDP socket from the
/// listener so it can have SO_BROADCAST set without affecting normal
/// recv behavior.
const BROADCAST_INTERVAL: Duration = Duration::from_secs(3);
const BROADCAST_DURATION: Duration = Duration::from_secs(60);

pub fn start_discovery_broadcaster(
    http_port: u16,
    server_name: String,
    players: PlayerMap,
    shutdown: Arc<AtomicBool>,
) -> JoinHandle<()> {
    tokio::task::spawn(async move {
        // Use a separate socket so SO_BROADCAST doesn't leak into the
        // listener, and so the broadcaster doesn't tie up port 3483.
        let socket = match UdpSocket::bind("0.0.0.0:0") {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    "Squeeze discovery broadcaster: bind failed: {} (auto-reconnect beacon disabled)",
                    e
                );
                return;
            }
        };
        if let Err(e) = socket.set_broadcast(true) {
            tracing::warn!(
                "Squeeze discovery broadcaster: SO_BROADCAST failed: {} (auto-reconnect beacon disabled)",
                e
            );
            return;
        }

        let broadcast_addr: SocketAddr = "255.255.255.255:3483".parse().unwrap();
        let response = build_tlv_response(&server_name, http_port);
        let started = Instant::now();

        // Fire the first beacon immediately so a player that's listening
        // at this exact moment doesn't have to wait for the next tick.
        match socket.send_to(&response, &broadcast_addr) {
            Ok(_) => tracing::info!(
                "Squeeze discovery: broadcast beacon sent to {} (auto-reconnect beacon enabled for {}s)",
                broadcast_addr,
                BROADCAST_DURATION.as_secs()
            ),
            Err(e) => tracing::warn!("Squeeze discovery: initial broadcast failed: {}", e),
        }

        loop {
            tokio::time::sleep(BROADCAST_INTERVAL).await;
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            if started.elapsed() > BROADCAST_DURATION {
                tracing::info!("Squeeze discovery: broadcaster stopped after timeout");
                break;
            }
            // Stop early once a player is registered — no need to keep
            // announcing ourselves.
            {
                let map = players.lock().await;
                if !map.is_empty() {
                    tracing::info!(
                        "Squeeze discovery: player connected, stopping broadcaster"
                    );
                    break;
                }
            }
            if let Err(e) = socket.send_to(&response, &broadcast_addr) {
                tracing::debug!("Squeeze discovery: broadcast failed: {}", e);
            }
        }

        tracing::info!("Squeeze discovery: broadcaster stopped");
    })
}
