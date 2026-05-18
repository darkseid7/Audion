pub mod api;
pub mod audio_server;
pub mod cometd_client;
pub mod discovery;
pub mod types;

use crate::db::Database;
use api::LmsApi;
use audio_server::AudioServer;
use cometd_client::CometdClient;
use reqwest::Client;
use tokio::sync::broadcast;
use types::*;

pub struct LmsClient {
    server: Option<LmsServer>,
    api: Option<LmsApi>,
    http_client: Client,
    audio_server: Option<AudioServer>,
    cometd: CometdClient,
    status_tx: broadcast::Sender<LmsPlayerStatus>,
    db: Database,
}

impl LmsClient {
    pub fn new(db: Database) -> Self {
        let (status_tx, _) = broadcast::channel(64);
        Self {
            server: None,
            api: None,
            http_client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .pool_idle_timeout(std::time::Duration::from_secs(90))
                .build()
                .unwrap_or_default(),
            audio_server: None,
            cometd: CometdClient::new(),
            status_tx,
            db,
        }
    }

    pub async fn connect(&mut self, host: &str, port: u16) -> Result<(), LmsError> {
        let api = LmsApi::new(self.http_client.clone(), host, port);

        // Verify connection by fetching players
        api.get_players().await?;

        // Start audio server if not already running
        if self.audio_server.is_none() {
            let server = AudioServer::start(self.db.clone())
                .await
                .map_err(|e| LmsError::Network(e))?;
            self.audio_server = Some(server);
        }

        self.server = Some(LmsServer {
            name: format!("LMS @ {}:{}", host, port),
            host: host.to_string(),
            json_port: port,
            uuid: String::new(),
        });
        self.api = Some(api);

        eprintln!("[LMS] Connected to LMS at {}:{}", host, port);
        Ok(())
    }

    pub fn disconnect(&mut self) {
        self.cometd.stop();
        if let Some(mut server) = self.audio_server.take() {
            server.stop();
        }
        self.api = None;
        self.server = None;
        tracing::info!("Disconnected from LMS");
    }

    pub fn is_connected(&self) -> bool {
        self.api.is_some()
    }

    pub fn connected_server(&self) -> Option<&LmsServer> {
        self.server.as_ref()
    }

    fn api(&self) -> Result<&LmsApi, LmsError> {
        self.api.as_ref().ok_or(LmsError::NotConnected)
    }

    // ── Player queries ─────────────────────────────────────────────────────

    pub async fn get_players(&self) -> Result<Vec<LmsPlayer>, LmsError> {
        self.api()?.get_players().await
    }

    pub async fn get_player_status(&self, player_id: &str) -> Result<LmsPlayerStatus, LmsError> {
        let mut status = self.api()?.get_player_status(player_id).await?;
        self.enrich_metadata(&mut status);
        Ok(status)
    }

    /// Enrich playlist tracks with local DB metadata when URLs are from our audio server.
    pub fn enrich_metadata(&self, status: &mut LmsPlayerStatus) {
        let pattern = "/audio/";

        for track in &mut status.playlist {
            let url_str = track.url.as_deref().unwrap_or("");
            let title_str = &track.title;
            tracing::warn!("enrich_metadata: url={}, title={}", url_str, title_str);

            if let Some(track_id) = extract_id_from_str(url_str, pattern)
                .or_else(|| extract_id_from_str(title_str, pattern))
            {
                tracing::warn!("enrich_metadata: extracted track_id={}", track_id);
                let conn = self.db.conn.lock().unwrap();
                match crate::db::queries::get_track_by_id(&conn, track_id) {
                    Ok(Some(db_track)) => {
                        tracing::warn!(
                            "enrich_metadata: DB hit for id={}: title={:?}, artist={:?}, album={:?}",
                            track_id, db_track.title, db_track.artist, db_track.album
                        );
                        track.title = db_track.title.unwrap_or_else(|| "Unknown".to_string());
                        track.artist = db_track.artist.unwrap_or_default();
                        track.album = db_track.album.unwrap_or_default();
                        track.duration = db_track.duration.unwrap_or(0) as f64;
                        track.format = db_track.format;
                        track.bitrate = db_track.bitrate.map(|b| format!("{}kbps", b));
                        track.id = track_id;
                        if let Some(server) = &self.audio_server {
                            track.artwork_url = Some(format!(
                                "http://{}:{}/cover/{}",
                                server.local_ip, server.port, track_id
                            ));
                        }
                    }
                    Ok(None) => {
                        tracing::warn!("enrich_metadata: track_id={} NOT FOUND in DB", track_id);
                    }
                    Err(e) => {
                        tracing::error!("enrich_metadata: DB error for id={}: {}", track_id, e);
                    }
                }
            } else {
                tracing::warn!("enrich_metadata: no /audio/ pattern found in url or title");
            }
        }

        // Update current_track from enriched playlist
        if let Some(current) = status.playlist.get(status.playlist_index).cloned() {
            status.current_track = Some(current);
        }
    }

    // ── Playback control ───────────────────────────────────────────────────

    pub async fn play(&self, player_id: &str) -> Result<(), LmsError> {
        self.api()?.play(player_id).await
    }

    pub async fn pause(&self, player_id: &str) -> Result<(), LmsError> {
        self.api()?.pause(player_id).await
    }

    pub async fn unpause(&self, player_id: &str) -> Result<(), LmsError> {
        self.api()?.unpause(player_id).await
    }

    pub async fn stop(&self, player_id: &str) -> Result<(), LmsError> {
        self.api()?.stop(player_id).await
    }

    pub async fn next_track(&self, player_id: &str) -> Result<(), LmsError> {
        self.api()?.next_track(player_id).await
    }

    pub async fn previous_track(&self, player_id: &str) -> Result<(), LmsError> {
        self.api()?.previous_track(player_id).await
    }

    pub async fn seek(&self, player_id: &str, seconds: f64) -> Result<(), LmsError> {
        self.api()?.seek(player_id, seconds).await
    }

    pub async fn set_volume(&self, player_id: &str, volume: u8) -> Result<(), LmsError> {
        self.api()?.set_volume(player_id, volume).await
    }

    // ── Queue management ───────────────────────────────────────────────────

    fn get_track_title(&self, track_id: i64) -> String {
        let conn = self.db.conn.lock().unwrap();
        crate::db::queries::get_track_by_id(&conn, track_id)
            .ok()
            .flatten()
            .and_then(|t| {
                let artist = t.artist.unwrap_or_default();
                let title = t.title.unwrap_or_default();
                if !title.is_empty() && !artist.is_empty() {
                    Some(format!("{} - {}", artist, title))
                } else if !title.is_empty() {
                    Some(title)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| format!("Track {}", track_id))
    }

    pub async fn play_tracks(
        &self,
        player_id: &str,
        track_ids: &[i64],
        start_index: usize,
    ) -> Result<(), LmsError> {
        eprintln!("[LMS] play_tracks: {} tracks, start_index={}", track_ids.len(), start_index);
        let api = self.api()?;
        let audio_server = self
            .audio_server
            .as_ref()
            .ok_or_else(|| LmsError::Network("Audio server not running".into()))?;

        api.playlist_clear(player_id).await?;

        for (i, &track_id) in track_ids.iter().enumerate() {
            let url = audio_server.track_url(track_id);
            let title = self.get_track_title(track_id);
            eprintln!("[LMS] adding track: {} -> {}", title, url);
            if i == 0 && start_index == 0 {
                api.playlist_play(player_id, &url, &title).await?;
            } else {
                api.playlist_add(player_id, &url, &title).await?;
            }
        }

        if start_index > 0 {
            api.playlist_jump(player_id, start_index).await?;
        }

        Ok(())
    }

    pub async fn add_tracks(
        &self,
        player_id: &str,
        track_ids: &[i64],
    ) -> Result<(), LmsError> {
        let api = self.api()?;
        let audio_server = self
            .audio_server
            .as_ref()
            .ok_or_else(|| LmsError::Network("Audio server not running".into()))?;

        for &track_id in track_ids {
            let url = audio_server.track_url(track_id);
            let title = self.get_track_title(track_id);
            api.playlist_add(player_id, &url, &title).await?;
        }

        Ok(())
    }

    // ── Real-time status subscription ──────────────────────────────────────

    pub fn subscribe_status(&mut self, player_id: &str) {
        let api = match &self.api {
            Some(api) => api,
            None => return,
        };

        self.cometd.start(
            self.http_client.clone(),
            api.base_url().to_string(),
            player_id.to_string(),
            self.status_tx.clone(),
        );
    }

    pub fn unsubscribe_status(&mut self) {
        self.cometd.stop();
    }

    pub fn status_receiver(&self) -> broadcast::Receiver<LmsPlayerStatus> {
        self.status_tx.subscribe()
    }
}

/// Extract a track_id from a string containing our audio server URL pattern.
/// e.g. "http://192.168.1.3:51496/audio/477" with pattern ":51496/audio/" → Some(477)
fn extract_id_from_str(s: &str, pattern: &str) -> Option<i64> {
    let idx = s.find(pattern)?;
    let after = &s[idx + pattern.len()..];
    let id_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    id_str.parse().ok()
}
