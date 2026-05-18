use crate::lms_client::types::{LmsError, LmsServer};
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::Duration;

const DISCOVERY_PORT: u16 = 3483;
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(2);

/// Discover LMS servers on the local network via UDP broadcast.
/// Also probes localhost as a fallback (common case: LMS on same machine).
pub async fn discover_servers() -> Result<Vec<LmsServer>, LmsError> {
    let mut servers = tokio::task::spawn_blocking(discover_broadcast)
        .await
        .map_err(|e| LmsError::Network(format!("Discovery task failed: {}", e)))??;

    // Also try localhost directly (UDP broadcast doesn't loop back on Windows)
    if let Ok(local) = probe_host("127.0.0.1", 9000).await {
        if !servers.iter().any(|s| s.host == "127.0.0.1") {
            servers.push(local);
        }
    }

    Ok(servers)
}

/// Probe a specific host to check if LMS is running there.
async fn probe_host(host: &str, port: u16) -> Result<LmsServer, LmsError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|e| LmsError::Network(e.to_string()))?;

    let url = format!("http://{}:{}/jsonrpc.js", host, port);
    let body = serde_json::json!({
        "id": 0,
        "method": "slim.request",
        "params": ["", ["serverstatus", "0", "0"]]
    });

    let resp: serde_json::Value = client
        .post(&url)
        .json(&body)
        .send()
        .await?
        .json()
        .await?;

    let result = resp.get("result").ok_or_else(|| LmsError::Parse("No result".into()))?;
    let name = result
        .get("uuid")
        .and_then(|u| u.as_str())
        .unwrap_or("Lyrion Music Server");
    let uuid = result
        .get("uuid")
        .and_then(|u| u.as_str())
        .unwrap_or("")
        .to_string();

    Ok(LmsServer {
        name: format!("LMS @ {}", host),
        host: host.to_string(),
        json_port: port,
        uuid,
    })
}

fn discover_broadcast() -> Result<Vec<LmsServer>, LmsError> {
    let socket = UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))
        .map_err(|e| LmsError::Network(format!("Failed to bind UDP socket: {}", e)))?;

    socket
        .set_broadcast(true)
        .map_err(|e| LmsError::Network(format!("Failed to set broadcast: {}", e)))?;

    socket
        .set_read_timeout(Some(DISCOVERY_TIMEOUT))
        .map_err(|e| LmsError::Network(format!("Failed to set timeout: {}", e)))?;

    let broadcast_addr = SocketAddr::from((Ipv4Addr::BROADCAST, DISCOVERY_PORT));
    socket
        .send_to(&[b'e'], broadcast_addr)
        .map_err(|e| LmsError::Network(format!("Failed to send discovery: {}", e)))?;

    let mut servers = Vec::new();
    let mut buf = [0u8; 4096];

    loop {
        match socket.recv_from(&mut buf) {
            Ok((len, src)) => {
                if len > 0 && buf[0] == b'E' {
                    if let Some(server) = parse_tlv_response(&buf[1..len], src) {
                        if !servers.iter().any(|s: &LmsServer| s.uuid == server.uuid) {
                            servers.push(server);
                        }
                    }
                }
            }
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                break;
            }
            Err(_) => break,
        }
    }

    Ok(servers)
}

fn parse_tlv_response(data: &[u8], src: SocketAddr) -> Option<LmsServer> {
    let mut name = String::new();
    let mut json_port: u16 = 9000;
    let mut uuid = String::new();

    let mut pos = 0;
    while pos + 4 < data.len() {
        let tag = std::str::from_utf8(&data[pos..pos + 4]).ok()?;
        pos += 4;

        if pos >= data.len() {
            break;
        }
        let len = data[pos] as usize;
        pos += 1;

        if pos + len > data.len() {
            break;
        }
        let value = std::str::from_utf8(&data[pos..pos + len]).unwrap_or("");
        pos += len;

        match tag {
            "NAME" => name = value.to_string(),
            "JSON" => json_port = value.parse().unwrap_or(9000),
            "UUID" => uuid = value.to_string(),
            _ => {}
        }
    }

    if name.is_empty() && uuid.is_empty() {
        return None;
    }

    Some(LmsServer {
        name: if name.is_empty() {
            format!("LMS @ {}", src.ip())
        } else {
            name
        },
        host: src.ip().to_string(),
        json_port,
        uuid,
    })
}
