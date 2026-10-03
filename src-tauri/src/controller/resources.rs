//! Opaque, authorized managed-image resources; never a filesystem or URL proxy.
use super::{commands::error, protocol::*};
#[derive(Clone, Debug)]
pub struct MediaBytes {
    pub mime: &'static str,
    pub bytes: Vec<u8>,
}

pub const MAX_ARTWORK_BYTES: usize = 5 * 1024 * 1024;
pub fn validate_image(bytes: Vec<u8>) -> Result<MediaBytes, ControlError> {
    if bytes.len() > MAX_ARTWORK_BYTES {
        return Err(error(ControlErrorCode::TooLarge));
    }
    let format = image::guess_format(&bytes).map_err(|_| error(ControlErrorCode::Unsupported))?;
    let mime = match format {
        image::ImageFormat::Png => "image/png",
        image::ImageFormat::Jpeg => "image/jpeg",
        image::ImageFormat::WebP => "image/webp",
        _ => return Err(error(ControlErrorCode::Unsupported)),
    };
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let decoder = reader
        .into_decoder()
        .map_err(|_| error(ControlErrorCode::InvalidRequest))?;
    use image::ImageDecoder;
    let (width, height) = decoder.dimensions();
    if width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || decoder.total_bytes() > 64 * 1024 * 1024
    {
        return Err(error(ControlErrorCode::TooLarge));
    }
    image::DynamicImage::from_decoder(decoder)
        .map_err(|_| error(ControlErrorCode::InvalidRequest))?;
    Ok(MediaBytes { mime, bytes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    #[test]
    fn valid_png_is_decoded_not_only_sniffed() {
        let bytes=base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=").unwrap();
        let media = validate_image(bytes).unwrap();
        assert_eq!(media.mime, "image/png");
    }

    #[test]
    fn jpeg_webp_decode_and_dimension_limits_are_real() {
        for (format, mime) in [
            (image::ImageFormat::Jpeg, "image/jpeg"),
            (image::ImageFormat::WebP, "image/webp"),
        ] {
            let mut data = std::io::Cursor::new(Vec::new());
            image::DynamicImage::new_rgb8(2, 2)
                .write_to(&mut data, format)
                .unwrap();
            assert_eq!(validate_image(data.into_inner()).unwrap().mime, mime);
        }
        let mut data = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(4097, 1)
            .write_to(&mut data, image::ImageFormat::Png)
            .unwrap();
        assert!(validate_image(data.into_inner()).is_err());
    }
    #[test]
    fn oversize_artwork_and_audio_are_denied() {
        assert!(matches!(
            validate_image(vec![0; 5 * 1024 * 1024 + 1]),
            Err(ControlError {
                code: ControlErrorCode::TooLarge,
                ..
            })
        ));
        assert!(validate_image(b"ID3fake audio".to_vec()).is_err());
        assert!(validate_image(b"\x89PNG\r\n\x1a\nfake".to_vec()).is_err());
    }
}

#[cfg(test)]
mod registry_tests {
    use super::*;
    use crate::{controller::queries::QueryContext, db::Database};
    use rusqlite::Connection;
    use std::sync::{Arc, Mutex};
    #[tokio::test]
    async fn registry_evicts_old_opaque_references_at_2048_without_retaining_payloads() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::init_schema(&conn).unwrap();
        conn.execute(
            "INSERT INTO tracks(id,path,track_cover) VALUES(1,'private','embedded')",
            [],
        )
        .unwrap();
        let db = Database {
            conn: Arc::new(Mutex::new(conn)),
        };
        let r = ManagedResources::new(db.clone(), PathBuf::new());
        let mut c = QueryContext {
            host_epoch: "epoch".into(),
            ..Default::default()
        };
        let first = r
            .register(&db.conn.lock().unwrap(), Entity::Track(1), &c)
            .unwrap()
            .unwrap();
        for revision in 1..=2048 {
            c.revision = revision;
            r.register(&db.conn.lock().unwrap(), Entity::Track(1), &c)
                .unwrap();
        }
        assert_eq!(r.state.lock().unwrap().entries.len(), 2048);
        assert!(r.state.lock().unwrap().cache.is_empty());
        assert_eq!(
            r.read_resource(first, c.clone()).await.unwrap_err().code,
            ControlErrorCode::NotFound
        );
        let current = r
            .register(&db.conn.lock().unwrap(), Entity::Track(1), &c)
            .unwrap()
            .unwrap();
        assert_eq!(
            current,
            r.register(&db.conn.lock().unwrap(), Entity::Track(1), &c)
                .unwrap()
                .unwrap()
        );
    }
    #[tokio::test]
    async fn registry_rejects_unknown_stale_deleted_and_audio_resources() {
        let root = std::env::temp_dir().join(format!("audion-art-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("tracks")).unwrap();
        let path = root.join("tracks/1.png");
        use base64::Engine;
        let png=base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=").unwrap();
        std::fs::write(&path, &png).unwrap();
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::init_schema(&conn).unwrap();
        conn.execute(
            "INSERT INTO tracks(id,path,track_cover_path) VALUES(1,'private audio',?1)",
            [path.to_str().unwrap()],
        )
        .unwrap();
        let db = Database {
            conn: Arc::new(Mutex::new(conn)),
        };
        let r = ManagedResources::new(db.clone(), root.clone());
        let mut c = QueryContext {
            host_epoch: "e".into(),
            revision: 2,
            stamp: crate::db::queries::controller_library_stamp(&db.conn.lock().unwrap()).unwrap(),
            ..Default::default()
        };
        let reference = r
            .register(&db.conn.lock().unwrap(), Entity::Track(1), &c)
            .unwrap()
            .unwrap();
        assert_eq!(
            r.read_resource(reference.clone(), c.clone())
                .await
                .unwrap()
                .mime,
            "image/png"
        );
        let mut replacement = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2, 1)
            .write_to(&mut replacement, image::ImageFormat::Png)
            .unwrap();
        std::fs::write(&path, replacement.get_ref()).unwrap();
        assert_eq!(
            r.read_resource(reference.clone(), c.clone())
                .await
                .unwrap_err()
                .code,
            ControlErrorCode::RevisionConflict
        );
        crate::db::queries::update_track_cover_path(&db.conn.lock().unwrap(), 1, path.to_str())
            .unwrap();
        c.stamp = crate::db::queries::controller_library_stamp(&db.conn.lock().unwrap()).unwrap();
        c.revision += 1;
        assert_eq!(
            r.read_resource(reference.clone(), c.clone())
                .await
                .unwrap_err()
                .code,
            ControlErrorCode::RevisionConflict
        );
        let changed_reference = r
            .register(&db.conn.lock().unwrap(), Entity::Track(1), &c)
            .unwrap()
            .unwrap();
        assert_ne!(reference.resource_id, changed_reference.resource_id);
        assert_eq!(
            r.read_resource(changed_reference.clone(), c.clone())
                .await
                .unwrap()
                .bytes,
            replacement.into_inner()
        );
        let reference = changed_reference;

        assert!(r
            .read_resource(
                ArtworkReference {
                    resource_id: "unknown".into(),
                    revision: 2
                },
                c.clone()
            )
            .await
            .is_err());
        let mut changed = c.clone();
        changed.revision += 1;
        assert!(r.read_resource(reference.clone(), changed).await.is_err());
        assert!(read_managed(
            &root,
            Some(&root.join("wrong")),
            path.to_str().unwrap(),
            Entity::Track(1)
        )
        .is_err());
        std::fs::remove_file(&path).unwrap();
        assert!(r.read_resource(reference.clone(), c.clone()).await.is_err());
        std::fs::write(&path, b"ID3 audio masquerading as PNG").unwrap();
        assert!(r.read_resource(reference, c).await.is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(root.join("tracks")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}

use crate::{controller::queries::QueryContext, db::Database};
use std::path::PathBuf;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Entity {
    Track(u64),
    Album(u64),
}

use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    io::Read,
    path::Path,
    sync::{Arc, Mutex},
};
#[derive(Clone, PartialEq)]
enum Source {
    File(String),
    Embedded(String),
}
#[derive(Clone)]
struct Registered {
    entity: Entity,
    epoch: String,
    revision: u64,
    stamp: u64,
    source: [u8; 32],
    bytes: Option<[u8; 32]>,
}
#[derive(Default)]
struct Registry {
    entries: HashMap<String, Registered>,
    order: VecDeque<String>,
    cache: HashMap<[u8; 32], MediaBytes>,
    cache_order: VecDeque<[u8; 32]>,
    cache_bytes: usize,
}
pub struct ManagedResources {
    db: Database,
    root: PathBuf,
    approved_root: Option<PathBuf>,
    state: Arc<Mutex<Registry>>,
    work: Arc<tokio::sync::Semaphore>,
}
impl ManagedResources {
    pub fn new(db: Database, root: PathBuf) -> Self {
        Self {
            db,
            approved_root: root
                .parent()
                .and_then(|p| p.canonicalize().ok())
                .zip(root.file_name())
                .map(|(p, name)| p.join(name)),
            root,
            state: Arc::new(Mutex::new(Registry::default())),
            work: Arc::new(tokio::sync::Semaphore::new(4)),
        }
    }
    pub fn register(
        &self,
        conn: &rusqlite::Connection,
        entity: Entity,
        c: &QueryContext,
    ) -> Result<Option<ArtworkReference>, ControlError> {
        let Some(source) = source(conn, entity)? else {
            return Ok(None);
        };
        // Do not advertise provider/custom URLs or arbitrary filesystem references.
        if let Source::File(path) = &source {
            if !managed_name(&self.root, path, entity) {
                return Ok(None);
            }
        }
        let source = source_hash(&source);
        let mut state = self
            .state
            .lock()
            .map_err(|_| error(ControlErrorCode::HostNotReady))?;
        if let Some((id, _)) = state.entries.iter().find(|(_, e)| {
            e.entity == entity
                && e.epoch == c.host_epoch
                && e.revision == c.revision
                && e.stamp == c.stamp
                && e.source == source
        }) {
            return Ok(Some(ArtworkReference {
                resource_id: id.clone(),
                revision: c.revision,
            }));
        }
        while state.entries.len() >= 2048 {
            if let Some(id) = state.order.pop_front() {
                state.entries.remove(&id);
            }
        }
        let id = uuid::Uuid::new_v4().to_string();
        state.order.push_back(id.clone());
        state.entries.insert(
            id.clone(),
            Registered {
                entity,
                epoch: c.host_epoch.clone(),
                revision: c.revision,
                stamp: c.stamp,
                source,
                bytes: None,
            },
        );
        Ok(Some(ArtworkReference {
            resource_id: id,
            revision: c.revision,
        }))
    }
    pub(crate) async fn read_resource(
        &self,
        r: ArtworkReference,
        c: QueryContext,
    ) -> Result<MediaBytes, ControlError> {
        let permit = self
            .work
            .clone()
            .try_acquire_owned()
            .map_err(|_| error(ControlErrorCode::Busy))?;
        let entry = self
            .state
            .lock()
            .map_err(|_| error(ControlErrorCode::HostNotReady))?
            .entries
            .get(&r.resource_id)
            .cloned()
            .ok_or_else(|| error(ControlErrorCode::NotFound))?;
        if r.revision != entry.revision
            || entry.epoch != c.host_epoch
            || entry.revision != c.revision
            || entry.stamp != c.stamp
        {
            return Err(error(ControlErrorCode::RevisionConflict));
        }
        let db = self.db.clone();
        let root = self.root.clone();
        let approved_root = self.approved_root.clone();
        let state = self.state.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let source = {
                let conn = db
                    .conn
                    .try_lock()
                    .map_err(|_| error(ControlErrorCode::Busy))?;
                if crate::db::queries::controller_library_stamp(&conn)
                    .map_err(|_| error(ControlErrorCode::ExecutionFailed))?
                    != entry.stamp
                {
                    return Err(error(ControlErrorCode::RevisionConflict));
                }
                let source = source(&conn, entry.entity)?
                    .ok_or_else(|| error(ControlErrorCode::NotFound))?;
                if source_hash(&source) != entry.source {
                    return Err(error(ControlErrorCode::RevisionConflict));
                }
                source
            };
            // DB/state locks are dropped before filesystem I/O and image decoding.
            let bytes = match source {
                Source::File(path) => {
                    read_managed(&root, approved_root.as_deref(), &path, entry.entity)?
                }
                Source::Embedded(data) => {
                    use base64::Engine;
                    if data == "!" || data.len() > MAX_ARTWORK_BYTES.div_ceil(3) * 4 {
                        return Err(error(ControlErrorCode::TooLarge));
                    }
                    base64::engine::general_purpose::STANDARD
                        .decode(data)
                        .map_err(|_| error(ControlErrorCode::InvalidRequest))?
                }
            };
            let hash: [u8; 32] = Sha256::digest(&bytes).into();
            {
                let mut registry = state
                    .lock()
                    .map_err(|_| error(ControlErrorCode::HostNotReady))?;
                let registered = registry
                    .entries
                    .get_mut(&r.resource_id)
                    .ok_or_else(|| error(ControlErrorCode::NotFound))?;
                if registered.bytes.is_some_and(|old| old != hash) {
                    return Err(error(ControlErrorCode::RevisionConflict));
                }
                registered.bytes = Some(hash);
            }
            if let Some(cached) = state
                .lock()
                .map_err(|_| error(ControlErrorCode::HostNotReady))?
                .cache
                .get(&hash)
                .cloned()
            {
                return Ok(cached);
            }
            let media = validate_image(bytes)?;
            let mut state = state
                .lock()
                .map_err(|_| error(ControlErrorCode::HostNotReady))?;
            while state.cache_bytes + media.bytes.len() > 50 * 1024 * 1024 {
                if let Some(old) = state.cache_order.pop_front() {
                    if let Some(v) = state.cache.remove(&old) {
                        state.cache_bytes -= v.bytes.len();
                    }
                } else {
                    break;
                }
            }
            if !state.cache.contains_key(&hash) {
                state.cache_bytes += media.bytes.len();
                state.cache_order.push_back(hash);
                state.cache.insert(hash, media.clone());
            }
            Ok(media)
        })
        .await
        .map_err(|_| error(ControlErrorCode::HostNotReady))?
    }
}
fn source_hash(source: &Source) -> [u8; 32] {
    match source {
        Source::File(s) => Sha256::digest(format!("file:{s}").as_bytes()).into(),
        Source::Embedded(s) => Sha256::digest(s.as_bytes()).into(),
    }
}
fn source(conn: &rusqlite::Connection, entity: Entity) -> Result<Option<Source>, ControlError> {
    use rusqlite::OptionalExtension;
    let(sql,id)=match entity{Entity::Track(id)=>("SELECT track_cover_path, CASE WHEN length(track_cover)<=6990508 THEN track_cover ELSE '!' END FROM tracks WHERE id=?1",id),Entity::Album(id)=>("SELECT art_path, CASE WHEN length(art_data)<=6990508 THEN art_data ELSE '!' END FROM albums WHERE id=?1",id)};
    let row = conn
        .query_row(sql, [id], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<String>>(1)?,
            ))
        })
        .optional()
        .map_err(|_| error(ControlErrorCode::ExecutionFailed))?;
    Ok(row.and_then(|(path, data)| {
        path.filter(|p| !p.is_empty())
            .map(Source::File)
            .or_else(|| data.filter(|d| !d.is_empty()).map(Source::Embedded))
    }))
}
fn managed_name(root: &Path, path: &str, entity: Entity) -> bool {
    let (dir, id) = match entity {
        Entity::Track(id) => ("tracks", id),
        Entity::Album(id) => ("albums", id),
    };
    let p = Path::new(path);
    let Some(ext) = p.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    ["png", "jpg", "jpeg", "webp"].contains(&ext.to_ascii_lowercase().as_str())
        && p == root.join(dir).join(format!("{id}.{ext}"))
}
fn read_managed(
    root: &Path,
    approved_root: Option<&Path>,
    path: &str,
    entity: Entity,
) -> Result<Vec<u8>, ControlError> {
    if !managed_name(root, path, entity) {
        return Err(error(ControlErrorCode::PermissionRequired));
    }
    let io_error = |_: std::io::Error| error(ControlErrorCode::NotFound);
    let path = Path::new(path);
    for p in [root, path.parent().unwrap(), path] {
        let m = std::fs::symlink_metadata(p).map_err(io_error)?;
        if m.file_type().is_symlink() {
            return Err(error(ControlErrorCode::PermissionRequired));
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if m.file_attributes() & 0x400 != 0 {
                return Err(error(ControlErrorCode::PermissionRequired));
            }
        }
    }
    let canonical_root = root.canonicalize().map_err(io_error)?;
    if approved_root != Some(canonical_root.as_path()) {
        return Err(error(ControlErrorCode::PermissionRequired));
    }
    let canonical = path.canonicalize().map_err(io_error)?;
    if !canonical.starts_with(&canonical_root) {
        return Err(error(ControlErrorCode::PermissionRequired));
    }
    let file = std::fs::File::open(path).map_err(io_error)?;
    // Verify the opened handle, not merely the name checked before open.
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        #[link(name = "kernel32")]
        extern "system" {
            fn GetFinalPathNameByHandleW(
                h: *mut std::ffi::c_void,
                path: *mut u16,
                len: u32,
                flags: u32,
            ) -> u32;
        }
        let mut buf = vec![0u16; 32768];
        let len = unsafe {
            GetFinalPathNameByHandleW(file.as_raw_handle(), buf.as_mut_ptr(), buf.len() as u32, 0)
        };
        if len == 0 || len as usize >= buf.len() {
            return Err(error(ControlErrorCode::PermissionRequired));
        }
        use std::os::windows::ffi::OsStringExt;
        let opened = PathBuf::from(std::ffi::OsString::from_wide(&buf[..len as usize]));
        if opened != canonical || !opened.starts_with(&canonical_root) {
            return Err(error(ControlErrorCode::PermissionRequired));
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let opened = file.metadata().map_err(io_error)?;
        let current = std::fs::metadata(&canonical).map_err(io_error)?;
        if opened.dev() != current.dev() || opened.ino() != current.ino() {
            return Err(error(ControlErrorCode::PermissionRequired));
        }
    }
    if !file.metadata().map_err(io_error)?.is_file() {
        return Err(error(ControlErrorCode::PermissionRequired));
    }
    if file.metadata().map_err(io_error)?.len() > MAX_ARTWORK_BYTES as u64 {
        return Err(error(ControlErrorCode::TooLarge));
    }
    let mut bytes = Vec::new();
    file.take(MAX_ARTWORK_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > MAX_ARTWORK_BYTES {
        return Err(error(ControlErrorCode::TooLarge));
    }
    Ok(bytes)
}
