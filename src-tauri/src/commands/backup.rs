use crate::db::Database;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    pub filename: String,
    pub date: String,
    pub size_bytes: u64,
}

/// Get the backups directory path (inside app data dir)
fn backups_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    Ok(app_dir.join("backups"))
}

/// Get the main database file path
fn db_path(app: &AppHandle) -> Result<PathBuf, String> {
    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    Ok(app_dir.join("rlist.db"))
}

/// Run daily backup — called on app startup.
/// Creates a backup if one doesn't exist for today, and cleans up old backups (>7 days).
pub fn run_daily_backup(app: &AppHandle, db: &Database) -> Result<(), String> {
    let backup_dir = backups_dir(app)?;
    std::fs::create_dir_all(&backup_dir)
        .map_err(|e| format!("Failed to create backups dir: {}", e))?;

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let backup_file = backup_dir.join(format!("rlist_{}.db", today));

    if !backup_file.exists() {
        // WAL checkpoint to flush pending writes to the main DB file
        {
            let conn = db.conn.lock().map_err(|e| e.to_string())?;
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                .map_err(|e| format!("WAL checkpoint failed: {}", e))?;
        }

        let source = db_path(app)?;
        std::fs::copy(&source, &backup_file)
            .map_err(|e| format!("Failed to copy DB for backup: {}", e))?;

        tracing::info!("Daily backup created: {}", backup_file.display());
    }

    // Cleanup backups older than 7 days
    if let Ok(entries) = std::fs::read_dir(&backup_dir) {
        let cutoff = chrono::Local::now() - chrono::Duration::days(7);
        let cutoff_str = cutoff.format("%Y-%m-%d").to_string();

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(date) = name.strip_prefix("rlist_").and_then(|s| s.strip_suffix(".db")) {
                if date < cutoff_str.as_str() {
                    let _ = std::fs::remove_file(entry.path());
                    tracing::info!("Removed old backup: {}", name);
                }
            }
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn list_backups(app: AppHandle) -> Result<Vec<BackupInfo>, String> {
    let backup_dir = backups_dir(&app)?;

    if !backup_dir.exists() {
        return Ok(vec![]);
    }

    let mut backups: Vec<BackupInfo> = std::fs::read_dir(&backup_dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            let date = name
                .strip_prefix("rlist_")
                .and_then(|s| s.strip_suffix(".db"))?
                .to_string();
            let size = entry.metadata().ok()?.len();
            Some(BackupInfo {
                filename: name,
                date,
                size_bytes: size,
            })
        })
        .collect();

    backups.sort_by(|a, b| b.date.cmp(&a.date));
    Ok(backups)
}

#[tauri::command]
pub async fn export_backup(
    app: AppHandle,
    db: State<'_, Database>,
    destination: String,
) -> Result<(), String> {
    // WAL checkpoint first
    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|e| format!("WAL checkpoint failed: {}", e))?;
    }

    let source = db_path(&app)?;
    std::fs::copy(&source, &destination)
        .map_err(|e| format!("Failed to export backup: {}", e))?;

    tracing::info!("Backup exported to: {}", destination);
    Ok(())
}

#[tauri::command]
pub async fn import_backup(
    app: AppHandle,
    db: State<'_, Database>,
    source_path: String,
) -> Result<(), String> {
    let source = PathBuf::from(&source_path);

    if !source.exists() {
        return Err("Backup file does not exist".to_string());
    }

    // Validate it's a valid SQLite database
    {
        let test_conn = rusqlite::Connection::open(&source)
            .map_err(|e| format!("Invalid database file: {}", e))?;
        // Check it has the expected tables
        let has_tracks: bool = test_conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='tracks'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .map_err(|e| format!("Failed to validate backup: {}", e))?;

        if !has_tracks {
            return Err("Invalid backup: missing 'tracks' table".to_string());
        }
    }

    // Create safety backup of current DB before replacing
    let target = db_path(&app)?;
    let safety = target.with_extension("db.pre-import");

    // WAL checkpoint current DB
    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    }

    // Copy current DB as safety backup
    if target.exists() {
        std::fs::copy(&target, &safety)
            .map_err(|e| format!("Failed to create safety backup: {}", e))?;
    }

    // Replace the DB file
    std::fs::copy(&source, &target)
        .map_err(|e| format!("Failed to import backup: {}", e))?;

    // Remove WAL/SHM files so SQLite doesn't try to replay old journal
    let wal = target.with_extension("db-wal");
    let shm = target.with_extension("db-shm");
    let _ = std::fs::remove_file(&wal);
    let _ = std::fs::remove_file(&shm);

    tracing::info!("Backup imported from: {}. Restarting app...", source_path);

    // Restart the app
    app.restart();
}

#[tauri::command]
pub async fn delete_backup(app: AppHandle, filename: String) -> Result<(), String> {
    // Sanitize filename to prevent path traversal
    if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
        return Err("Invalid filename".to_string());
    }

    let backup_dir = backups_dir(&app)?;
    let path = backup_dir.join(&filename);

    if !path.exists() {
        return Err("Backup not found".to_string());
    }

    std::fs::remove_file(&path).map_err(|e| format!("Failed to delete backup: {}", e))?;
    tracing::info!("Backup deleted: {}", filename);
    Ok(())
}
