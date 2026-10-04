use super::{init_schema, initialize_controller_revision};
use crate::db::queries::record_play;
use rusqlite::Connection;

fn fixture() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    init_schema(&conn).unwrap();
    conn.execute(
        "INSERT INTO albums(id,name,artist) VALUES(1,'Album','Artist')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tracks(id,path,title,album_id) VALUES(1,'fixture.flac','Track',1)",
        [],
    )
    .unwrap();
    conn
}

fn stamp(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT stamp FROM controller_library_revision WHERE id=1",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn completed_play_preserves_controller_library_revision_and_records_statistics() {
    let conn = fixture();
    let before = stamp(&conn);
    record_play(&conn, 1, Some(1), 120).unwrap();
    assert_eq!(
        stamp(&conn),
        before,
        "play statistics must not invalidate album pages"
    );
    assert_eq!(
        conn.query_row("SELECT play_count FROM tracks WHERE id=1", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM play_history WHERE track_id=1",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}

#[test]
fn existing_all_column_trigger_is_migrated_without_resetting_revision() {
    let conn = fixture();
    conn.execute_batch(
        "DROP TRIGGER controller_revision_tracks_UPDATE;
        CREATE TRIGGER controller_revision_tracks_UPDATE AFTER UPDATE ON tracks
        WHEN OLD.play_count IS NOT NEW.play_count OR OLD.title IS NOT NEW.title
        BEGIN UPDATE controller_library_revision SET stamp=stamp+1 WHERE id=1; END;",
    )
    .unwrap();
    let before = stamp(&conn);
    initialize_controller_revision(&conn).unwrap();
    initialize_controller_revision(&conn).unwrap();
    assert_eq!(stamp(&conn), before);
    record_play(&conn, 1, Some(1), 120).unwrap();
    assert_eq!(
        stamp(&conn),
        before,
        "existing installations need the new trigger policy"
    );
    conn.execute(
        "UPDATE tracks SET title='Changed',play_count=2 WHERE id=1",
        [],
    )
    .unwrap();
    assert_eq!(
        stamp(&conn),
        before + 1,
        "mixed metadata/statistics writes must invalidate"
    );
}

#[test]
fn metadata_artwork_membership_and_rollback_keep_revision_semantics() {
    let conn = fixture();
    for sql in [
        "UPDATE tracks SET title='Changed' WHERE id=1",
        "UPDATE tracks SET track_cover_path='cover.png' WHERE id=1",
        "UPDATE tracks SET cover_url='https://example.invalid/art.png' WHERE id=1",
        "UPDATE albums SET art_path='album.png' WHERE id=1",
        "INSERT INTO liked_tracks(track_id) VALUES(1)",
        "DELETE FROM liked_tracks WHERE track_id=1",
        "INSERT INTO tracks(id,path) VALUES(2,'second.flac')",
        "DELETE FROM tracks WHERE id=2",
    ] {
        let before = stamp(&conn);
        conn.execute(sql, []).unwrap();
        assert_eq!(stamp(&conn), before + 1, "{sql}");
    }
    let before = stamp(&conn);
    conn.execute(
        "UPDATE tracks SET title=title,play_count=play_count WHERE id=1",
        [],
    )
    .unwrap();
    assert_eq!(stamp(&conn), before);
    conn.execute_batch("BEGIN; UPDATE tracks SET title=NULL WHERE id=1; ROLLBACK;")
        .unwrap();
    assert_eq!(stamp(&conn), before);
    conn.execute("UPDATE tracks SET title=NULL WHERE id=1", [])
        .unwrap();
    assert_eq!(stamp(&conn), before + 1, "NULL transitions remain changes");
}
