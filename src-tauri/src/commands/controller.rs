//! Window-scoped IPC only. LAN requests never invoke application command names.
use crate::controller::protocol::{
    CommandEnvelope, ControlError, ControlErrorCode, ExecutionResult,
};
use serde::{Deserialize, Serialize};

fn local_main(label: &str, url: &url::Url, dev: Option<&url::Url>) -> bool {
    label == "main"
        && ((url.scheme() == "tauri"
            && url.host_str() == Some("localhost")
            && url.port().is_none())
            || (matches!(url.scheme(), "http" | "https")
                && url.host_str() == Some("tauri.localhost")
                && url.port().is_none())
            || dev.is_some_and(|dev| dev.origin() == url.origin()))
}
fn rejected(code: ControlErrorCode) -> ControlError {
    ControlError {
        code,
        message: "Controller operation is unavailable.".into(),
        retryable: false,
    }
}

fn authorize(window: &tauri::WebviewWindow, app: &tauri::AppHandle) -> Result<(), ControlError> {
    #[cfg(debug_assertions)]
    let dev = app.config().build.dev_url.as_ref();
    #[cfg(not(debug_assertions))]
    let dev = None;
    let url = window
        .url()
        .map_err(|_| rejected(ControlErrorCode::Unauthorized))?;
    if !local_main(window.label(), &url, dev) {
        return Err(rejected(ControlErrorCode::Unauthorized));
    }
    Ok(())
}

#[cfg(desktop)]
mod desktop {
    use super::*;
    use crate::controller::protocol::HostUpdate;
    use crate::controller::{
        commands::{AuthoritativeWindow, CoordinatorLease, ExecutionTicket},
        host::{start_host, HostDependencies, HostHandle, LanConfig},
        identity::load_or_create_identity,
        pairing::{create_invitation, Grants},
        secrets::NativeSecretStore,
    };
    use std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex, OnceLock,
        },
        time::SystemTime,
    };
    use tauri::{Emitter, Manager};
    use uuid::Uuid;
    #[derive(Default)]
    pub struct NativeHostState {
        dependencies: OnceLock<Arc<HostDependencies>>,
        library: OnceLock<Arc<crate::controller::queries::LibraryQueries>>,
        library_initialization: Mutex<()>,
        initialization: Mutex<()>,
        stopping: AtomicBool,
        listener: tokio::sync::Mutex<Option<HostHandle>>,
    }
    impl NativeHostState {
        pub fn stop(&self) {
            self.stopping.store(true, Ordering::Release);
            if let Some(deps) = self.dependencies.get() {
                deps.commands.stop();
            }
        }
        fn library_with(
            &self,
            initialize: impl FnOnce() -> Result<
                Arc<crate::controller::queries::LibraryQueries>,
                ControlError,
            >,
        ) -> Result<Arc<crate::controller::queries::LibraryQueries>, ControlError> {
            if self.stopping.load(Ordering::Acquire) {
                return Err(rejected(ControlErrorCode::HostNotReady));
            }
            if let Some(library) = self.library.get() {
                return Ok(library.clone());
            }
            let _guard = self
                .library_initialization
                .try_lock()
                .map_err(|_| rejected(ControlErrorCode::Busy))?;
            if self.stopping.load(Ordering::Acquire) {
                return Err(rejected(ControlErrorCode::HostNotReady));
            }
            if self.library.get().is_none() {
                let _ = self.library.set(initialize()?);
            }
            if self.stopping.load(Ordering::Acquire) {
                return Err(rejected(ControlErrorCode::HostNotReady));
            }
            Ok(self.library.get().unwrap().clone())
        }
        fn library(
            &self,
            app: &tauri::AppHandle,
        ) -> Result<Arc<crate::controller::queries::LibraryQueries>, ControlError> {
            self.library_with(|| {
                let db = app
                    .try_state::<crate::db::Database>()
                    .ok_or_else(|| rejected(ControlErrorCode::HostNotReady))?
                    .inner()
                    .clone();
                let root = app
                    .path()
                    .app_data_dir()
                    .map_err(|_| rejected(ControlErrorCode::HostNotReady))?
                    .join("covers");
                Ok(Arc::new(
                    crate::controller::queries::LibraryQueries::with_root(db, root),
                ))
            })
        }
        fn registration_dependencies(
            &self,
            request: &Registration,
            initialize: impl FnOnce() -> Result<Arc<HostDependencies>, ControlError>,
        ) -> Result<Option<Arc<HostDependencies>>, ControlError> {
            if self.stopping.load(Ordering::Acquire) {
                return Err(rejected(ControlErrorCode::HostNotReady));
            }
            if matches!(
                request,
                Registration::DesktopLibraryQuery { .. } | Registration::DesktopArtwork { .. }
            ) {
                return Ok(None);
            }
            initialize().map(Some)
        }
        fn dependencies(&self) -> Result<Arc<HostDependencies>, ControlError> {
            self.dependencies_with(|| {
                let store = Arc::new(NativeSecretStore);
                let identity = load_or_create_identity(store.as_ref())?;
                Ok(Arc::new(HostDependencies::new(identity, store)?))
            })
        }
        fn dependencies_with(
            &self,
            initialize: impl FnOnce() -> Result<Arc<HostDependencies>, ControlError>,
        ) -> Result<Arc<HostDependencies>, ControlError> {
            if self.stopping.load(Ordering::Acquire) {
                return Err(rejected(ControlErrorCode::HostNotReady));
            }
            let _initialization = self
                .initialization
                .lock()
                .map_err(|_| rejected(ControlErrorCode::HostNotReady))?;
            if self.stopping.load(Ordering::Acquire) {
                return Err(rejected(ControlErrorCode::HostNotReady));
            }
            if self.dependencies.get().is_none() {
                let deps = initialize()?;
                let _ = self.dependencies.set(deps);
            }
            let deps = self.dependencies.get().unwrap();
            // Covers stop racing a still-running protected-storage initialization.
            if self.stopping.load(Ordering::Acquire) {
                deps.commands.stop();
                return Err(rejected(ControlErrorCode::HostNotReady));
            }
            Ok(deps.clone())
        }
        pub fn invalidate_window(&self, label: &str) {
            if self.stopping.load(Ordering::Acquire) {
                return;
            }
            if let Some(deps) = self.dependencies.get() {
                deps.commands.invalidate_window(label);
            }
        }
    }
    #[derive(Deserialize)]
    #[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
    pub enum Registration {
        Prepare {},
        // serde Option accepts both omission and null as revision-only; all other shapes remain closed.
        DesktopLibraryQuery {
            query: Option<crate::controller::protocol::ApplicationQuery>,
            #[serde(rename = "pinnedAlbumIds")]
            pinned_album_ids: Vec<u64>,
        },
        DesktopArtwork {
            reference: crate::controller::protocol::ArtworkReference,
        },
        LibraryRevision {
            lease: CoordinatorLease,
        },
        LibraryQuery {
            lease: CoordinatorLease,
            query: crate::controller::protocol::ApplicationQuery,
        },
        Artwork {
            lease: CoordinatorLease,
            reference: crate::controller::protocol::ArtworkReference,
        },
        Ready {
            lease: CoordinatorLease,
        },
        Release {
            lease: CoordinatorLease,
        },
        Publish {
            lease: CoordinatorLease,
            update: HostUpdate,
        },
    }
    #[derive(Serialize)]
    #[serde(
        tag = "type",
        rename_all = "snake_case",
        rename_all_fields = "camelCase"
    )]
    pub enum Registered {
        Registered {
            host_id: String,
            lease: CoordinatorLease,
            #[serde(skip_serializing_if = "Option::is_none")]
            revision: Option<u64>,
        },
        LibraryRevision {
            revision: u64,
        },
        LibraryQuery {
            result: crate::controller::protocol::QueryResult,
        },
        Artwork {
            mime: String,
            base64: String,
        },
    }
    async fn desktop_registration(
        library: &crate::controller::queries::LibraryQueries,
        request: Registration,
    ) -> Result<Registered, ControlError> {
        use crate::controller::queries::{DesktopLibraryAuthority, LibraryQueries};
        let authority = DesktopLibraryAuthority::new();
        match request {
            Registration::DesktopLibraryQuery {
                query,
                pinned_album_ids,
            } => {
                let context = library
                    .desktop_context(&authority, pinned_album_ids)
                    .await?;
                let Some(query) = query else {
                    return Ok(Registered::LibraryRevision {
                        revision: context.revision,
                    });
                };
                let result = library.query_library(query, context.clone()).await?;
                let current = library.desktop_current(&authority).await?;
                LibraryQueries::check_context(&context, &current)?;
                Ok(Registered::LibraryQuery { result })
            }
            Registration::DesktopArtwork { reference } => {
                let context = library.desktop_current(&authority).await?;
                let media = library
                    .resources
                    .read_resource(reference, context.clone())
                    .await?;
                let current = library.desktop_current(&authority).await?;
                LibraryQueries::check_context(&context, &current)?;
                use base64::Engine;
                Ok(Registered::Artwork {
                    mime: media.mime.into(),
                    base64: base64::engine::general_purpose::STANDARD.encode(media.bytes),
                })
            }
            _ => Err(rejected(ControlErrorCode::InvalidRequest)),
        }
    }
    #[tauri::command]
    pub async fn control_host_register(
        app: tauri::AppHandle,
        window: tauri::WebviewWindow,
        request: Registration,
    ) -> Result<Registered, ControlError> {
        authorize(&window, &app)?;
        let state = app.state::<NativeHostState>();
        let deps = match state.registration_dependencies(&request, || state.dependencies())? {
            Some(deps) => deps,
            None => {
                let result = desktop_registration(state.library(&app)?.as_ref(), request).await?;
                if state.stopping.load(Ordering::Acquire) {
                    return Err(rejected(ControlErrorCode::HostNotReady));
                }
                return Ok(result);
            }
        };
        let mut revision = None;
        let lease = match request {
            Registration::DesktopLibraryQuery { .. } | Registration::DesktopArtwork { .. } => {
                return Err(rejected(ControlErrorCode::InvalidRequest))
            }
            Registration::LibraryRevision { lease } => {
                let context = deps
                    .commands
                    .library_context_async(Some((window.label(), &lease)))
                    .await?;
                return Ok(Registered::LibraryRevision {
                    revision: context.revision,
                });
            }
            Registration::LibraryQuery { lease, query } => {
                let context = deps
                    .commands
                    .library_context_async(Some((window.label(), &lease)))
                    .await?;
                let result = deps.commands.query_library(query).await?;
                let current = deps
                    .commands
                    .library_context_async(Some((window.label(), &lease)))
                    .await?;
                if context.host_epoch != current.host_epoch {
                    return Err(rejected(ControlErrorCode::ResyncRequired));
                }
                return Ok(Registered::LibraryQuery { result });
            }
            Registration::Artwork { lease, reference } => {
                let context = deps
                    .commands
                    .library_context_async(Some((window.label(), &lease)))
                    .await?;
                let media = deps
                    .commands
                    .library
                    .get()
                    .ok_or_else(|| rejected(ControlErrorCode::Unsupported))?
                    .resources
                    .read_resource(reference, context.clone())
                    .await?;
                let current = deps
                    .commands
                    .library_context_async(Some((window.label(), &lease)))
                    .await?;
                if context.host_epoch != current.host_epoch || context.revision != current.revision
                {
                    return Err(rejected(ControlErrorCode::RevisionConflict));
                }
                use base64::Engine;
                return Ok(Registered::Artwork {
                    mime: media.mime.into(),
                    base64: base64::engine::general_purpose::STANDARD.encode(media.bytes),
                });
            }
            Registration::Prepare {} => {
                if deps.commands.library.get().is_none() {
                    deps.commands.install_library(state.library(&app)?)?;
                }
                let target = window.clone();
                deps.commands
                    .register_coordinator(AuthoritativeWindow::new(
                        window.label().into(),
                        Arc::new(move |dispatch| {
                            // emit_to a label alone can address every WebView in a window.
                            // A WebviewWindow emitter targets this authoritative WebView.
                            target
                                .emit_to(
                                    tauri::EventTarget::webview_window(target.label()),
                                    "controller://dispatch",
                                    dispatch,
                                )
                                .map_err(|_| rejected(ControlErrorCode::HostNotReady))
                        }),
                    ))?
            }
            Registration::Publish { lease, update } => {
                revision = Some(deps.commands.publish(window.label(), &lease, update)?);
                lease
            }
            Registration::Ready { lease } => {
                deps.commands.ready(window.label(), &lease)?;
                deps.commands.start_library_observer(lease.clone());
                lease
            }
            Registration::Release { lease } => {
                deps.commands.release(window.label(), &lease)?;
                lease
            }
        };
        Ok(Registered::Registered {
            host_id: deps.identity.id().into(),
            lease,
            revision,
        })
    }
    #[derive(Deserialize)]
    #[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
    pub enum Completion {
        Authorize {
            lease: CoordinatorLease,
            ticket: ExecutionTicket,
        },
        Complete {
            lease: CoordinatorLease,
            ticket: ExecutionTicket,
            result: ExecutionResult,
        },
    }
    #[tauri::command]
    pub fn control_host_complete(
        app: tauri::AppHandle,
        window: tauri::WebviewWindow,
        request: Completion,
    ) -> Result<Option<CommandEnvelope>, ControlError> {
        authorize(&window, &app)?;
        let deps = app.state::<NativeHostState>().dependencies()?;
        match request {
            Completion::Authorize { lease, ticket } => deps
                .commands
                .claim(window.label(), &lease, ticket)
                .map(Some),
            Completion::Complete {
                lease,
                ticket,
                result,
            } => {
                deps.commands
                    .complete(window.label(), &lease, ticket, result)?;
                Ok(None)
            }
        }
    }
    #[derive(Deserialize)]
    #[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
    pub enum HostToggle {
        Enable { config: LanConfig },
        Disable {},
        Status {},
    }
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct HostStatus {
        enabled: bool,
        ready: bool,
        endpoint: Option<String>,
        pending: Option<Vec<PendingView>>,
        paired: Option<Vec<DeviceView>>,
    }
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct PendingView {
        id: Uuid,
        name: String,
        expires_in_seconds: u64,
    }
    #[derive(Serialize)]
    struct DeviceView {
        id: Uuid,
        name: String,
        grants: Grants,
    }
    impl NativeHostState {
        fn inspect(&self, listener: Option<&HostHandle>) -> Result<HostStatus, ControlError> {
            let enabled = listener.is_some_and(|host| host.running());
            let (ready, pending, paired) = if let Some(deps) = self.dependencies.get() {
                let pending = deps
                    .pairing
                    .pending_pairings()?
                    .into_iter()
                    .map(|p| PendingView {
                        id: p.id,
                        name: p.device_name,
                        expires_in_seconds: p
                            .expires_at
                            .duration_since(SystemTime::now())
                            .unwrap_or_default()
                            .as_secs(),
                    })
                    .collect();
                let paired = deps
                    .pairing
                    .paired_devices()?
                    .into_iter()
                    .map(|p| DeviceView {
                        id: p.id,
                        name: p.name,
                        grants: p.grants,
                    })
                    .collect();
                (
                    deps.commands.host_epoch().is_ok(),
                    Some(pending),
                    Some(paired),
                )
            } else {
                (false, None, None)
            };
            Ok(HostStatus {
                enabled,
                ready: enabled && ready,
                endpoint: listener
                    .filter(|_| enabled)
                    .map(|host| host.endpoint.to_string()),
                pending,
                paired,
            })
        }
    }
    #[tauri::command]
    pub async fn control_host_enable(
        app: tauri::AppHandle,
        window: tauri::WebviewWindow,
        request: HostToggle,
    ) -> Result<HostStatus, ControlError> {
        authorize(&window, &app)?;
        let state = app.state::<NativeHostState>();
        let mut listener = state.listener.lock().await;
        match request {
            HostToggle::Status {} => {}
            HostToggle::Disable {} => {
                state.invalidate_window(window.label());
                listener.take();
                let deps = state.dependencies.get().cloned();
                if let Some(deps) = deps {
                    deps.invalidate_pairing()?;
                }
            }
            HostToggle::Enable { config } => {
                if listener.as_ref().is_some_and(|host| host.running()) {
                    return Err(rejected(ControlErrorCode::Busy));
                }
                config.validate()?;
                *listener = Some(start_host(config, state.dependencies()?).await?);
            }
        }
        state.inspect(listener.as_ref())
    }
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Invitation {
        encoded: String,
        qr_svg: String,
    }
    #[tauri::command]
    pub async fn control_host_invitation(
        app: tauri::AppHandle,
        window: tauri::WebviewWindow,
    ) -> Result<Invitation, ControlError> {
        authorize(&window, &app)?;
        let state = app.state::<NativeHostState>();
        let listener = state.listener.lock().await;
        let host = listener
            .as_ref()
            .filter(|host| host.running())
            .ok_or_else(|| rejected(ControlErrorCode::HostNotReady))?;
        let deps = state.dependencies()?;
        deps.commands.host_epoch()?;
        let invitation = create_invitation(&deps.identity, host.endpoint, SystemTime::now())?;
        let encoded = invitation.encode()?;
        if encoded.len() > 2048 {
            return Err(rejected(ControlErrorCode::TooLarge));
        }
        deps.pairing.register_invitation(&invitation)?;
        Ok(Invitation {
            encoded: encoded.to_string(),
            qr_svg: invitation.qr_svg()?.to_string(),
        })
    }
    #[tauri::command]
    pub fn control_host_approve(
        app: tauri::AppHandle,
        window: tauri::WebviewWindow,
        pending_id: Uuid,
        grants: Grants,
    ) -> Result<(), ControlError> {
        authorize(&window, &app)?;
        app.state::<NativeHostState>()
            .dependencies()?
            .approve(pending_id, grants)
    }
    #[tauri::command]
    pub fn control_host_revoke(
        app: tauri::AppHandle,
        window: tauri::WebviewWindow,
        device_id: Uuid,
    ) -> Result<(), ControlError> {
        authorize(&window, &app)?;
        app.state::<NativeHostState>()
            .dependencies()?
            .revoke(device_id)
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::controller::secrets::tests::MemoryStore;
        #[tokio::test]
        async fn local_library_bypasses_failed_or_locked_protected_host_factory() {
            let state = NativeHostState::default();
            let conn = rusqlite::Connection::open_in_memory().unwrap();
            crate::db::schema::init_schema(&conn).unwrap();
            conn.execute("INSERT INTO tracks(id,path,track_cover) VALUES(1,'synthetic-private-audio',?1)",["iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII="]).unwrap();
            let db = crate::db::Database {
                conn: Arc::new(Mutex::new(conn)),
            };
            let library = state
                .library_with(|| {
                    Ok(Arc::new(crate::controller::queries::LibraryQueries::new(
                        db,
                    )))
                })
                .unwrap();
            assert!(state
                .dependencies_with(|| Err(rejected(ControlErrorCode::HostNotReady)))
                .is_err());
            let _held = state.initialization.lock().unwrap();
            for payload in [
                serde_json::json!({"phase":"desktop_library_query","pinnedAlbumIds":[]}),
                serde_json::json!({"phase":"desktop_library_query","query":{"type":"albums"},"pinnedAlbumIds":[]}),
            ] {
                let request: Registration = serde_json::from_value(payload).unwrap();
                assert!(state
                    .registration_dependencies(&request, || panic!(
                        "local request touched protected factory"
                    ))
                    .unwrap()
                    .is_none());
                let result = desktop_registration(&library, request).await.unwrap();
                assert!(matches!(
                    result,
                    Registered::LibraryRevision { .. } | Registered::LibraryQuery { .. }
                ));
            }
            let tracks=desktop_registration(&library,serde_json::from_value(serde_json::json!({"phase":"desktop_library_query","query":{"type":"tracks"},"pinnedAlbumIds":[]})).unwrap()).await.unwrap();
            let Registered::LibraryQuery {
                result: crate::controller::protocol::QueryResult::Tracks { page },
            } = tracks
            else {
                panic!()
            };
            let request = Registration::DesktopArtwork {
                reference: page.items[0].artwork.clone().unwrap(),
            };
            assert!(state
                .registration_dependencies(&request, || panic!("artwork touched protected factory"))
                .unwrap()
                .is_none());
            assert!(
                matches!(desktop_registration(&library,request).await.unwrap(),Registered::Artwork{mime,..} if mime=="image/png")
            );
            assert!(Arc::ptr_eq(
                &library,
                &state
                    .library_with(|| panic!("library initialized twice"))
                    .unwrap()
            ));
            state.stop();
            assert!(state
                .library_with(|| panic!("stopped library restarted"))
                .is_err());
        }
        #[test]
        fn desktop_library_registration_is_closed_and_null_query_means_revision_only() {
            assert!(matches!(serde_json::from_value::<Registration>(serde_json::json!({"phase":"desktop_library_query","query":null,"pinnedAlbumIds":[]})).unwrap(),Registration::DesktopLibraryQuery{query:None,..}));
            for value in [
                serde_json::json!({"phase":"desktop_library_query","query":{"type":"invoke","command":"scan"},"pinnedAlbumIds":[]}),
                serde_json::json!({"phase":"desktop_artwork","reference":{"resourceId":"x","revision":0},"path":"E:/audio"}),
            ] {
                assert!(serde_json::from_value::<Registration>(value).is_err());
            }
        }
        #[test]
        fn exit_does_not_wait_for_host_initialization_lock() {
            let state = Arc::new(NativeHostState::default());
            let held = state.initialization.lock().unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            let worker = state.clone();
            let thread = std::thread::spawn(move || {
                worker.stop();
                tx.send(()).unwrap();
            });
            let returned = rx
                .recv_timeout(std::time::Duration::from_millis(100))
                .is_ok();
            drop(held);
            thread.join().unwrap();
            assert!(returned, "Exit waited for host initialization");
        }
        #[test]
        fn initialization_finishing_after_exit_cannot_resurrect_host() {
            let state = Arc::new(NativeHostState::default());
            let store = Arc::new(MemoryStore::default());
            let deps = Arc::new(
                HostDependencies::new(load_or_create_identity(store.as_ref()).unwrap(), store)
                    .unwrap(),
            );
            let worker = state.clone();
            let worker_deps = deps.clone();
            let (entered_tx, entered_rx) = std::sync::mpsc::channel();
            let (finish_tx, finish_rx) = std::sync::mpsc::channel();
            let thread = std::thread::spawn(move || {
                worker.dependencies_with(|| {
                    entered_tx.send(()).unwrap();
                    finish_rx
                        .recv_timeout(std::time::Duration::from_secs(1))
                        .unwrap();
                    Ok(worker_deps)
                })
            });
            entered_rx
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap();
            state.stop();
            finish_tx.send(()).unwrap();
            assert!(thread.join().unwrap().is_err());
            assert!(state
                .dependencies_with(|| panic!("stopped initialization restarted"))
                .is_err());
            assert!(deps
                .commands
                .register_coordinator(AuthoritativeWindow::new(
                    "main".into(),
                    Arc::new(|_| Ok(()))
                ))
                .is_err());
            assert!(deps.commands.capture_snapshot().is_err());
        }
        #[test]
        fn inspection_is_side_effect_free_and_contains_only_device_metadata() {
            let state = NativeHostState::default();
            let off = serde_json::to_value(state.inspect(None).unwrap()).unwrap();
            assert_eq!(
                off,
                serde_json::json!({"enabled":false,"ready":false,"endpoint":null,"pending":null,"paired":null})
            );
            assert!(state.dependencies.get().is_none());
            let store = Arc::new(MemoryStore::default());
            let deps = Arc::new(
                HostDependencies::new(load_or_create_identity(store.as_ref()).unwrap(), store)
                    .unwrap(),
            );
            let invitation = create_invitation(
                &deps.identity,
                "192.168.1.2:9010".parse().unwrap(),
                SystemTime::now(),
            )
            .unwrap();
            deps.pairing.register_invitation(&invitation).unwrap();
            let pending = deps
                .pairing
                .request_pairing(invitation, "Phone".into())
                .unwrap();
            let credential = deps
                .pairing
                .approve_pairing(
                    pending.id,
                    Grants {
                        control: true,
                        administration: false,
                    },
                )
                .unwrap();
            assert!(state.dependencies.set(deps).is_ok());
            let result = serde_json::to_value(state.inspect(None).unwrap()).unwrap();
            assert_eq!(
                result["paired"],
                serde_json::json!([{"id":credential.device_id(),"name":"Phone","grants":{"control":true,"administration":false}}])
            );
            assert_eq!(result["pending"], serde_json::json!([]));
            assert_eq!(result["enabled"], false);
            assert_eq!(result.as_object().unwrap().len(), 5);
        }
        #[test]
        fn publication_registration_is_closed_and_decodes_the_projection() {
            let lease = CoordinatorLease {
                lease_id: Uuid::new_v4(),
                host_epoch: Uuid::new_v4(),
            };
            let snapshot =
                crate::controller::events::tests::projection(&lease.host_epoch.to_string());
            let mut request = serde_json::json!({"phase":"publish","lease":lease,"update":{"type":"projection","snapshot":snapshot}});
            assert!(serde_json::from_value::<Registration>(request.clone()).is_ok());
            request["update"]["snapshot"]["playback"]["track"] =
                serde_json::json!({"path":"C:/secret.mp3"});
            assert!(serde_json::from_value::<Registration>(request).is_err());
        }
        #[test]
        fn registration_and_inspection_reject_caller_authority_and_extra_fields() {
            assert!(
                serde_json::from_str::<Registration>(r#"{"phase":"prepare","window":"main"}"#)
                    .is_err()
            );
            assert!(
                serde_json::from_str::<HostToggle>(r#"{"action":"status","enable":true}"#).is_err()
            );
        }
    }
}
#[cfg(desktop)]
pub use desktop::*;

/// Closed seam only. Task 9 owns the native paired-origin session facade.
#[derive(Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ControllerRequest {
    Command {
        envelope: CommandEnvelope,
    },
    CommandStatus {
        request_id: String,
    },
    Handshake {},
    Query {
        query: crate::controller::protocol::ApplicationQuery,
    },
    Poll {
        cursor: crate::controller::protocol::EventCursor,
    },
    Media {
        reference: crate::controller::protocol::ArtworkReference,
    },
}
#[cfg(target_os = "android")]
fn controller_session(
    app: &tauri::AppHandle,
) -> std::sync::Arc<crate::controller::native_session::NativeSession> {
    use tauri::Manager;
    app.state::<std::sync::Arc<crate::controller::native_session::NativeSession>>()
        .inner()
        .clone()
}
#[cfg(target_os = "android")]
fn controller_store(
    app: &tauri::AppHandle,
) -> std::sync::Arc<dyn crate::controller::native_session::ControllerStore> {
    std::sync::Arc::new(crate::controller::mobile::AndroidControllerStore(
        app.clone(),
    ))
}
#[cfg(target_os = "android")]
#[tauri::command]
pub(crate) async fn controller_request(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    fence: crate::controller::native_session::Fence,
    request: ControllerRequest,
) -> Result<crate::controller::native_session::ControllerReply, ControlError> {
    authorize(&window, &app)?;
    controller_session(&app).request(&fence, request).await
}
#[cfg(target_os = "android")]
#[tauri::command]
pub(crate) async fn controller_connection(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    request: crate::controller::native_session::ConnectionRequest,
) -> Result<crate::controller::native_session::ConnectionReply, ControlError> {
    use crate::controller::native_session::{ConnectionReply, ConnectionRequest};
    authorize(&window, &app)?;
    let session = controller_session(&app);
    match request {
        ConnectionRequest::BeginScope {} => Ok(ConnectionReply::Scope {
            scope_id: session.begin_scope()?,
        }),
        ConnectionRequest::Connect { host_id, fence } => Ok(ConnectionReply::Connected {
            snapshot: session
                .connect(controller_store(&app), host_id, fence)
                .await?,
        }),
    }
}
#[cfg(target_os = "android")]
#[tauri::command]
pub(crate) fn controller_suspend(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    fence: crate::controller::native_session::Fence,
) -> Result<(), ControlError> {
    authorize(&window, &app)?;
    controller_session(&app).suspend(&fence)
}
#[cfg(target_os = "android")]
#[tauri::command]
pub(crate) async fn controller_forget(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    host_id: String,
    fence: crate::controller::native_session::Fence,
) -> Result<(), ControlError> {
    authorize(&window, &app)?;
    controller_session(&app)
        .forget(controller_store(&app), host_id, fence)
        .await
}
#[cfg(target_os = "android")]
#[tauri::command]
pub(crate) async fn controller_pair(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    fence: crate::controller::native_session::Fence,
    device_name: String,
) -> Result<serde_json::Value, ControlError> {
    authorize(&window, &app)?;
    let host_id = controller_session(&app)
        .pair(controller_store(&app), fence, device_name)
        .await?;
    Ok(serde_json::json!({"hostId":host_id}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controller_commands_require_local_main_window() {
        assert!(local_main(
            "main",
            &"tauri://localhost".parse().unwrap(),
            None
        ));
        assert!(!local_main(
            "other",
            &"tauri://localhost".parse().unwrap(),
            None
        ));
        assert!(!local_main(
            "main",
            &"https://example.com".parse().unwrap(),
            None
        ));
        assert!(!local_main(
            "main",
            &"https://tauri.localhost:1234".parse().unwrap(),
            None
        ));
    }
    #[test]
    fn desktop_acl_preserves_registered_domain_commands_and_isolates_controller() {
        let app = tauri::test::mock_builder()
            .invoke_handler(|invoke| {
                invoke.resolver.resolve("authorized");
                true
            })
            .build(tauri::generate_context!())
            .unwrap();
        let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let other = tauri::WebviewWindowBuilder::new(&app, "other", Default::default())
            .build()
            .unwrap();
        let invoke =
            |window: &tauri::WebviewWindow<tauri::test::MockRuntime>, name: &str, origin: &str| {
                tauri::test::get_ipc_response(
                    window,
                    tauri::webview::InvokeRequest {
                        cmd: name.into(),
                        callback: tauri::ipc::CallbackFn(0),
                        error: tauri::ipc::CallbackFn(1),
                        url: origin.parse().unwrap(),
                        body: tauri::ipc::InvokeBody::Json(serde_json::json!({})),
                        headers: Default::default(),
                        invoke_key: tauri::test::INVOKE_KEY.into(),
                    },
                )
            };
        // Discover registered commands, but test them against the independently
        // maintained static ACL via real Tauri IPC authorization (no domain I/O).
        let registration = include_str!("../lib.rs")
            .split("tauri::generate_handler![")
            .nth(1)
            .unwrap()
            .split(']')
            .next()
            .unwrap();
        for line in registration.lines() {
            let name = line.trim().trim_end_matches(',');
            if name.starts_with("commands::") || name.starts_with("controller::") {
                let command = name.rsplit("::").next().unwrap();
                assert!(
                    invoke(&main, command, "http://tauri.localhost").is_ok(),
                    "{command}"
                );
                assert!(
                    invoke(&other, command, "http://tauri.localhost").is_err(),
                    "wrong window: {command}"
                );
                assert!(
                    invoke(&main, command, "https://example.com").is_err(),
                    "remote: {command}"
                );
            }
        }
        for command in [
            "controller_pair",
            "controller_scan_pair",
            "controller_request",
            "controller_connection",
            "controller_suspend",
            "controller_forget",
        ] {
            assert!(
                invoke(&main, command, "http://tauri.localhost").is_err(),
                "mobile command: {command}"
            );
        }
    }
    #[test]
    fn controller_request_rejects_arbitrary_urls_commands_and_unknown_fields() {
        for payload in [
            r#"{"type":"invoke","command":"delete_track"}"#,
            r#"{"type":"handshake","url":"https://example.com"}"#,
            r#"{"type":"command_status","requestId":"id","command":"pause"}"#,
        ] {
            assert!(serde_json::from_str::<ControllerRequest>(payload).is_err());
        }
    }
    #[test]
    fn android_acl_grants_only_controller_main_and_denies_desktop_authority() {
        use std::collections::BTreeMap;
        use tauri::utils::{
            acl::{capability::Capability, manifest::Manifest, resolved::Resolved},
            platform::Target,
        };
        let manifests: BTreeMap<String, Manifest> =
            serde_json::from_str(include_str!("../../gen/schemas/acl-manifests.json")).unwrap();
        let capabilities: BTreeMap<String, Capability> = [
            include_str!("../../capabilities/default.json"),
            include_str!("../../capabilities/mobile.json"),
        ]
        .into_iter()
        .map(|json| {
            let capability: Capability = serde_json::from_str(json).unwrap();
            (capability.identifier.clone(), capability)
        })
        .collect();
        let resolved = Resolved::resolve(&manifests, capabilities, Target::Android).unwrap();
        let authority = tauri::ipc::RuntimeAuthority::new(manifests, resolved);
        for command in [
            "get_application_mode",
            "controller_pair",
            "controller_scan_pair",
            "controller_request",
            "controller_connection",
            "controller_suspend",
            "controller_forget",
        ] {
            assert!(
                authority
                    .resolve_access(command, "main", "main", &tauri::ipc::Origin::Local)
                    .is_some(),
                "{command}"
            );
            assert!(authority
                .resolve_access(command, "other", "other", &tauri::ipc::Origin::Local)
                .is_none());
            assert!(authority
                .resolve_access(
                    command,
                    "main",
                    "main",
                    &tauri::ipc::Origin::Remote {
                        url: "https://example.com".parse().unwrap()
                    }
                )
                .is_none());
        }
        for command in [
            "control_host_enable",
            "control_host_invitation",
            "control_host_approve",
            "control_host_revoke",
            "control_host_register",
            "control_host_complete",
            "delete_track",
            "squeeze_start_server",
            "plugin:controller-native|loadCredentials",
        ] {
            assert!(
                authority
                    .resolve_access(command, "main", "main", &tauri::ipc::Origin::Local)
                    .is_none(),
                "{command}"
            );
        }
    }
}
