//! Bounded native invitation validation, shared by scan and paste.
use super::{
    client::{invalid_pairing, validate_host_endpoint, validate_trust},
    protocol::ControlError,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::Deserialize;
use std::{
    net::SocketAddr,
    time::{Duration, SystemTime},
};
use zeroize::{Zeroize, Zeroizing};

pub struct NativeInvitation {
    pub invitation_id: uuid::Uuid,
    pub host_id: String,
    pub endpoint: SocketAddr,
    pub ca_der: Vec<u8>,
    pub fingerprint: String,
    pub secret: Zeroizing<[u8; 32]>,
    pub expires_at: SystemTime,
}
impl std::fmt::Debug for NativeInvitation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeInvitation([REDACTED])")
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Expiration {
    unix_seconds: u64,
    nanoseconds: u32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InvitationPayload {
    version: u8,
    invitation_id: String,
    host_id: String,
    endpoint: String,
    ca: String,
    fingerprint: String,
    secret: String,
    expires_at: Expiration,
}
impl Drop for InvitationPayload {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}

pub fn validate_invitation(text: &str, now: SystemTime) -> Result<NativeInvitation, ControlError> {
    if text.len() > 2048 {
        return Err(invalid_pairing());
    }
    let payload: InvitationPayload = serde_json::from_str(text).map_err(|_| invalid_pairing())?;
    if payload.version != 1
        || payload.expires_at.unix_seconds > 9_007_199_254_740_991
        || payload.expires_at.nanoseconds >= 1_000_000_000
    {
        return Err(invalid_pairing());
    }
    let expires_at = SystemTime::UNIX_EPOCH
        .checked_add(Duration::new(
            payload.expires_at.unix_seconds,
            payload.expires_at.nanoseconds,
        ))
        .ok_or_else(invalid_pairing)?;
    if expires_at <= now
        || expires_at
            > now
                .checked_add(Duration::from_secs(300))
                .ok_or_else(invalid_pairing)?
    {
        return Err(invalid_pairing());
    }
    let endpoint: SocketAddr = payload.endpoint.parse().map_err(|_| invalid_pairing())?;
    validate_host_endpoint(&payload.host_id, endpoint)?;
    let invitation_id =
        uuid::Uuid::parse_str(&payload.invitation_id).map_err(|_| invalid_pairing())?;
    if invitation_id.is_nil() || invitation_id.to_string() != payload.invitation_id {
        return Err(invalid_pairing());
    }
    let ca_der = URL_SAFE_NO_PAD
        .decode(&payload.ca)
        .map_err(|_| invalid_pairing())?;
    let fingerprint = super::client::ca_fingerprint(&ca_der);
    if payload.fingerprint != fingerprint { return Err(invalid_pairing()); }
    // Build (without connecting) to validate certificate encoding and the exact
    // pinned trust policy before any pairing secret can be submitted.
    validate_trust(&payload.host_id, endpoint, &ca_der)?;
    let decoded = Zeroizing::new(
        URL_SAFE_NO_PAD
            .decode(&payload.secret)
            .map_err(|_| invalid_pairing())?,
    );
    let secret = Zeroizing::new(
        decoded
            .as_slice()
            .try_into()
            .map_err(|_| invalid_pairing())?,
    );
    Ok(NativeInvitation {
        invitation_id,
        host_id: payload.host_id.clone(),
        endpoint,
        ca_der,
        fingerprint,
        secret,
        expires_at,
    })
}

#[derive(serde::Serialize)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum PairingStatus {
    Cancelled,
    InvitationReady { host_id: String, fingerprint: String },
}

#[derive(Default)]
pub struct NativePairingState {
    pending: std::sync::Mutex<Option<NativeInvitation>>,
    #[cfg(target_os = "android")]
    scan: std::sync::Mutex<()>,
}
impl NativePairingState {
    pub fn accept_input(
        &self,
        input: Option<&str>,
        now: SystemTime,
    ) -> Result<PairingStatus, ControlError> {
        let mut pending = self.pending.lock().map_err(|_| invalid_pairing())?;
        *pending = None;
        let Some(input) = input else {
            return Ok(PairingStatus::Cancelled);
        };
        let invitation = validate_invitation(input, now)?;
        let status = PairingStatus::InvitationReady {
            host_id: invitation.host_id.clone(),
            fingerprint: invitation.fingerprint.clone(),
        };
        *pending = Some(invitation);
        Ok(status)
    }
    pub fn take_invitation(
        &self,
        now: SystemTime,
    ) -> Result<Option<NativeInvitation>, ControlError> {
        let invitation = self.pending.lock().map_err(|_| invalid_pairing())?.take();
        if invitation
            .as_ref()
            .is_some_and(|invitation| invitation.expires_at <= now)
        {
            return Err(invalid_pairing());
        }
        Ok(invitation)
    }
}

#[cfg(any(target_os = "android", test))]
fn pairing_window_allowed(label: &str, url: &url::Url, dev_url: Option<&url::Url>) -> bool {
    label == "main"
        && ((url.scheme() == "tauri"
            && url.host_str() == Some("localhost")
            && url.port().is_none())
            || (matches!(url.scheme(), "http" | "https")
                && url.host_str() == Some("tauri.localhost")
                && url.port().is_none())
            || dev_url.is_some_and(|dev| url.origin() == dev.origin()))
}

/// The only frontend pairing entry point. Native invitation/credentials never
/// leave this module; scan and pasted text converge on the same bounded parser.
#[cfg(target_os = "android")]
#[tauri::command]
pub(crate) async fn controller_scan_pair(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    invitation: Option<String>,
    fence: super::native_session::Fence,
) -> Result<PairingStatus, ControlError> {
    use tauri::Manager;
    let url = window.url().map_err(|_| invalid_pairing())?;
    #[cfg(debug_assertions)]
    let dev_url = app.config().build.dev_url.as_ref();
    #[cfg(not(debug_assertions))]
    let dev_url = None;
    if !pairing_window_allowed(window.label(), &url, dev_url) {
        return Err(ControlError {
            code: super::protocol::ControlErrorCode::Unauthorized,
            message: "Pairing is only available in the local controller window.".into(),
            retryable: false,
        });
    }
    let session = app
        .state::<std::sync::Arc<super::native_session::NativeSession>>()
        .inner()
        .clone();
    session.reserve_pairing(&fence)?;
    tauri::async_runtime::spawn_blocking(move || {
        // The session owns logical admission; this guard also prevents a cancelled
        // native capture Activity still returning from overlapping a second scan.
        let capture = app.state::<NativePairingState>();
        let _capture = match capture.scan.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                let _ = session.stage_pairing(&fence, None);
                return Err(super::client::transport_error(
                    super::protocol::ControlErrorCode::Busy,
                ));
            }
        };
        let input = match invitation {
            Some(text) => Ok(Some(Zeroizing::new(text))),
            None => app.state::<NativeBridge<tauri::Wry>>().scan_invitation(),
        };
        match input {
            Ok(input) => session.stage_pairing(&fence, input),
            Err(error) => {
                let _ = session.stage_pairing(&fence, None);
                Err(error)
            }
        }
    })
    .await
    .map_err(|_| invalid_pairing())?
}

pub fn native_plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("controller-native")
        // Returning false would allow Tauri's mobile fallback dispatcher.
        // Always resolve the invocation as rejected, even if ACL is misgranted.
        .invoke_handler(|invoke| {
            invoke
                .resolver
                .reject("Native controller methods are not available to the WebView.");
            true
        })
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            {
                use tauri::Manager;
                let handle =
                    api.register_android_plugin("com.audion.app", "ControllerNativePlugin")?;
                app.manage(NativeBridge(handle));
                app.manage(NativePairingState::default());
            }
            #[cfg(not(target_os = "android"))]
            let _ = (app, api);
            Ok(())
        })
        .build()
}

/// Not a command or serializable DTO. Only native Rust owns this handle.
#[cfg(target_os = "android")]
pub struct NativeBridge<R: tauri::Runtime>(tauri::plugin::PluginHandle<R>);

#[cfg(target_os = "android")]
impl<R: tauri::Runtime> NativeBridge<R> {
    fn unavailable() -> ControlError {
        ControlError {
            code: super::protocol::ControlErrorCode::HostNotReady,
            message: "Native secure storage or scanning is unavailable. Pair again.".into(),
            retryable: false,
        }
    }
    pub fn save_credentials(&self, host_id: &str, bytes: &[u8]) -> Result<(), ControlError> {
        self.0
            .run_mobile_plugin::<serde_json::Value>(
                "saveCredentials",
                serde_json::json!({"hostId":host_id,"bytes":bytes}),
            )
            .map(|_| ())
            .map_err(|_| Self::unavailable())
    }
    pub fn load_credentials(
        &self,
        host_id: &str,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, ControlError> {
        #[derive(Deserialize)]
        struct Response {
            bytes: Option<Vec<u8>>,
        }
        let response: Response = self
            .0
            .run_mobile_plugin("loadCredentials", serde_json::json!({"hostId":host_id}))
            .map_err(|_| Self::unavailable())?;
        Ok(response.bytes.map(Zeroizing::new))
    }
    pub fn delete_credentials(&self, host_id: &str) -> Result<(), ControlError> {
        self.0
            .run_mobile_plugin::<serde_json::Value>(
                "deleteCredentials",
                serde_json::json!({"hostId":host_id}),
            )
            .map(|_| ())
            .map_err(|_| Self::unavailable())
    }
    pub fn scan_invitation(&self) -> Result<Option<Zeroizing<String>>, ControlError> {
        #[derive(Deserialize)]
        struct Response {
            invitation: Option<String>,
        }
        let response: Response = self
            .0
            .run_mobile_plugin("scanInvitation", serde_json::json!({}))
            .map_err(|_| Self::unavailable())?;
        Ok(response.invitation.map(Zeroizing::new))
    }
}

#[cfg(all(test, desktop))]
mod tests {
    use super::*;
    use crate::controller::{
        identity::load_or_create_identity, pairing::create_invitation, secrets::SecretStore,
    };
    use std::{collections::HashMap, sync::Mutex, time::Duration};
    #[test]
    fn direct_frontend_credentials_invocation_is_denied() {
        // Grant only the mock fixture permission so the test reaches our Rust
        // rejection handler, rather than passing solely due to default ACL.
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        let methods = [
            "saveCredentials",
            "loadCredentials",
            "deleteCredentials",
            "scanInvitation",
        ];
        for method in methods {
            context.runtime_authority_mut().__allow_command(
                format!("plugin:controller-native|{method}"),
                tauri::utils::acl::ExecutionContext::Local,
            );
        }
        let app = tauri::test::mock_builder()
            .plugin(native_plugin())
            .build(context)
            .unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        for method in methods {
            let response = tauri::test::get_ipc_response(
                &webview,
                tauri::webview::InvokeRequest {
                    cmd: format!("plugin:controller-native|{method}"),
                    callback: tauri::ipc::CallbackFn(0),
                    error: tauri::ipc::CallbackFn(1),
                    url: "http://tauri.localhost".parse().unwrap(),
                    body: tauri::ipc::InvokeBody::Json(
                        serde_json::json!({"hostId":"cf8dd70c-8cc2-4640-bd40-5b06f68cc301"}),
                    ),
                    headers: Default::default(),
                    invoke_key: tauri::test::INVOKE_KEY.into(),
                },
            );
            assert_eq!(
                response.err(),
                Some(serde_json::json!(
                    "Native controller methods are not available to the WebView."
                ))
            );
        }
    }
    #[test]
    fn pairing_command_is_scoped_to_local_main_window() {
        for local in [
            "tauri://localhost/",
            "http://tauri.localhost/",
            "https://tauri.localhost/",
        ] {
            assert!(pairing_window_allowed(
                "main",
                &local.parse().unwrap(),
                None
            ));
            assert!(!pairing_window_allowed(
                "other",
                &local.parse().unwrap(),
                None
            ));
        }
        assert!(!pairing_window_allowed(
            "main",
            &"https://example.com/".parse().unwrap(),
            None
        ));
        assert!(!pairing_window_allowed(
            "main",
            &"http://tauri.localhost:9999/".parse().unwrap(),
            None
        ));
        let dev: url::Url = "http://localhost:1420/".parse().unwrap();
        assert!(pairing_window_allowed("main", &dev, Some(&dev)));
        assert!(!pairing_window_allowed(
            "main",
            &"http://localhost:1421/".parse().unwrap(),
            Some(&dev)
        ));
    }
    #[derive(Default)]
    struct Memory(Mutex<HashMap<String, Vec<u8>>>);
    impl SecretStore for Memory {
        fn read(&self, key: &str) -> Result<Option<Vec<u8>>, ControlError> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn write(&self, key: &str, bytes: &[u8]) -> Result<(), ControlError> {
            self.0.lock().unwrap().insert(key.into(), bytes.into());
            Ok(())
        }
        fn delete(&self, key: &str) -> Result<(), ControlError> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
    }
    fn invitation() -> (String, SystemTime) {
        let now = SystemTime::UNIX_EPOCH + Duration::new(1_800_000_000, 123_456_789);
        let identity = load_or_create_identity(&Memory::default()).unwrap();
        let invitation =
            create_invitation(&identity, "192.168.1.8:9010".parse().unwrap(), now).unwrap();
        (invitation.encode().unwrap().to_string(), now)
    }
    #[test]
    fn invitation_fingerprint_is_canonical_and_required_before_staging() {
        use sha2::{Digest, Sha256};
        let (text, now) = invitation();
        let mut wire: serde_json::Value = serde_json::from_str(&text).unwrap();
        let ca = URL_SAFE_NO_PAD.decode(wire["ca"].as_str().unwrap()).unwrap();
        let expected = format!("{:x}", Sha256::digest(&ca));
        assert_eq!(wire["fingerprint"], expected);
        let status = NativePairingState::default().accept_input(Some(&text), now).unwrap();
        assert_eq!(serde_json::to_value(status).unwrap()["fingerprint"], expected);
        for invalid in [None, Some(serde_json::json!("0".repeat(64))), Some(serde_json::json!(expected.to_uppercase()))] {
            if let Some(value) = invalid { wire["fingerprint"] = value; }
            else { wire.as_object_mut().unwrap().remove("fingerprint"); }
            assert!(validate_invitation(&wire.to_string(), now).is_err());
        }
    }
    #[test]
    fn scan_and_paste_stage_native_invitation_but_return_only_status() {
        let (text, now) = invitation();
        let state = NativePairingState::default();
        let status = state.accept_input(Some(&text), now).unwrap();
        let pending = state
            .take_invitation(now)
            .unwrap()
            .expect("native invitation must be staged");
        assert_eq!(
            serde_json::to_value(status).unwrap(),
            serde_json::json!({"status":"invitation_ready","hostId":pending.host_id,"fingerprint":pending.fingerprint})
        );
        assert!(state.take_invitation(now).unwrap().is_none());
        state.accept_input(Some(&text), now).unwrap();
        assert!(state
            .take_invitation(now + Duration::from_secs(300))
            .is_err());
        assert!(state.take_invitation(now).unwrap().is_none());
    }
    #[test]
    fn cancelled_or_invalid_input_clears_previous_native_invitation() {
        let (text, now) = invitation();
        let state = NativePairingState::default();
        state.accept_input(Some(&text), now).unwrap();
        assert!(state.accept_input(Some("invalid"), now).is_err());
        assert!(state.take_invitation(now).unwrap().is_none());
        state.accept_input(Some(&text), now).unwrap();
        assert_eq!(
            serde_json::to_value(state.accept_input(None, now).unwrap()).unwrap(),
            serde_json::json!({"status":"cancelled"})
        );
        assert!(state.take_invitation(now).unwrap().is_none());
    }
    #[test]
    fn native_invitation_preserves_exact_expiry_and_rejects_noncanonical_values() {
        let (text, now) = invitation();
        let result =
            validate_invitation(&text, now).expect("host-emitted invitation must validate");
        assert_eq!(result.expires_at, now + Duration::from_secs(300));
        assert!(validate_invitation(&text, result.expires_at).is_err());
        let original: serde_json::Value = serde_json::from_str(&text).unwrap();
        for invalid in [
            serde_json::json!({"unixSeconds":1800000300,"nanoseconds":1000000000}),
            serde_json::json!({"unixSeconds":9007199254740992_u64,"nanoseconds":0}),
            serde_json::json!({"unixSeconds":1800000300.1,"nanoseconds":0}),
            serde_json::json!({"unixSeconds":-1,"nanoseconds":0}),
            serde_json::json!(1800000300),
        ] {
            let mut value = original.clone();
            value["expiresAt"] = invalid;
            assert!(validate_invitation(&value.to_string(), now).is_err());
        }
    }
    #[test]
    fn native_invitation_rejects_non_lan_endpoints_and_unbounded_or_invalid_material() {
        let (text, now) = invitation();
        assert!(validate_invitation(&text, now).is_ok());
        let original: serde_json::Value = serde_json::from_str(&text).unwrap();
        for endpoint in [
            "127.0.0.1:9010",
            "8.8.8.8:9010",
            "169.254.1.2:9010",
            "[fd00::1]:9010",
            "192.168.1.8:0",
            "host.local:9010",
            "https://192.168.1.8:9010",
        ] {
            let mut value = original.clone();
            value["endpoint"] = endpoint.into();
            assert!(
                validate_invitation(&value.to_string(), now).is_err(),
                "{endpoint}"
            );
        }
        for (field, value) in [
            ("version", serde_json::json!(2)),
            ("ca", serde_json::json!("broken")),
            ("secret", serde_json::json!("AA")),
            ("hostId", serde_json::json!("../../escape")),
            ("unknown", serde_json::json!(true)),
        ] {
            let mut bad = original.clone();
            bad[field] = value;
            assert!(
                validate_invitation(&bad.to_string(), now).is_err(),
                "{field}"
            );
        }
        assert!(validate_invitation(&"x".repeat(2049), now).is_err());
    }
}

/// Lazy native-only credential store: constructing it never opens the Keystore.
#[cfg(target_os = "android")]
pub(crate) struct AndroidControllerStore(pub tauri::AppHandle);
#[cfg(target_os = "android")]
impl super::native_session::ControllerStore for AndroidControllerStore {
    fn load(&self, host: &str) -> Result<Option<Zeroizing<Vec<u8>>>, ControlError> {
        use tauri::Manager;
        self.0
            .state::<NativeBridge<tauri::Wry>>()
            .load_credentials(host)
    }
    fn save(&self, host: &str, bytes: &[u8]) -> Result<(), ControlError> {
        use tauri::Manager;
        self.0
            .state::<NativeBridge<tauri::Wry>>()
            .save_credentials(host, bytes)
    }
    fn delete(&self, host: &str) -> Result<(), ControlError> {
        use tauri::Manager;
        self.0
            .state::<NativeBridge<tauri::Wry>>()
            .delete_credentials(host)
    }
}
