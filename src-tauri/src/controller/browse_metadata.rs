//! Fresh pathless metrics capture, independent of catalog/artwork publication.
use super::{commands::error, protocol::*};
use rusqlite::{types::ValueRef, Connection, OptionalExtension};
#[derive(Debug)]
pub struct BrowseMetadataPayload {
    pub tracks: Vec<TrackPlayCount>,
    pub album: Option<AlbumDuration>,
}
fn metric(value: ValueRef<'_>) -> Option<u64> {
    match value {
        ValueRef::Integer(v) if (0..=9_007_199_254_740_991).contains(&v) => Some(v as u64),
        _ => None,
    }
}
pub fn capture_browse_metadata(
    conn: &Connection,
    query: &BrowseMetadataQuery,
) -> Result<BrowseMetadataPayload, ControlError> {
    if !query.valid() {
        return Err(error(ControlErrorCode::InvalidRequest));
    }
    let db_error = |_| error(ControlErrorCode::ExecutionFailed);
    // One consistent snapshot, even when an external DB connection writes.
    let tx = conn.unchecked_transaction().map_err(db_error)?;
    let mut tracks = Vec::new();
    let mut seen = std::collections::HashSet::new();
    {
        // One bound key per query stays below every SQLite variable limit.
        let mut stmt = tx
            .prepare("SELECT play_count FROM tracks WHERE id=?1")
            .map_err(db_error)?;
        for id in &query.track_ids {
            if !seen.insert(*id) {
                continue;
            }
            let play_count = stmt
                .query_row([*id], |row| Ok(metric(row.get_ref(0)?)))
                .optional()
                .map_err(db_error)?
                .flatten();
            tracks.push(TrackPlayCount {
                track_id: *id,
                play_count,
            });
        }
    }
    let album = if let Some(album_id) = query.album_id {
        if !tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM albums WHERE id=?1)",
                [album_id],
                |r| r.get::<_, bool>(0),
            )
            .map_err(db_error)?
        {
            return Err(error(ControlErrorCode::NotFound));
        }
        let mut stmt = tx
            .prepare("SELECT duration FROM tracks WHERE album_id=?1")
            .map_err(db_error)?;
        let mut rows = stmt.query([album_id]).map_err(db_error)?;
        let mut total = Some(0u64);
        while let Some(row) = rows.next().map_err(db_error)? {
            total = total
                .zip(metric(row.get_ref(0).map_err(db_error)?))
                .and_then(|(a, b)| a.checked_add(b))
                .filter(|v| *v <= 9_007_199_254_740_991);
        }
        Some(AlbumDuration {
            album_id,
            total_duration_seconds: total,
        })
    } else {
        None
    };
    tx.commit().map_err(db_error)?;
    Ok(BrowseMetadataPayload { tracks, album })
}
#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    fn fixture() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::init_schema(&conn).unwrap();
        conn.execute(
            "INSERT INTO albums(id,name) VALUES(8,'Full album'),(9,'Empty')",
            [],
        )
        .unwrap();
        for id in 1..=101 {
            conn.execute("INSERT INTO tracks(id,path,album_id,duration,disc_number,play_count) VALUES(?1,?2,8,2,?3,?4)", rusqlite::params![id,format!("fixture-{id}"),if id>50{2}else{1},if id==2{12}else{0}]).unwrap();
        }
        conn
    }
    fn query() -> BrowseMetadataQuery {
        BrowseMetadataQuery {
            track_ids: vec![1, 2, 999],
            album_id: Some(8),
        }
    }
    #[tokio::test]
    async fn browse_metadata_admission_and_catalog_fences() {
        let conn = fixture();
        let stamp = crate::db::queries::controller_library_stamp(&conn).unwrap();
        let library = crate::controller::queries::LibraryQueries::new(crate::db::Database {
            conn: std::sync::Arc::new(std::sync::Mutex::new(conn)),
        });
        let context = crate::controller::queries::QueryContext {
            host_epoch: "epoch".into(),
            revision: 7,
            stamp,
            queue_revision: 99,
            ..Default::default()
        };
        let request = BrowseMetadataRequest {
            metadata_version: 1,
            host_epoch: "epoch".into(),
            library_revision: 7,
            track_ids: vec![1],
            album_id: Some(8),
        };
        assert!(library
            .query_browse_metadata(request.clone(), context.clone())
            .await
            .unwrap()
            .matches(&request));
        let mut wrong = request.clone();
        wrong.library_revision = 8;
        assert_eq!(
            library
                .query_browse_metadata(wrong, context.clone())
                .await
                .unwrap_err()
                .code,
            ControlErrorCode::ResyncRequired
        );
        library
            .db
            .conn
            .lock()
            .unwrap()
            .execute("UPDATE tracks SET title='changed' WHERE id=1", [])
            .unwrap();
        assert_eq!(
            library
                .query_browse_metadata(request, context)
                .await
                .unwrap_err()
                .code,
            ControlErrorCode::ResyncRequired
        );
    }
    #[tokio::test]
    async fn browse_metadata_catalog_change_during_capture_is_rejected() {
        let conn = fixture();
        let stamp = crate::db::queries::controller_library_stamp(&conn).unwrap();
        let db = crate::db::Database {
            conn: std::sync::Arc::new(std::sync::Mutex::new(conn)),
        };
        let mut library = crate::controller::queries::LibraryQueries::new(db.clone());
        library.metadata_probe(std::sync::Arc::new(move || {
            db.conn
                .lock()
                .unwrap()
                .execute("UPDATE tracks SET title='changed' WHERE id=1", [])
                .unwrap();
        }));
        let context = crate::controller::queries::QueryContext {
            host_epoch: "epoch".into(),
            revision: 7,
            stamp,
            ..Default::default()
        };
        let request = BrowseMetadataRequest {
            metadata_version: 1,
            host_epoch: "epoch".into(),
            library_revision: 7,
            track_ids: vec![1],
            album_id: None,
        };
        assert_eq!(
            library
                .query_browse_metadata(request, context)
                .await
                .unwrap_err()
                .code,
            ControlErrorCode::ResyncRequired
        );
    }
    #[test]
    fn browse_metadata_full_album_not_loaded_or_liked_subtotal() {
        let conn = fixture();
        conn.execute("INSERT INTO liked_tracks(track_id) VALUES(1)", [])
            .unwrap();
        let data = capture_browse_metadata(&conn, &query()).unwrap();
        assert_eq!(data.album.unwrap().total_duration_seconds, Some(202));
        assert_eq!(
            data.tracks.iter().map(|t| t.play_count).collect::<Vec<_>>(),
            vec![Some(0), Some(12), None]
        );
        let empty = capture_browse_metadata(
            &conn,
            &BrowseMetadataQuery {
                track_ids: vec![],
                album_id: Some(9),
            },
        )
        .unwrap();
        assert_eq!(empty.album.unwrap().total_duration_seconds, Some(0));
        assert_eq!(
            capture_browse_metadata(
                &conn,
                &BrowseMetadataQuery {
                    track_ids: vec![],
                    album_id: Some(10)
                }
            )
            .unwrap_err()
            .code,
            ControlErrorCode::NotFound
        );
    }
    #[test]
    fn browse_metadata_unknowns_are_not_zero() {
        let conn = fixture();
        for value in ["NULL", "-1", "9007199254740992", "'bad'", "1.5"] {
            conn.execute(
                &format!("UPDATE tracks SET play_count={value} WHERE id=1"),
                [],
            )
            .unwrap();
            assert_eq!(
                capture_browse_metadata(&conn, &query()).unwrap().tracks[0].play_count,
                None
            );
        }
        for value in ["NULL", "-1", "9007199254740992", "'bad'", "1.5"] {
            conn.execute(
                &format!("UPDATE tracks SET duration={value} WHERE id=1"),
                [],
            )
            .unwrap();
            assert_eq!(
                capture_browse_metadata(&conn, &query())
                    .unwrap()
                    .album
                    .unwrap()
                    .total_duration_seconds,
                None
            );
        }
        conn.execute(
            "UPDATE tracks SET duration=9007199254740991 WHERE id IN(1,2)",
            [],
        )
        .unwrap();
        assert_eq!(
            capture_browse_metadata(&conn, &query())
                .unwrap()
                .album
                .unwrap()
                .total_duration_seconds,
            None
        );
    }
    #[test]
    fn browse_metadata_count_write_does_not_advance_library() {
        let conn = fixture();
        let before = crate::db::queries::controller_library_stamp(&conn).unwrap();
        conn.execute("UPDATE tracks SET play_count=1 WHERE id=1", [])
            .unwrap();
        assert_eq!(
            crate::db::queries::controller_library_stamp(&conn).unwrap(),
            before
        );
        assert_eq!(
            capture_browse_metadata(&conn, &query()).unwrap().tracks[0].play_count,
            Some(1)
        );
    }
}
