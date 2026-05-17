// HTTP streaming server — serves audio files to Squeeze players.
//
// Listens on a configurable port (default 9000).
// Endpoint: GET /stream?player=<MAC>&gen=<generation>
// The player MAC maps to a queued file path set by the SlimProto server.

use crate::squeeze::codec::MacAddress;
use crate::squeeze::cometd::{self, CometdState};
use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Router;
use axum::Json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::io::ReaderStream;

/// Entry in the streaming queue for a player.
#[derive(Debug, Clone)]
pub struct StreamEntry {
    pub path: PathBuf,
    pub generation: u64,
    pub byte_offset: u64, // for seek support
}

/// Shared state for the streaming server.
#[derive(Debug, Clone)]
pub struct StreamingState {
    /// Per-player file queue keyed by MAC string.
    queue: Arc<Mutex<HashMap<String, StreamEntry>>>,
}

impl StreamingState {
    pub fn new() -> Self {
        Self {
            queue: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Queue a file for a player to stream.
    pub async fn queue_file(&self, mac: &MacAddress, path: PathBuf, generation: u64, byte_offset: u64) {
        let mut q = self.queue.lock().await;
        q.insert(mac.to_string(), StreamEntry {
            path,
            generation,
            byte_offset,
        });
    }

    /// Get the queued file for a player.
    pub async fn get_entry(&self, mac_str: &str) -> Option<StreamEntry> {
        let q = self.queue.lock().await;
        q.get(mac_str).cloned()
    }
}

/// Combined state for the HTTP server (streaming + cometd).
#[derive(Clone)]
pub struct HttpState {
    pub streaming: StreamingState,
    pub cometd: CometdState,
}

#[derive(Debug, serde::Deserialize)]
struct StreamQuery {
    player: String,
    #[serde(default)]
    gen: u64,
}

/// Handler for GET /stream?player=<MAC>&gen=<gen>
async fn stream_handler(
    State(state): State<HttpState>,
    Query(query): Query<StreamQuery>,
) -> impl IntoResponse {
    let entry = match state.streaming.get_entry(&query.player).await {
        Some(e) => e,
        None => {
            tracing::warn!("Squeeze HTTP: no queued file for player {}", query.player);
            return Err((StatusCode::NOT_FOUND, "No stream queued for this player"));
        }
    };

    // Open the file
    let file = match tokio::fs::File::open(&entry.path).await {
        Ok(f) => f,
        Err(e) => {
            tracing::error!("Squeeze HTTP: cannot open {:?}: {}", entry.path, e);
            return Err((StatusCode::INTERNAL_SERVER_ERROR, "Cannot open audio file"));
        }
    };

    // Seek to byte offset if needed
    if entry.byte_offset > 0 {
        use tokio::io::AsyncSeekExt;
        let mut file = file;
        if let Err(e) = file.seek(std::io::SeekFrom::Start(entry.byte_offset)).await {
            tracing::warn!("Squeeze HTTP: seek failed: {}", e);
            // Continue from start rather than failing
        }
        let stream = ReaderStream::new(file);
        let content_type = guess_content_type(&entry.path);
        return Ok((
            [
                (header::CONTENT_TYPE, content_type),
                (header::CONNECTION, "close".to_string()),
            ],
            axum::body::Body::from_stream(stream),
        ));
    }

    let stream = ReaderStream::new(file);
    let content_type = guess_content_type(&entry.path);

    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (header::CONNECTION, "close".to_string()),
        ],
        axum::body::Body::from_stream(stream),
    ))
}

fn guess_content_type(path: &PathBuf) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "flac" => "audio/flac",
        "mp3" => "audio/mpeg",
        "ogg" | "oga" => "audio/ogg",
        "aac" | "m4a" => "audio/mp4",
        "wav" | "wave" => "audio/wav",
        "aif" | "aiff" => "audio/aiff",
        "wma" => "audio/x-ms-wma",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// LMS-compatible root handler — players may probe this to verify the server is alive.
async fn root_handler() -> impl IntoResponse {
    axum::response::Html(
        "<html><head><title>Audion</title></head><body><h1>Audion Squeeze Server</h1></body></html>"
    )
}

/// LMS-compatible JSONRPC handler — returns minimal valid responses.
/// Players (e.g. Eversolo) may call this to verify the server before connecting via SlimProto.
async fn jsonrpc_handler(body: axum::body::Bytes) -> impl IntoResponse {
    tracing::info!("Squeeze HTTP: JSONRPC request ({} bytes)", body.len());
    // Return a minimal valid JSON-RPC response
    Json(serde_json::json!({
        "params": [],
        "method": "slim.request",
        "id": 1,
        "result": {
            "version": "7.999.999",
            "_can": 1
        }
    }))
}

/// LMS server status endpoint — some players query this.
async fn server_status_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "result": {
            "version": "7.999.999",
            "uuid": "audion-squeeze-server-0001",
            "info total albums": 0,
            "info total artists": 0,
            "info total songs": 0,
            "player count": 0,
            "lastscan": 0,
            "progressname": "",
            "progressdone": "",
            "progresstotal": ""
        }
    }))
}

/// Catch-all handler — log any unhandled requests for debugging.
async fn fallback_handler(uri: axum::http::Uri, method: axum::http::Method, body: axum::body::Bytes) -> impl IntoResponse {
    let body_str = String::from_utf8_lossy(&body);
    tracing::info!("Squeeze HTTP: unhandled {} {} ({} bytes): {}", method, uri, body.len(), body_str);
    (StatusCode::OK, "OK")
}

/// Cometd endpoint handler — delegates to the cometd module.
async fn cometd_route_handler(
    State(state): State<HttpState>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    cometd::cometd_handler(
        axum::extract::State(state.cometd),
        body,
    ).await
}

/// Start the HTTP streaming server with a pre-bound listener.
/// Uses hyper directly (instead of axum::serve) so we can enable title_case_headers,
/// which is required because the Eversolo's Squeezer app has a custom HTTP parser
/// that does case-sensitive header matching (e.g. looks for "Content-Length: " not "content-length: ").
pub async fn start_streaming_server_with_listener(
    listener: tokio::net::TcpListener,
    state: StreamingState,
    cometd_state: CometdState,
) -> Result<(), String> {
    let http_state = HttpState { streaming: state, cometd: cometd_state };

    let app = Router::new()
        .route("/stream", axum::routing::get(stream_handler))
        .route("/cometd", axum::routing::post(cometd_route_handler))
        .route("/cometd/connect", axum::routing::post(cometd_route_handler))
        .route("/cometd/subscribe", axum::routing::post(cometd_route_handler))
        .route("/cometd/disconnect", axum::routing::post(cometd_route_handler))
        .route("/cometd/handshake", axum::routing::post(cometd_route_handler))
        .route("/", axum::routing::get(root_handler))
        .route("/jsonrpc.js", axum::routing::post(jsonrpc_handler).get(server_status_handler))
        .route("/jsonrpc", axum::routing::post(jsonrpc_handler).get(server_status_handler))
        .route("/status", axum::routing::get(server_status_handler))
        .fallback(fallback_handler)
        .with_state(http_state)
        .layer(axum::middleware::from_fn(http_logging_middleware));

    tracing::info!("Squeeze HTTP: listening on port {}", listener.local_addr().map(|a| a.port()).unwrap_or(0));

    loop {
        let (stream, addr) = listener.accept().await.map_err(|e| format!("{}", e))?;
        let app = app.clone();

        tokio::spawn(async move {
            let io = hyper_util::rt::TokioIo::new(stream);

            let service = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                let app = app.clone();
                async move {
                    let req = req.map(axum::body::Body::new);
                    let resp = tower::ServiceExt::oneshot(app, req).await
                        .unwrap_or_else(|err: std::convert::Infallible| match err {});
                    Ok::<_, std::convert::Infallible>(resp)
                }
            });

            let mut builder = hyper::server::conn::http1::Builder::new();
            builder.title_case_headers(true);

            if let Err(e) = builder.serve_connection(io, service).await {
                // Don't log connection-reset errors (normal for clients disconnecting)
                let msg = format!("{}", e);
                if !msg.contains("reset") && !msg.contains("broken pipe") {
                    tracing::debug!("Squeeze HTTP: connection error from {}: {}", addr, e);
                }
            }
        });
    }
}

/// Middleware that logs every HTTP request and response at the transport level.
async fn http_logging_middleware(
    req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let version = req.version();
    let headers = format!("{:?}", req.headers());
    tracing::info!("HTTP IN: {:?} {} {} headers={}", version, method, uri, headers);

    let response = next.run(req).await;

    tracing::info!(
        "HTTP OUT: {} {} → {} headers={:?}",
        method, uri, response.status(), response.headers()
    );
    response
}
