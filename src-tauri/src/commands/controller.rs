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
    #[serde(rename_all = "camelCase")]
    pub struct Registered {
        host_id: String,
        lease: CoordinatorLease,
        #[serde(skip_serializing_if = "Option::is_none")]
        revision: Option<u64>,
    }
    #[tauri::command]
    pub fn control_host_register(
        app: tauri::AppHandle,
        window: tauri::WebviewWindow,
        request: Registration,
    ) -> Result<Registered, ControlError> {
        authorize(&window, &app)?;
        let state = app.state::<NativeHostState>();
        let deps = state.dependencies()?;
        let mut revision = None;
        let lease = match request {
            Registration::Prepare {} => {
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
                lease
            }
            Registration::Release { lease } => {
                deps.commands.release(window.label(), &lease)?;
                lease
            }
        };
        Ok(Registered {
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
    Command { envelope: CommandEnvelope },
    CommandStatus { request_id: String },
    Handshake {},
}
#[cfg(mobile)]
#[tauri::command]
pub fn controller_request(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    request: ControllerRequest,
) -> Result<serde_json::Value, ControlError> {
    authorize(&window, &app)?;
    let _ = request;
    Err(rejected(ControlErrorCode::Unsupported))
}
#[cfg(mobile)]
macro_rules! unavailable_command {
    ($name:ident) => {
        #[tauri::command]
        pub fn $name(
            app: tauri::AppHandle,
            window: tauri::WebviewWindow,
        ) -> Result<(), ControlError> {
            authorize(&window, &app)?;
            Err(rejected(ControlErrorCode::Unsupported))
        }
    };
}
#[cfg(mobile)]
unavailable_command!(controller_pair);
#[cfg(mobile)]
unavailable_command!(controller_connection);
#[cfg(mobile)]
unavailable_command!(controller_suspend);
#[cfg(mobile)]
unavailable_command!(controller_forget);

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
