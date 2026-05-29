// File system watcher for automatic library updates (desktop only)
//
// Uses the `notify` crate to watch registered music folders for changes.
// Events are debounced (2 seconds) and processed incrementally:
//   - New audio files  → extract metadata + insert into DB
//   - Modified files   → extract metadata + update in DB
//   - Deleted files    → remove from DB + cleanup empty albums

use crate::db::queries::{self, TrackInsert};
use crate::scanner::cover_storage;
use crate::scanner::metadata::extract_metadata;
use crate::scanner::walker::{is_supported_audio_file, scan_directory};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, RecommendedCache};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::Emitter;

/// Managed state for the file watcher
pub struct WatcherState {
    debouncer: Mutex<Option<Debouncer<RecommendedWatcher, RecommendedCache>>>,
    pub is_running: AtomicBool,
}

impl WatcherState {
    pub fn new() -> Self {
        Self {
            debouncer: Mutex::new(None),
            is_running: AtomicBool::new(false),
        }
    }
}

/// Event payload sent to the frontend when the watcher detects changes
#[derive(Debug, Clone, Serialize)]
pub struct WatcherChangeEvent {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub errors: Vec<String>,
}

/// Start watching the given folder paths for audio file changes.
/// Any existing watcher is stopped first.
pub fn start_watching(
    state: &WatcherState,
    folders: Vec<String>,
    db_conn: Arc<Mutex<Connection>>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    // Stop any existing watcher
    stop_watching(state);

    if folders.is_empty() {
        return Ok(());
    }

    let db_for_handler = Arc::clone(&db_conn);
    let handle_for_handler = app_handle.clone();

    // Create debounced watcher (2 second debounce)
    let mut debouncer = new_debouncer(
        Duration::from_secs(2),
        None, // no tick rate override
        move |result: DebounceEventResult| {
            match result {
                Ok(events) => {
                    process_debounced_events(events, &db_for_handler, &handle_for_handler);
                }
                Err(errors) => {
                    for e in errors {
                        tracing::warn!("[Watcher] Error: {}", e);
                    }
                }
            }
        },
    )
    .map_err(|e| format!("Failed to create file watcher: {}", e))?;

    // Watch each registered music folder
    for folder in &folders {
        let path = Path::new(folder);
        if path.exists() && path.is_dir() {
            if let Err(e) = debouncer.watch(path, RecursiveMode::Recursive) {
                tracing::warn!("[Watcher] Failed to watch {}: {}", folder, e);
            } else {
                tracing::info!("[Watcher] Watching: {}", folder);
            }
        } else {
            tracing::warn!("[Watcher] Skipping non-existent folder: {}", folder);
        }
    }

    // Store the debouncer to keep it alive
    let mut guard = state.debouncer.lock().map_err(|e| e.to_string())?;
    *guard = Some(debouncer);
    state.is_running.store(true, Ordering::SeqCst);

    tracing::info!("[Watcher] Started watching {} folder(s)", folders.len());
    Ok(())
}

/// Stop the file watcher
pub fn stop_watching(state: &WatcherState) {
    if let Ok(mut guard) = state.debouncer.lock() {
        if guard.is_some() {
            *guard = None; // dropping the debouncer stops the watcher
            tracing::info!("[Watcher] Stopped");
        }
    }
    state.is_running.store(false, Ordering::SeqCst);
}

/// Process debounced file system events: classify into add/modify/remove,
/// then do incremental DB updates and emit a single event to the frontend.
fn process_debounced_events(
    events: Vec<notify_debouncer_full::DebouncedEvent>,
    db_conn: &Arc<Mutex<Connection>>,
    app_handle: &tauri::AppHandle,
) {
    let mut files_to_upsert: HashSet<PathBuf> = HashSet::new();
    let mut files_to_remove: HashSet<PathBuf> = HashSet::new();
    let mut dirs_removed: Vec<PathBuf> = Vec::new();

    for event in &events {
        use notify::EventKind;

        for path in &event.paths {
            match &event.kind {
                EventKind::Create(_) | EventKind::Modify(_) => {
                    if is_supported_audio_file(path) {
                        files_to_remove.remove(path);
                        files_to_upsert.insert(path.clone());
                    } else if path.is_dir() {
                        // Directory created (e.g. album folder pasted in)
                        // Walk it for audio files
                        let result = scan_directory(&path.to_string_lossy());
                        tracing::info!("[Watcher] Dir created, found {} audio files in {:?}", 
                            result.audio_files.len(), path);
                        for file in result.audio_files {
                            let p = PathBuf::from(&file);
                            files_to_remove.remove(&p);
                            files_to_upsert.insert(p);
                        }
                    }
                }
                EventKind::Remove(_) => {
                    if is_supported_audio_file(path) {
                        // Single audio file removed
                        files_to_upsert.remove(path);
                        files_to_remove.insert(path.clone());
                    } else if path.extension().is_none() {
                        // No extension = likely a directory removal
                        // (Windows emits Remove for the folder, not individual files)
                        dirs_removed.push(path.clone());
                    }
                }
                _ => {}
            }
        }
    }

    if files_to_upsert.is_empty() && files_to_remove.is_empty() && dirs_removed.is_empty() {
        return;
    }

    tracing::info!(
        "[Watcher] Processing {} upsert(s), {} file removal(s), {} dir removal(s)",
        files_to_upsert.len(),
        files_to_remove.len(),
        dirs_removed.len(),
    );

    let mut added = 0usize;
    let mut updated = 0usize;
    let mut removed = 0usize;
    let mut errors = Vec::new();

    let conn = match db_conn.lock() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("[Watcher] Failed to lock DB: {}", e);
            return;
        }
    };

    // Process upserts (new + modified files)
    for path in &files_to_upsert {
        let path_str = match path.to_str() {
            Some(s) => s,
            None => continue,
        };

        // Verify the file still exists (could have been moved/deleted between event and processing)
        if !path.exists() {
            continue;
        }

        match extract_metadata(path_str) {
            Some(track) => {
                match queries::insert_or_update_track(&conn, &track) {
                    Ok((track_id, was_new)) if track_id > 0 => {
                        if was_new {
                            added += 1;
                        } else {
                            updated += 1;
                        }

                        // Save track cover
                        let cover_path = track.track_cover.as_ref().and_then(|bytes| {
                            cover_storage::save_track_cover(track_id, bytes).ok()
                        });
                        if let Some(ref path) = cover_path {
                            let _ = queries::update_track_cover_path(&conn, track_id, Some(path));
                        }

                        // Save album art if the album doesn't have one yet
                        if let Some(album_id) = conn
                            .query_row(
                                "SELECT album_id FROM tracks WHERE id = ?1",
                                rusqlite::params![track_id],
                                |row| row.get::<_, Option<i64>>(0),
                            )
                            .ok()
                            .flatten()
                        {
                            if let Some(ref art_bytes) = track.album_art {
                                let has_art: bool = conn
                                    .query_row(
                                        "SELECT art_path IS NOT NULL FROM albums WHERE id = ?1",
                                        rusqlite::params![album_id],
                                        |row| row.get(0),
                                    )
                                    .unwrap_or(false);

                                if !has_art {
                                    if let Ok(art_path) = cover_storage::save_album_art(album_id, art_bytes) {
                                        let _ = queries::update_album_art_path(&conn, album_id, Some(&art_path));
                                    }
                                }
                            }
                        }
                    }
                    Ok(_) => {} // duplicate skipped
                    Err(e) => {
                        errors.push(format!("DB error for {}: {}", path_str, e));
                    }
                }
            }
            None => {
                errors.push(format!("Failed to read metadata: {}", path_str));
            }
        }
    }

    // Process removals (individual files)
    for path in &files_to_remove {
        let path_str = match path.to_str() {
            Some(s) => s,
            None => continue,
        };

        match delete_track_by_path(&conn, path_str) {
            Ok(true) => {
                removed += 1;
            }
            Ok(false) => {
                // Track wasn't in DB — no-op
            }
            Err(e) => {
                errors.push(format!("DB remove error for {}: {}", path_str, e));
            }
        }
    }

    // Process directory removals — delete all tracks whose path starts with the removed dir
    for dir_path in &dirs_removed {
        let dir_str = match dir_path.to_str() {
            Some(s) => s,
            None => continue,
        };

        match delete_tracks_by_directory(&conn, dir_str) {
            Ok(count) => {
                removed += count;
                if count > 0 {
                    tracing::info!("[Watcher] Removed {} tracks from deleted dir: {}", count, dir_str);
                }
            }
            Err(e) => {
                errors.push(format!("DB dir remove error for {}: {}", dir_str, e));
            }
        }
    }

    // Cleanup empty albums if we removed tracks
    if removed > 0 {
        let _ = queries::cleanup_empty_albums(&conn);
    }

    // Drop the lock before emitting
    drop(conn);

    if added > 0 || updated > 0 || removed > 0 {
        let event = WatcherChangeEvent {
            added,
            updated,
            removed,
            errors,
        };
        tracing::info!(
            "[Watcher] Changes: +{} added, ~{} updated, -{} removed",
            added,
            updated,
            removed
        );
        let _ = app_handle.emit("watcher-files-changed", &event);
    }
}

/// Delete a track from the database by its file path.
/// Returns true if a track was actually deleted.
fn delete_track_by_path(conn: &Connection, path: &str) -> Result<bool, rusqlite::Error> {
    let deleted = conn.execute("DELETE FROM tracks WHERE path = ?1", rusqlite::params![path])?;
    Ok(deleted > 0)
}

/// Delete all tracks whose path starts with the given directory path.
/// Used when a whole folder is removed/moved.
/// Returns the number of tracks deleted.
fn delete_tracks_by_directory(conn: &Connection, dir_path: &str) -> Result<usize, rusqlite::Error> {
    // Ensure the pattern ends with a path separator so we don't match partial names
    let pattern = if dir_path.ends_with('\\') || dir_path.ends_with('/') {
        format!("{}%", dir_path)
    } else {
        format!("{}\\%", dir_path) // Windows separator
    };
    let deleted = conn.execute(
        "DELETE FROM tracks WHERE path LIKE ?1",
        rusqlite::params![pattern],
    )?;
    Ok(deleted)
}
