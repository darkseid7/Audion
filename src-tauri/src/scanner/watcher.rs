// File system watcher for automatic library updates (desktop only)
//
// Uses the `notify` crate to watch registered music folders for changes.
// Events are debounced (2 seconds) and processed incrementally:
//   - New audio files  → extract metadata + insert into DB
//   - Modified files   → extract metadata + update in DB
//   - Deleted files    → remove from DB + cleanup empty albums

use crate::db::queries::{self, TrackInsert};
use crate::scanner::metadata::extract_metadata;
use crate::scanner::walker::is_supported_audio_file;
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

    for event in &events {
        use notify::EventKind;

        for path in &event.paths {
            if !is_supported_audio_file(path) {
                continue;
            }

            match &event.kind {
                EventKind::Create(_) | EventKind::Modify(_) => {
                    // If a file was created/modified, make sure it's not in the remove set
                    files_to_remove.remove(path);
                    files_to_upsert.insert(path.clone());
                }
                EventKind::Remove(_) => {
                    // If it was queued for upsert, cancel that
                    files_to_upsert.remove(path);
                    files_to_remove.insert(path.clone());
                }
                _ => {}
            }
        }
    }

    if files_to_upsert.is_empty() && files_to_remove.is_empty() {
        return;
    }

    tracing::info!(
        "[Watcher] Processing {} upsert(s), {} removal(s)",
        files_to_upsert.len(),
        files_to_remove.len()
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
                    Ok((_id, was_new)) => {
                        if was_new {
                            added += 1;
                        } else {
                            updated += 1;
                        }
                    }
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

    // Process removals
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
