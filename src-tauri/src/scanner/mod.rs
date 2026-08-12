// Scanner module for file walking, metadata extraction, and cover storage
pub mod walker;
pub mod metadata;
pub mod cover_storage;
#[cfg(desktop)]
pub mod watcher;

pub use walker::scan_directory;
pub use metadata::extract_metadata;