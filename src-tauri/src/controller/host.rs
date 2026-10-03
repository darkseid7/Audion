//! Separate native TLS listener. No media, generic invoke or arbitrary URL routes.
use super::{
    commands::{error, AuthenticatedDevice, CommandService},
    identity::HostIdentity,
    pairing::{Grants, NativeCredential, PairingService},
    protocol::{CommandEnvelope, ControlError, ControlErrorCode},
    secrets::SecretStore,
};
use axum::{
    body::{to_bytes, Body},
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr, TcpListener},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};
use subtle::ConstantTimeEq;
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LanConfig {
    pub address: std::net::Ipv4Addr,
    pub port: u16,
}
impl LanConfig {
    pub fn validate(&self) -> Result<SocketAddr, ControlError> {
        if !self.address.is_private() || self.port == 0 || self.port == 9000 {
            return Err(error(ControlErrorCode::InvalidRequest));
        }
        Ok(SocketAddr::new(IpAddr::V4(self.address), self.port))
    }
}
struct Delivery {
    source: IpAddr,
    authorization: [u8; 32],
    expires_at: SystemTime,
    credential: Option<NativeCredential>,
    issued: bool,
}
#[derive(Default)]
struct Admission {
    attempts: HashMap<IpAddr, (Instant, u8)>,
    deliveries: HashMap<Uuid, Delivery>,
}
pub struct HostDependencies {
    pub identity: Arc<HostIdentity>,
    pub pairing: Arc<PairingService>,
    pub commands: Arc<CommandService>,
    admission: Mutex<Admission>,
    requests: tokio::sync::Semaphore,
}
impl HostDependencies {
    pub fn invalidate_pairing(&self) -> Result<(), ControlError> {
        self.admission
            .lock()
            .map_err(|_| error(ControlErrorCode::HostNotReady))?
            .deliveries
            .clear();
        self.pairing.invalidate_invitations()
    }
    pub fn revoke(&self, id: Uuid) -> Result<(), ControlError> {
        let result = self.commands.revoke(id);
        self.admission
            .lock()
            .map_err(|_| error(ControlErrorCode::HostNotReady))?
            .deliveries
            .retain(|_, d| {
                !d.credential
                    .as_ref()
                    .is_some_and(|credential| credential.device_id() == id)
            });
        result
    }
    fn prune_pairings(&self) -> Result<(), ControlError> {
        self.admission
            .lock()
            .map_err(|_| error(ControlErrorCode::HostNotReady))?
            .deliveries
            .retain(|_, d| d.expires_at > SystemTime::now() && !d.issued);
        Ok(())
    }
    pub fn new(identity: HostIdentity, store: Arc<dyn SecretStore>) -> Result<Self, ControlError> {
        let pairing = Arc::new(PairingService::new(&identity, store)?);
        Ok(Self {
            identity: Arc::new(identity),
            commands: Arc::new(CommandService::new(pairing.clone())),
            pairing,
            admission: Mutex::new(Admission::default()),
            requests: tokio::sync::Semaphore::new(64),
        })
    }
    pub fn approve(&self, id: Uuid, grants: Grants) -> Result<(), ControlError> {
        let mut admission = self
            .admission
            .lock()
            .map_err(|_| error(ControlErrorCode::HostNotReady))?;
        let delivery = admission
            .deliveries
            .get_mut(&id)
            .filter(|d| d.expires_at > SystemTime::now() && !d.issued && d.credential.is_none())
            .ok_or_else(|| error(ControlErrorCode::Unauthorized))?;
        delivery.credential = Some(self.pairing.approve_pairing(id, grants)?);
        Ok(())
    }
}
fn failure(failure: ControlError) -> Response {
    let status = match failure.code {
        ControlErrorCode::Unauthorized => StatusCode::UNAUTHORIZED,
        ControlErrorCode::PermissionRequired => StatusCode::FORBIDDEN,
        ControlErrorCode::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        ControlErrorCode::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        ControlErrorCode::Busy | ControlErrorCode::HostNotReady => StatusCode::SERVICE_UNAVAILABLE,
        ControlErrorCode::Unsupported => StatusCode::NOT_IMPLEMENTED,
        ControlErrorCode::NotFound => StatusCode::NOT_FOUND,
        ControlErrorCode::OutcomeUnknown => StatusCode::GATEWAY_TIMEOUT,
        _ => StatusCode::BAD_REQUEST,
    };
    (status, axum::Json(failure)).into_response()
}
fn router(deps: Arc<HostDependencies>) -> Router {
    Router::new().fallback(route).with_state(deps)
}
fn private_peer(peer: SocketAddr) -> bool {
    matches!(peer.ip(),IpAddr::V4(ip) if ip.is_private())
}
async fn route(
    State(deps): State<Arc<HostDependencies>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
) -> Response {
    if !private_peer(peer) {
        return failure(error(ControlErrorCode::PermissionRequired));
    }
    let Ok(_permit) = deps.requests.try_acquire() else {
        return failure(error(ControlErrorCode::Busy));
    };
    let mut response = match tokio::time::timeout(
        Duration::from_secs(30),
        route_inner(deps.clone(), peer, request),
    )
    .await
    {
        Ok(Ok(response)) => response,
        Ok(Err(err)) => failure(err),
        Err(_) => failure(error(ControlErrorCode::OutcomeUnknown)),
    };
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PairRequest {
    invitation: String,
    device_name: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PairStatus {
    pending_id: Uuid,
    invitation: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StatusRequest {
    request_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyRequest {}
fn decode<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, ControlError> {
    serde_json::from_slice(body).map_err(|_| error(ControlErrorCode::InvalidRequest))
}
async fn route_inner(
    deps: Arc<HostDependencies>,
    peer: SocketAddr,
    request: Request,
) -> Result<Response, ControlError> {
    let path = request.uri().path().to_owned();
    if request.method() != axum::http::Method::POST
        || request.uri().query().is_some()
        || !path.starts_with("/control/v1/")
    {
        return Err(error(ControlErrorCode::InvalidRequest));
    }
    // Header authentication precedes body buffering for every protected route.
    let device = if path == "/control/v1/pairing" || path == "/control/v1/pairing/status" {
        None
    } else {
        let authorization = request
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .filter(|v| v.len() <= 128)
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(|| error(ControlErrorCode::Unauthorized))?;
        let (id, secret) = authorization
            .split_once(':')
            .ok_or_else(|| error(ControlErrorCode::Unauthorized))?;
        let id = Uuid::parse_str(id).map_err(|_| error(ControlErrorCode::Unauthorized))?;
        let secret = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(secret)
                .map_err(|_| error(ControlErrorCode::Unauthorized))?,
        );
        Some(AuthenticatedDevice::authenticate(
            &deps.pairing,
            id,
            &secret,
        )?)
    };
    let limit = if device.is_none() { 4096 } else { 1024 * 1024 };
    let body = Zeroizing::new(
        to_bytes(request.into_body(), limit)
            .await
            .map_err(|_| error(ControlErrorCode::TooLarge))?
            .to_vec(),
    );
    match path.as_str() {
        "/control/v1/pairing" => {
            let input: PairRequest = decode(&body)?;
            let invitation = Zeroizing::new(input.invitation);
            let mut admission = deps
                .admission
                .lock()
                .map_err(|_| error(ControlErrorCode::HostNotReady))?;
            let now = Instant::now();
            admission
                .attempts
                .retain(|_, (at, _)| now.duration_since(*at) < Duration::from_secs(60));
            if !admission.attempts.contains_key(&peer.ip()) && admission.attempts.len() >= 256 {
                return Err(error(ControlErrorCode::Busy));
            }
            let (_, attempts) = admission.attempts.entry(peer.ip()).or_insert((now, 0));
            if *attempts >= 10 {
                return Err(error(ControlErrorCode::RateLimited));
            }
            *attempts += 1;
            admission
                .deliveries
                .retain(|_, d| d.expires_at > SystemTime::now() && !d.issued);
            // Before approval there is no authenticated device identity. This
            // conservative direct-source quota is not an identity assertion;
            // devices sharing a NAT address must pair sequentially.
            if admission.deliveries.len() >= 4
                || admission.deliveries.values().any(|d| d.source == peer.ip())
            {
                return Err(error(ControlErrorCode::Busy));
            }
            let pending = deps.pairing.request_wire(&invitation, input.device_name)?;
            admission.deliveries.insert(
                pending.id,
                Delivery {
                    source: peer.ip(),
                    authorization: Sha256::digest(invitation.as_bytes()).into(),
                    expires_at: pending.expires_at,
                    credential: None,
                    issued: false,
                },
            );
            Ok(
                axum::Json(serde_json::json!({"status":"pending","pendingId":pending.id}))
                    .into_response(),
            )
        }
        "/control/v1/pairing/status" => {
            let input: PairStatus = decode(&body)?;
            let invitation = Zeroizing::new(input.invitation);
            if invitation.len() > 2048 {
                return Err(error(ControlErrorCode::TooLarge));
            }
            let authorization: [u8; 32] = Sha256::digest(invitation.as_bytes()).into();
            let mut admission = deps
                .admission
                .lock()
                .map_err(|_| error(ControlErrorCode::HostNotReady))?;
            let delivery = admission
                .deliveries
                .get_mut(&input.pending_id)
                .filter(|d| {
                    d.expires_at > SystemTime::now()
                        && bool::from(d.authorization.ct_eq(&authorization))
                        && !d.issued
                })
                .ok_or_else(|| error(ControlErrorCode::Unauthorized))?;
            match delivery.credential.take() {
                None => Ok(axum::Json(serde_json::json!({"status":"pending"})).into_response()),
                Some(credential) => {
                    // Exactly one native TLS response. Losing it requires fresh pairing.
                    delivery.issued = true;
                    deps.pairing
                        .authenticate(credential.device_id(), credential.secret())?;
                    let secret = Zeroizing::new(URL_SAFE_NO_PAD.encode(credential.secret()));
                    let response = format!(
                        "{{\"status\":\"approved\",\"deviceId\":\"{}\",\"secret\":\"{}\"}}",
                        credential.device_id(),
                        secret.as_str()
                    );
                    Ok(([("content-type", "application/json")], response).into_response())
                }
            }
        }
        "/control/v1/commands" => {
            let envelope: CommandEnvelope = decode(&body)?;
            Ok(axum::Json(
                deps.commands
                    .submit(device.as_ref().unwrap(), envelope)
                    .await?,
            )
            .into_response())
        }
        "/control/v1/commands/status" => {
            let input: StatusRequest = decode(&body)?;
            if input.request_id.len() > 128 {
                return Err(error(ControlErrorCode::InvalidRequest));
            }
            match deps
                .commands
                .status(device.as_ref().unwrap(), &input.request_id)?
            {
                None => Ok(axum::Json(serde_json::json!({"status":"pending"})).into_response()),
                Some(result) => Ok(axum::Json(result?).into_response()),
            }
        }
        "/control/v1/handshake" => {
            let _: EmptyRequest = decode(&body)?;
            Ok(axum::Json(serde_json::json!({"protocolVersion":1,"hostId":deps.identity.id(),"hostEpoch":deps.commands.host_epoch()?,"capabilities":{"queries":[],"intents":[]}})).into_response())
        }
        // Tasks 7/8 own publication/projections. Never fabricate state or invoke
        // arbitrary native methods, including administrative operations.
        "/control/v1/admin" => Err(error(ControlErrorCode::PermissionRequired)),
        "/control/v1/queries" | "/control/v1/events" | "/control/v1/resources" => {
            Err(error(ControlErrorCode::Unsupported))
        }
        _ => Err(error(ControlErrorCode::NotFound)),
    }
}

pub struct HostHandle {
    handle: axum_server::Handle<SocketAddr>,
    task: tokio::task::JoinHandle<()>,
    pub endpoint: SocketAddr,
}
impl HostHandle {
    pub fn running(&self) -> bool {
        !self.task.is_finished()
    }
}
impl Drop for HostHandle {
    fn drop(&mut self) {
        self.handle.shutdown();
        self.task.abort();
    }
}
pub async fn start_host(
    config: LanConfig,
    deps: Arc<HostDependencies>,
) -> Result<HostHandle, ControlError> {
    let endpoint = config.validate()?;
    deps.commands.host_epoch()?;
    let listener = bind_listener(endpoint)?;
    start_prebound(listener, deps).await
}
fn bind_listener(endpoint: SocketAddr) -> Result<TcpListener, ControlError> {
    TcpListener::bind(endpoint).map_err(|_| error(ControlErrorCode::OutputUnavailable))
}
async fn start_prebound(
    listener: TcpListener,
    deps: Arc<HostDependencies>,
) -> Result<HostHandle, ControlError> {
    let endpoint = listener
        .local_addr()
        .map_err(|_| error(ControlErrorCode::HostNotReady))?;
    listener
        .set_nonblocking(true)
        .map_err(|_| error(ControlErrorCode::HostNotReady))?;
    let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|_| error(ControlErrorCode::HostNotReady))?
    .with_no_client_auth()
    .with_single_cert(
        vec![rustls::pki_types::CertificateDer::from(
            deps.identity.leaf_der().to_vec(),
        )],
        deps.identity.leaf_private_key()?,
    )
    .map_err(|_| error(ControlErrorCode::HostNotReady))?;
    let config = axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(tls));
    let mut server = axum_server::from_tcp_rustls(listener, config)
        .map_err(|_| error(ControlErrorCode::HostNotReady))?
        .map(|tls| tls.acceptor(BoundedAcceptor(Arc::new(tokio::sync::Semaphore::new(64)))))
        .http1_only();
    server
        .http_builder()
        .http1()
        .timer(hyper_util::rt::TokioTimer::new())
        .header_read_timeout(Duration::from_secs(10))
        .max_buf_size(16 * 1024)
        .keep_alive(false);
    let handle = axum_server::Handle::new();
    let server = server.handle(handle.clone());
    let shutdown = handle.clone();
    let task = tokio::spawn(async move {
        let serving =
            server.serve(router(deps.clone()).into_make_service_with_connect_info::<SocketAddr>());
        tokio::pin!(serving);
        let mut expiry = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = &mut serving => break,
                _ = expiry.tick() => {
                    // Own cleanup in the listener task: no detached timer and
                    // no expired native credential retained until new traffic.
                    if deps.prune_pairings().is_err() {
                        shutdown.shutdown();
                        break;
                    }
                }
            }
        }
        deps.commands.invalidate_window("main");
        let _ = deps.invalidate_pairing();
    });
    Ok(HostHandle {
        handle,
        task,
        endpoint,
    })
}

/// The permit lives as long as the TCP/TLS stream, including handshake and idle
/// headers. HTTP route admission alone does not bound unauthenticated sockets.
#[derive(Clone)]
struct BoundedAcceptor(Arc<tokio::sync::Semaphore>);
struct BoundedStream {
    stream: tokio::net::TcpStream,
    _permit: tokio::sync::OwnedSemaphorePermit,
}
impl<S> axum_server::accept::Accept<tokio::net::TcpStream, S> for BoundedAcceptor {
    type Stream = BoundedStream;
    type Service = S;
    type Future = std::future::Ready<std::io::Result<(Self::Stream, S)>>;
    fn accept(&self, stream: tokio::net::TcpStream, service: S) -> Self::Future {
        std::future::ready(
            self.0
                .clone()
                .try_acquire_owned()
                .map(|permit| {
                    (
                        BoundedStream {
                            stream,
                            _permit: permit,
                        },
                        service,
                    )
                })
                .map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::WouldBlock,
                        "Controller connection limit",
                    )
                }),
        )
    }
}
impl tokio::io::AsyncRead for BoundedStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}
impl tokio::io::AsyncWrite for BoundedStream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.stream).poll_write(cx, buf)
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.stream).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::{identity::load_or_create_identity, secrets::tests::MemoryStore};
    use tower::ServiceExt;
    fn fixture() -> Arc<HostDependencies> {
        let store = Arc::new(MemoryStore::default());
        Arc::new(
            HostDependencies::new(load_or_create_identity(store.as_ref()).unwrap(), store).unwrap(),
        )
    }
    #[tokio::test]
    async fn real_tls_host_client_and_port_collision() {
        let deps = fixture();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = listener.local_addr().unwrap();
        assert!(bind_listener(endpoint).is_err());
        let host = start_prebound(listener, deps.clone()).await.unwrap();
        assert!(host.running());
        let client = crate::controller::client::trusted_client(
            deps.identity.id(),
            endpoint,
            deps.identity.ca_der(),
        )
        .unwrap();
        let url = format!(
            "https://{}:{}/control/v1/handshake",
            deps.identity.server_name(),
            endpoint.port()
        );
        let response = client.post(&url).body("{}").send().await.unwrap();
        // Test-only prebound loopback TLS does NOT weaken the direct-peer gate.
        assert_eq!(response.status().as_u16(), 403);
        let other = fixture();
        let wrong = crate::controller::client::trusted_client(
            deps.identity.id(),
            endpoint,
            other.identity.ca_der(),
        )
        .unwrap();
        assert!(wrong.post(url).body("{}").send().await.is_err());
        drop(host);
    }
    #[tokio::test]
    async fn tls_connection_admission_is_bounded_before_handshake() {
        use tokio::io::AsyncReadExt;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = listener.local_addr().unwrap();
        let host = start_prebound(listener, fixture()).await.unwrap();
        let mut sockets = Vec::new();
        for _ in 0..64 {
            sockets.push(tokio::net::TcpStream::connect(endpoint).await.unwrap());
        }
        // Give the accepted connections a turn to reserve their TLS slots.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut overflow = tokio::net::TcpStream::connect(endpoint).await.unwrap();
        let mut byte = [0];
        let result = tokio::time::timeout(Duration::from_secs(1), overflow.read(&mut byte)).await;
        assert!(
            matches!(result, Ok(Ok(0)) | Ok(Err(_))),
            "overflow connection must be rejected before TLS timeout"
        );
        drop(sockets);
        drop(host);
    }
    #[tokio::test]
    async fn pairing_delivers_native_credential_once_and_protected_stubs_authenticate() {
        let deps = fixture();
        let invitation = super::super::pairing::create_invitation(
            &deps.identity,
            "192.168.1.2:9010".parse().unwrap(),
            SystemTime::now(),
        )
        .unwrap();
        deps.pairing.register_invitation(&invitation).unwrap();
        let encoded = invitation.encode().unwrap();
        let request =
            serde_json::json!({"invitation":encoded.as_str(),"deviceName":"Phone"}).to_string();
        let response = call(
            router(deps.clone()),
            "/control/v1/pairing",
            "192.168.1.3:23456",
            &request,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let pending: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        let id: Uuid = serde_json::from_value(pending["pendingId"].clone()).unwrap();
        deps.approve(
            id,
            Grants {
                control: true,
                administration: false,
            },
        )
        .unwrap();
        let status = serde_json::json!({"pendingId":id,"invitation":encoded.as_str()}).to_string();
        let response = call(
            router(deps.clone()),
            "/control/v1/pairing/status",
            "192.168.1.3:23456",
            &status,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let credential: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert_eq!(
            call(
                router(deps.clone()),
                "/control/v1/pairing/status",
                "192.168.1.3:23456",
                &status
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            call(
                router(deps.clone()),
                "/control/v1/pairing",
                "192.168.1.3:23456",
                &request
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        for (path, expected, body) in [
            ("queries", StatusCode::NOT_IMPLEMENTED, "{}".into()),
            ("events", StatusCode::NOT_IMPLEMENTED, "{}".into()),
            ("resources", StatusCode::NOT_IMPLEMENTED, "{}".into()),
            ("admin", StatusCode::FORBIDDEN, "{}".into()),
            (
                "commands",
                StatusCode::PAYLOAD_TOO_LARGE,
                "x".repeat(1024 * 1024 + 1),
            ),
        ] {
            let mut request = axum::http::Request::builder()
                .method("POST")
                .uri(format!("/control/v1/{path}"))
                .header(
                    "authorization",
                    format!(
                        "Bearer {}:{}",
                        credential["deviceId"].as_str().unwrap(),
                        credential["secret"].as_str().unwrap()
                    ),
                )
                .body(Body::from(body))
                .unwrap();
            request.extensions_mut().insert(ConnectInfo(
                "192.168.1.3:23456".parse::<SocketAddr>().unwrap(),
            ));
            assert_eq!(
                router(deps.clone())
                    .oneshot(request)
                    .await
                    .unwrap()
                    .status(),
                expected
            );
        }
    }
    #[tokio::test]
    async fn pairing_source_admission_is_single_pending_and_released_after_delivery() {
        let deps = fixture();
        let invitation = || {
            let value = super::super::pairing::create_invitation(
                &deps.identity,
                "192.168.1.2:9010".parse().unwrap(),
                SystemTime::now(),
            )
            .unwrap();
            deps.pairing.register_invitation(&value).unwrap();
            value.encode().unwrap().to_string()
        };
        let first = invitation();
        let first_body = serde_json::json!({"invitation":first,"deviceName":"One"}).to_string();
        let response = call(
            router(deps.clone()),
            "/control/v1/pairing",
            "192.168.1.3:2345",
            &first_body,
        )
        .await;
        let pending: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        let id: Uuid = serde_json::from_value(pending["pendingId"].clone()).unwrap();
        let second_body =
            serde_json::json!({"invitation":invitation(),"deviceName":"Two"}).to_string();
        assert_eq!(
            call(
                router(deps.clone()),
                "/control/v1/pairing",
                "192.168.1.3:2346",
                &second_body
            )
            .await
            .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            call(
                router(deps.clone()),
                "/control/v1/pairing",
                "192.168.1.4:2346",
                &second_body
            )
            .await
            .status(),
            StatusCode::OK
        );
        deps.approve(
            id,
            Grants {
                control: true,
                administration: false,
            },
        )
        .unwrap();
        let status = serde_json::json!({"pendingId":id,"invitation":first}).to_string();
        assert_eq!(
            call(
                router(deps.clone()),
                "/control/v1/pairing/status",
                "192.168.1.3:2345",
                &status
            )
            .await
            .status(),
            StatusCode::OK
        );
        let third_body =
            serde_json::json!({"invitation":invitation(),"deviceName":"Three"}).to_string();
        assert_eq!(
            call(
                router(deps.clone()),
                "/control/v1/pairing",
                "192.168.1.3:2346",
                &third_body
            )
            .await
            .status(),
            StatusCode::OK
        );
    }
    #[tokio::test]
    async fn pairing_delivery_cleanup_covers_revoke_disable_and_idle_expiry() {
        let deps = fixture();
        async fn pending(deps: &Arc<HostDependencies>) -> Uuid {
            let invitation = super::super::pairing::create_invitation(
                &deps.identity,
                "192.168.1.2:9010".parse().unwrap(),
                SystemTime::now(),
            )
            .unwrap();
            deps.pairing.register_invitation(&invitation).unwrap();
            let body = serde_json::json!({"invitation":invitation.encode().unwrap().as_str(),"deviceName":"Phone"}).to_string();
            let response = call(
                router(deps.clone()),
                "/control/v1/pairing",
                "192.168.1.3:2345",
                &body,
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            let value: serde_json::Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap())
                    .unwrap();
            serde_json::from_value(value["pendingId"].clone()).unwrap()
        }
        let id = pending(&deps).await;
        deps.approve(
            id,
            Grants {
                control: true,
                administration: false,
            },
        )
        .unwrap();
        let device = deps.admission.lock().unwrap().deliveries[&id]
            .credential
            .as_ref()
            .unwrap()
            .device_id();
        deps.revoke(device).unwrap();
        assert!(deps.admission.lock().unwrap().deliveries.is_empty());
        pending(&deps).await;
        deps.invalidate_pairing().unwrap();
        assert!(deps.admission.lock().unwrap().deliveries.is_empty());
        assert!(deps.pairing.pending_pairings().unwrap().is_empty());
        let id = pending(&deps).await;
        deps.approve(
            id,
            Grants {
                control: true,
                administration: false,
            },
        )
        .unwrap();
        deps.admission
            .lock()
            .unwrap()
            .deliveries
            .get_mut(&id)
            .unwrap()
            .expires_at = SystemTime::now();
        let host = start_prebound(TcpListener::bind("127.0.0.1:0").unwrap(), deps.clone())
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if deps.admission.lock().unwrap().deliveries.is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("expired undelivered native credential must be dropped without client traffic");
        drop(host);
    }
    #[tokio::test]
    async fn pairing_body_rate_and_http_concurrency_are_bounded() {
        let deps = fixture();
        let too_large = call(
            router(deps.clone()),
            "/control/v1/pairing",
            "192.168.1.3:12345",
            &"x".repeat(4097),
        )
        .await;
        assert_eq!(too_large.status(), StatusCode::PAYLOAD_TOO_LARGE);
        for _ in 0..10 {
            let response = call(
                router(deps.clone()),
                "/control/v1/pairing",
                "192.168.1.3:12345",
                r#"{"invitation":"invalid","deviceName":"Phone"}"#,
            )
            .await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        }
        assert_eq!(
            call(
                router(deps.clone()),
                "/control/v1/pairing",
                "192.168.1.3:12345",
                r#"{"invitation":"invalid","deviceName":"Phone"}"#
            )
            .await
            .status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        let _permits = deps.requests.acquire_many(64).await.unwrap();
        assert_eq!(
            call(
                router(deps.clone()),
                "/control/v1/pairing",
                "192.168.1.4:12345",
                "{}"
            )
            .await
            .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
    async fn call(app: Router, path: &str, peer: &str, body: &str) -> Response {
        let mut request = axum::http::Request::builder()
            .method("POST")
            .uri(path)
            .body(Body::from(body.to_string()))
            .unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
        app.oneshot(request).await.unwrap()
    }
    #[tokio::test]
    async fn unauthorized_routes_fail_closed() {
        let deps = fixture();
        for route in [
            "handshake",
            "commands",
            "commands/status",
            "queries",
            "events",
            "resources",
            "admin",
        ] {
            let response = call(
                router(deps.clone()),
                &format!("/control/v1/{route}"),
                "192.168.1.3:12345",
                "{}",
            )
            .await;
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{route}");
        }
        let response = call(router(deps), "/control/v1/pairing", "8.8.8.8:12345", "{}").await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    #[test]
    fn production_bind_rejects_public_loopback_wildcard_and_squeeze_port() {
        for address in ["0.0.0.0", "127.0.0.1", "8.8.8.8", "169.254.1.1"] {
            assert!(LanConfig {
                address: address.parse().unwrap(),
                port: 9010
            }
            .validate()
            .is_err());
        }
        assert!(LanConfig {
            address: "192.168.1.2".parse().unwrap(),
            port: 9000
        }
        .validate()
        .is_err());
        assert_eq!(
            LanConfig {
                address: "192.168.1.2".parse().unwrap(),
                port: 9010
            }
            .validate()
            .unwrap(),
            "192.168.1.2:9010".parse::<SocketAddr>().unwrap()
        );
    }
}
