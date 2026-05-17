// UDP discovery responder for Squeeze players.
//
// Players broadcast a single byte 'e' (TLV) or 'd' (legacy) to UDP port 3483.
// We respond with our server IP so they can initiate a TCP SlimProto connection.

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
