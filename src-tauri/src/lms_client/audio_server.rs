use crate::db::Database;
use axum::extract::{Path as AxumPath, State as AxumState};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Router;
use lofty::file::TaggedFileExt;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::net::TcpListener;
use tokio_util::io::ReaderStream;

#[derive(Clone)]
struct AudioServerState {
    db: Database,
}

pub struct AudioServer {
    pub port: u16,
    pub local_ip: Ipv4Addr,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl AudioServer {
    pub async fn start(db: Database) -> Result<Self, String> {
        let local_ip = detect_local_ip();
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))
            .await
            .map_err(|e| format!("Failed to bind audio server: {}", e))?;

        let port = listener
            .local_addr()
            .map_err(|e| format!("Failed to get local addr: {}", e))?
            .port();

        let state = AudioServerState { db };
        let app = Router::new()
            .route("/audio/{track_id}", axum::routing::get(serve_audio).head(serve_audio_head))
            .route("/cover/{track_id}", axum::routing::get(serve_cover))
            .with_state(state);

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .ok();
        });

        eprintln!("[AUDIO-SERVER] started on {}:{}", local_ip, port);

        Ok(Self {
            port,
            local_ip,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    pub fn track_url(&self, track_id: i64) -> String {
        format!("http://{}:{}/audio/{}", self.local_ip, self.port, track_id)
    }

    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for AudioServer {
    fn drop(&mut self) {
        self.stop();
    }
}

async fn serve_audio(
    AxumState(state): AxumState<AudioServerState>,
    AxumPath(track_id): AxumPath<i64>,
    headers: HeaderMap,
) -> impl IntoResponse {
    eprintln!("[AUDIO-SERVER] request for track_id={}, range={:?}", track_id, headers.get(header::RANGE));

    let path_str = {
        let conn = state.db.conn.lock().unwrap();
        crate::db::queries::get_track_by_id(&conn, track_id)
            .ok()
            .flatten()
            .map(|t| t.path)
    };

    let Some(path_str) = path_str else {
        eprintln!("[AUDIO-SERVER] track {} not found in DB", track_id);
        return Err((StatusCode::NOT_FOUND, "Track not found"));
    };

    let path = PathBuf::from(&path_str);
    if !path.exists() {
        eprintln!("[AUDIO-SERVER] file not found: {}", path_str);
        return Err((StatusCode::NOT_FOUND, "File not found"));
    };

    let content_type = guess_content_type(&path);
    let file_size = tokio::fs::metadata(&path)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to read metadata"))?
        .len();

    eprintln!("[AUDIO-SERVER] serving track {} size={} type={}", track_id, file_size, content_type);

    let range_header = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("bytes="));

    if let Some(range_str) = range_header {
        let (start, end) = parse_range(range_str, file_size);
        let length = end - start + 1;

        let mut file = tokio::fs::File::open(&path)
            .await
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to open file"))?;
        file.seek(std::io::SeekFrom::Start(start))
            .await
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Seek failed"))?;

        let limited = file.take(length);
        let stream = ReaderStream::new(limited);
        let body = axum::body::Body::from_stream(stream);

        Ok((
            StatusCode::PARTIAL_CONTENT,
            [
                (header::CONTENT_TYPE, content_type),
                (header::CONTENT_LENGTH, length.to_string()),
                (
                    header::CONTENT_RANGE,
                    format!("bytes {}-{}/{}", start, end, file_size),
                ),
                (header::ACCEPT_RANGES, "bytes".to_string()),
                (header::CONNECTION, "keep-alive".to_string()),
            ],
            body,
        )
            .into_response())
    } else {
        let file = tokio::fs::File::open(&path)
            .await
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to open file"))?;

        let stream = ReaderStream::new(file);
        let body = axum::body::Body::from_stream(stream);

        Ok((
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, content_type),
                (header::CONTENT_LENGTH, file_size.to_string()),
                (header::ACCEPT_RANGES, "bytes".to_string()),
                (header::CONNECTION, "keep-alive".to_string()),
            ],
            body,
        )
            .into_response())
    }
}

async fn serve_audio_head(
    AxumState(state): AxumState<AudioServerState>,
    AxumPath(track_id): AxumPath<i64>,
) -> impl IntoResponse {
    let path_str = {
        let conn = state.db.conn.lock().unwrap();
        crate::db::queries::get_track_by_id(&conn, track_id)
            .ok()
            .flatten()
            .map(|t| t.path)
    };

    let Some(path_str) = path_str else {
        return Err((StatusCode::NOT_FOUND, "Track not found"));
    };

    let path = PathBuf::from(&path_str);
    if !path.exists() {
        return Err((StatusCode::NOT_FOUND, "File not found"));
    }

    let content_type = guess_content_type(&path);
    let file_size = tokio::fs::metadata(&path)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to read metadata"))?
        .len();

    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (header::CONTENT_LENGTH, file_size.to_string()),
            (header::ACCEPT_RANGES, "bytes".to_string()),
        ],
        "",
    ))
}

fn parse_range(range_str: &str, file_size: u64) -> (u64, u64) {
    let parts: Vec<&str> = range_str.splitn(2, '-').collect();
    let start = parts[0].parse::<u64>().unwrap_or(0);
    let end = parts
        .get(1)
        .and_then(|s| if s.is_empty() { None } else { s.parse::<u64>().ok() })
        .unwrap_or(file_size - 1)
        .min(file_size - 1);
    (start, end)
}

async fn serve_cover(
    AxumState(state): AxumState<AudioServerState>,
    AxumPath(track_id): AxumPath<i64>,
) -> impl IntoResponse {
    let track_info = {
        let conn = state.db.conn.lock().unwrap();
        crate::db::queries::get_track_by_id(&conn, track_id)
            .ok()
            .flatten()
    };

    let Some(track) = track_info else {
        return Err((StatusCode::NOT_FOUND, "Track not found"));
    };

    // Try track_cover_path first, then extract from file
    if let Some(cover_path) = &track.track_cover_path {
        let path = PathBuf::from(cover_path);
        if path.exists() {
            let data = tokio::fs::read(&path)
                .await
                .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to read cover"))?;
            return Ok(([(header::CONTENT_TYPE, "image/jpeg".to_string())], data));
        }
    }

    // Extract from audio file metadata
    {
        let path = PathBuf::from(&track.path);
        if path.exists() {
            if let Ok(tagged_file) = lofty::read_from_path(&path) {
                for tag in tagged_file.tags() {
                    for pic in tag.pictures() {
                        return Ok((
                            [(
                                header::CONTENT_TYPE,
                                pic.mime_type()
                                    .map(|m| m.to_string())
                                    .unwrap_or_else(|| "image/jpeg".to_string()),
                            )],
                            pic.data().to_vec(),
                        ));
                    }
                }
            }
        }
    }

    Err((StatusCode::NOT_FOUND, "No cover art available"))
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
        "dsf" | "dff" => "audio/dsf",
        _ => "application/octet-stream",
    }
    .to_string()
}

fn detect_local_ip() -> Ipv4Addr {
    let socket = UdpSocket::bind("0.0.0.0:0").ok();
    socket
        .and_then(|s| {
            s.connect("8.8.8.8:80").ok()?;
            s.local_addr().ok()
        })
        .and_then(|addr| match addr.ip() {
            std::net::IpAddr::V4(ip) => Some(ip),
            _ => None,
        })
        .unwrap_or(Ipv4Addr::LOCALHOST)
}
