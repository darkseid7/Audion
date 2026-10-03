//! Revision-consistent DB capture only. Sorting, metadata parsing and projection run after unlock.
use super::{commands::error, protocol::*, queries::QueryContext};
use crate::db::queries as db;
use rusqlite::Connection;
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct CapturedLibrary {
    pub tracks: Vec<db::Track>,
    pub albums: Vec<db::Album>,
    pub liked: HashSet<i64>,
    pub liked_albums: HashSet<i64>,
    pub playlists: Vec<DisplayPlaylist>,
}
fn db_error(_: rusqlite::Error) -> ControlError {
    error(ControlErrorCode::ExecutionFailed)
}
impl CapturedLibrary {
    pub fn read(
        conn: &Connection,
        query: &ApplicationQuery,
        context: &QueryContext,
        offset: usize,
        limit: usize,
    ) -> Result<Self, ControlError> {
        let mut result = Self::default();
        match query {
            ApplicationQuery::Playlists { .. } => {
                result.playlists = conn
                    .prepare("SELECT p.id,p.name,COUNT(pt.track_id) FROM playlists p LEFT JOIN playlist_tracks pt ON pt.playlist_id=p.id WHERE COALESCE(p.deleted,0)=0 GROUP BY p.id ORDER BY p.name,p.id")
                    .map_err(db_error)?
                    .query_map([], |r| {
                        Ok(DisplayPlaylist {
                            id: r.get(0)?, name: r.get(1)?, track_count: r.get(2)?, artwork: None,
                        })
                    })
                    .map_err(db_error)?
                    .collect::<rusqlite::Result<_>>()
                    .map_err(db_error)?;
                return Ok(result);
            }
            ApplicationQuery::Queue { .. } => {
                for entry in context.queue.iter().skip(offset).take(limit) {
                    result.load_tracks(conn, "WHERE t.id=?1", rusqlite::params![entry.track.id])?;
                    if result
                        .tracks
                        .last()
                        .is_none_or(|track| track.id as u64 != entry.track.id)
                    {
                        return Err(error(ControlErrorCode::NotFound));
                    }
                }
            }
            ApplicationQuery::AlbumTracks { album_id, .. }
            | ApplicationQuery::AlbumDetail { album_id } => {
                result.load_tracks(conn, "WHERE t.album_id=?1", rusqlite::params![album_id])?
            }
            ApplicationQuery::ArtistTracks { artist_name, .. } => {
                result.load_tracks(conn, "WHERE t.artist=?1", rusqlite::params![artist_name])?
            }
            ApplicationQuery::ArtistAlbums { artist_name, .. } => result.load_tracks(
                conn,
                "WHERE t.album_id IN (SELECT album_id FROM tracks WHERE artist=?1)",
                rusqlite::params![artist_name],
            )?,
            ApplicationQuery::LikedTracks { .. } => result.load_tracks(
                conn,
                "WHERE liked.track_id IS NOT NULL ORDER BY liked.liked_at DESC,t.id",
                [],
            )?,
            ApplicationQuery::PlaylistTracks { playlist_id, .. } => {
                let exists = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM playlists WHERE id=?1 AND COALESCE(deleted,0)=0)",
                    [playlist_id], |r| r.get::<_, bool>(0),
                ).map_err(db_error)?;
                if !exists {
                    return Err(error(ControlErrorCode::NotFound));
                }
                result.load_tracks(
                    conn,
                    "JOIN playlist_tracks pt ON pt.track_id=t.id WHERE pt.playlist_id=?1 ORDER BY pt.position",
                    rusqlite::params![playlist_id],
                )?;
            }
            ApplicationQuery::Tracks { .. }
            | ApplicationQuery::Albums { .. }
            | ApplicationQuery::Artists { .. }
            | ApplicationQuery::Search { .. } => result.load_tracks(conn, "", [])?,
            _ => return Err(error(ControlErrorCode::Unsupported)),
        }
        match query {
            ApplicationQuery::AlbumDetail { album_id } => {
                result.load_albums(conn, "WHERE a.id=?1", rusqlite::params![album_id])?
            }
            ApplicationQuery::ArtistAlbums { artist_name, .. } => result.load_albums(
                conn,
                "WHERE a.id IN (SELECT album_id FROM tracks WHERE artist=?1)",
                rusqlite::params![artist_name],
            )?,
            ApplicationQuery::Albums { .. } | ApplicationQuery::Search { .. } => {
                result.load_albums(conn, "", [])?
            }
            _ => {}
        }
        Ok(result)
    }
    fn load_tracks(
        &mut self,
        conn: &Connection,
        suffix: &str,
        params: impl rusqlite::Params,
    ) -> Result<(), ControlError> {
        // No paths, provider URLs, embedded cover bodies or local playback sources are captured.
        let sql=format!("SELECT t.id,t.title,t.artist,t.album,t.track_number,t.duration,t.album_id,t.format,t.bitrate,t.disc_number,t.metadata_json,t.date_added,liked.track_id IS NOT NULL FROM tracks t LEFT JOIN liked_tracks liked ON liked.track_id=t.id {suffix}");
        let mut statement = conn.prepare(&sql).map_err(db_error)?;
        let rows = statement
            .query_map(params, |r| {
                Ok((
                    db::Track {
                        id: r.get(0)?,
                        path: String::new(),
                        title: r.get(1)?,
                        artist: r.get(2)?,
                        album: r.get(3)?,
                        track_number: r.get(4)?,
                        duration: r.get(5)?,
                        album_id: r.get(6)?,
                        format: r.get(7)?,
                        bitrate: r.get(8)?,
                        disc_number: r.get(9)?,
                        metadata_json: r.get(10)?,
                        date_added: r.get(11)?,
                        source_type: None,
                        cover_url: None,
                        external_id: None,
                        local_src: None,
                        track_cover: None,
                        track_cover_path: None,
                        play_count: None,
                    },
                    r.get::<_, bool>(12)?,
                ))
            })
            .map_err(db_error)?;
        for row in rows {
            let (track, liked) = row.map_err(db_error)?;
            if liked {
                self.liked.insert(track.id);
            }
            self.tracks.push(track);
        }
        Ok(())
    }
    fn load_albums(
        &mut self,
        conn: &Connection,
        suffix: &str,
        params: impl rusqlite::Params,
    ) -> Result<(), ControlError> {
        let sql=format!("SELECT a.id,a.name,a.artist,a.year,a.original_year,liked.album_id IS NOT NULL FROM albums a LEFT JOIN liked_albums liked ON liked.album_id=a.id {suffix}");
        let mut statement = conn.prepare(&sql).map_err(db_error)?;
        let rows = statement
            .query_map(params, |r| {
                Ok((
                    db::Album {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        artist: r.get(2)?,
                        year: r.get(3)?,
                        original_year: r.get(4)?,
                        art_data: None,
                        art_path: None,
                    },
                    r.get::<_, bool>(5)?,
                ))
            })
            .map_err(db_error)?;
        for row in rows {
            let (album, liked) = row.map_err(db_error)?;
            if liked {
                self.liked_albums.insert(album.id);
            }
            self.albums.push(album);
        }
        Ok(())
    }
}
