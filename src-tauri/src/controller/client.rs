//! Host-bound native transport. No credential is a frontend DTO.
use super::protocol::{ControlError, ControlErrorCode};
use std::net::SocketAddr;
use zeroize::Zeroizing;

pub(crate) struct NativePairing {
    host_id: String,
    endpoint: SocketAddr,
    ca_der: Vec<u8>,
    device_id: uuid::Uuid,
    credential: Zeroizing<[u8; 32]>,
}

impl NativePairing {
    pub fn new(
        host_id: String,
        endpoint: SocketAddr,
        ca_der: Vec<u8>,
        device_id: uuid::Uuid,
        credential: [u8; 32],
    ) -> Result<Self, ControlError> {
        validate_host_endpoint(&host_id, endpoint)?;
        let pairing = Self {
            host_id,
            endpoint,
            ca_der,
            device_id,
            credential: Zeroizing::new(credential),
        };
        build_host_client(&pairing)?;
        Ok(pairing)
    }
    pub fn server_name(&self) -> String {
        format!("audion-{}.invalid", self.host_id)
    }
    pub fn base_url(&self) -> String {
        format!("https://{}:{}", self.server_name(), self.endpoint.port())
    }
    pub fn host_id(&self) -> &str {
        &self.host_id
    }
    pub fn device_id(&self) -> uuid::Uuid {
        self.device_id
    }
    pub fn credential(&self) -> &[u8] {
        self.credential.as_ref()
    }
    /// Endpoint changes must discard the previous client/pool. Build first so a
    /// validation failure leaves the current endpoint and connection untouched.
    pub(crate) fn change_endpoint(
        &mut self,
        endpoint: SocketAddr,
    ) -> Result<NativeTransport, ControlError> {
        validate_host_endpoint(&self.host_id, endpoint)?;
        let pairing = Self::new(
            self.host_id.clone(),
            endpoint,
            self.ca_der.clone(),
            self.device_id,
            *self.credential,
        )?;
        let transport = NativeTransport::paired(pairing)?;
        self.endpoint = endpoint;
        Ok(transport)
    }
}

impl std::fmt::Debug for NativePairing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativePairing([REDACTED])")
    }
}

/// Public comparison hint only; pinned CA DER remains the trust authority.
pub(crate) fn ca_fingerprint(ca: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(ca))
}

pub(crate) fn invalid_pairing() -> ControlError {
    ControlError {
        code: ControlErrorCode::InvalidRequest,
        message: "Invalid or expired pairing invitation. Pair again.".into(),
        retryable: false,
    }
}

pub(crate) fn validate_host_endpoint(
    host_id: &str,
    endpoint: SocketAddr,
) -> Result<(), ControlError> {
    let id = uuid::Uuid::parse_str(host_id).map_err(|_| invalid_pairing())?;
    if id.to_string() != host_id
        || id.is_nil()
        || endpoint.port() == 0
        || !matches!(endpoint.ip(), std::net::IpAddr::V4(ip) if ip.is_private())
    {
        return Err(invalid_pairing());
    }
    Ok(())
}

fn build_trusted_client(
    host_id: &str,
    endpoint: SocketAddr,
    ca_der: &[u8],
) -> Result<reqwest::Client, ControlError> {
    let ca = reqwest::Certificate::from_der(ca_der).map_err(|_| invalid_pairing())?;
    reqwest::Client::builder()
        .use_rustls_tls()
        .tls_built_in_root_certs(false)
        .add_root_certificate(ca)
        .https_only(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .dns_resolver(std::sync::Arc::new(PairedResolver {
            name: format!("audion-{host_id}.invalid"),
            endpoint,
        }))
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|_| invalid_pairing())
}

pub(crate) fn validate_trust(
    host_id: &str,
    endpoint: SocketAddr,
    ca: &[u8],
) -> Result<(), ControlError> {
    build_trusted_client(host_id, endpoint, ca).map(|_| ())
}
#[cfg(all(test, desktop))]
pub(crate) fn trusted_client(
    host_id: &str,
    endpoint: SocketAddr,
    ca: &[u8],
) -> Result<reqwest::Client, ControlError> {
    build_trusted_client(host_id, endpoint, ca)
}

/// No system/public DNS fallback, even for an accidentally supplied other name.
struct PairedResolver {
    name: String,
    endpoint: SocketAddr,
}
impl reqwest::dns::Resolve for PairedResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let result: Result<reqwest::dns::Addrs, Box<dyn std::error::Error + Send + Sync>> =
            if name.as_str() == self.name {
                Ok(Box::new(std::iter::once(self.endpoint)))
            } else {
                Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "unpaired controller host",
                )))
            };
        Box::pin(async move { result })
    }
}

/// Always creates a fresh trust-bound pool; never mutate DNS on a live pool.
fn build_host_client(pairing: &NativePairing) -> Result<reqwest::Client, ControlError> {
    build_trusted_client(&pairing.host_id, pairing.endpoint, &pairing.ca_der)
}

use super::protocol::{
    ApplicationQuery, ArtworkReference, CommandEnvelope, EventBatch, EventCursor, ExecutionResult,
    HostCapabilities, QueryResult,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};

pub(crate) fn transport_error(code: ControlErrorCode) -> ControlError {
    ControlError {
        retryable: matches!(
            code,
            ControlErrorCode::HostNotReady | ControlErrorCode::Busy | ControlErrorCode::RateLimited
        ),
        code,
        message: "The paired PC could not confirm this operation.".into(),
    }
}
fn connection_error(error: reqwest::Error) -> ControlError {
    use std::error::Error;
    let mut source = error.source();
    while let Some(cause) = source {
        if cause
            .to_string()
            .to_ascii_lowercase()
            .contains("certificate")
        {
            return transport_error(ControlErrorCode::Unauthorized);
        }
        source = cause.source();
    }
    transport_error(if error.is_connect() {
        ControlErrorCode::HostNotReady
    } else {
        ControlErrorCode::OutcomeUnknown
    })
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ControlGrants { pub control: bool }
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Handshake {
    pub protocol_version: u8,
    pub host_id: String,
    pub host_epoch: String,
    pub capabilities: HostCapabilities,
    pub grants: ControlGrants,
}
#[derive(Deserialize, Serialize)]
#[serde(untagged)]
pub(crate) enum CommandStatus {
    Result(ExecutionResult),
    Pending(PendingStatus),
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PendingStatus {
    pub status: Pending,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Pending {
    Pending,
}
#[derive(Clone, Serialize)]
pub(crate) struct NativeImage {
    pub mime: String,
    pub bytes: Vec<u8>,
}
struct WireResponse {
    mime: String,
    bytes: Zeroizing<Vec<u8>>,
}
impl std::fmt::Debug for WireResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WireResponse([REDACTED])")
    }
}
/// No raw client/request builder escapes this facade. Even internally constructed
/// URLs must pass exact scheme, synthetic hostname and paired-port confinement.
pub(crate) struct NativeTransport {
    client: reqwest::Client,
    pairing: NativePairing,
    authenticated: bool,
}
impl NativeTransport {
    pub(crate) fn new(pairing: NativePairing, authenticated: bool) -> Result<Self, ControlError> {
        let client = build_host_client(&pairing)?;
        Ok(Self {
            client,
            pairing,
            authenticated,
        })
    }
    pub(crate) fn paired(pairing: NativePairing) -> Result<Self, ControlError> {
        Self::new(pairing, true)
    }
    pub(crate) fn invitation(pairing: NativePairing) -> Result<Self, ControlError> {
        Self::new(pairing, false)
    }
    fn origin_allowed(&self, url: &url::Url) -> bool {
        url.scheme() == "https"
            && url.host_str() == Some(self.pairing.server_name().as_str())
            && url.port_or_known_default() == Some(self.pairing.endpoint.port())
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
    }
    async fn send(
        &self,
        url: &str,
        body: &[u8],
        limit: usize,
    ) -> Result<WireResponse, ControlError> {
        let url = url::Url::parse(url).map_err(|_| invalid_pairing())?;
        if !self.origin_allowed(&url) {
            return Err(invalid_pairing());
        }
        let mut request = self
            .client
            .post(url)
            .header("content-type", "application/json")
            .body(body.to_vec());
        if self.authenticated {
            let secret = Zeroizing::new(URL_SAFE_NO_PAD.encode(self.pairing.credential()));
            let auth = Zeroizing::new(format!(
                "Bearer {}:{}",
                self.pairing.device_id(),
                secret.as_str()
            ));
            let mut header =
                reqwest::header::HeaderValue::from_str(&auth).map_err(|_| invalid_pairing())?;
            header.set_sensitive(true);
            request = request.header(reqwest::header::AUTHORIZATION, header);
        }
        let mut response = request.send().await.map_err(connection_error)?;
        let success = response.status().is_success();
        if response.status().is_redirection() {
            return Err(transport_error(ControlErrorCode::Unsupported));
        }
        let mime = response
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let bound = if success { limit } else { 4096 };
        if response.content_length().is_some_and(|n| n > bound as u64) {
            return Err(transport_error(ControlErrorCode::TooLarge));
        }
        let mut bytes = Zeroizing::new(Vec::new());
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| transport_error(ControlErrorCode::OutcomeUnknown))?
        {
            if bytes.len() + chunk.len() > bound {
                return Err(transport_error(ControlErrorCode::TooLarge));
            }
            bytes.extend_from_slice(&chunk);
        }
        if !success {
            return Err(serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| transport_error(ControlErrorCode::Unsupported)));
        }
        Ok(WireResponse { mime, bytes })
    }
    async fn json<T: serde::de::DeserializeOwned>(
        &self,
        path: &'static str,
        input: &impl Serialize,
    ) -> Result<T, ControlError> {
        let body = Zeroizing::new(serde_json::to_vec(input).map_err(|_| invalid_pairing())?);
        if body.len() > 1024 * 1024 {
            return Err(transport_error(ControlErrorCode::TooLarge));
        }
        let response = self
            .send(
                &format!("{}{path}", self.pairing.base_url()),
                &body,
                8 * 1024 * 1024,
            )
            .await?;
        serde_json::from_slice(&response.bytes)
            .map_err(|_| transport_error(ControlErrorCode::Unsupported))
    }
    pub(crate) async fn handshake(&self) -> Result<Handshake, ControlError> {
        self.json("/control/v1/handshake", &serde_json::json!({}))
            .await
    }
    pub(crate) async fn browse_metadata(&self, request: &super::protocol::BrowseMetadataRequest) -> Result<super::protocol::BrowseMetadataResult, ControlError> {
        if !request.valid() { return Err(transport_error(ControlErrorCode::InvalidRequest)); }
        let body = Zeroizing::new(serde_json::to_vec(request).map_err(|_|invalid_pairing())?);
        if body.len()>64*1024 { return Err(transport_error(ControlErrorCode::TooLarge)); }
        let response = self.send(&format!("{}/control/v1/browse-metadata",self.pairing.base_url()), &body,256*1024).await?;
        let result: super::protocol::BrowseMetadataResult = serde_json::from_slice(&response.bytes).map_err(|_|transport_error(ControlErrorCode::Unsupported))?;
        if !result.matches(request) { return Err(transport_error(ControlErrorCode::Unsupported)); }
        Ok(result)
    }
    pub(crate) async fn query(&self, q: &ApplicationQuery) -> Result<QueryResult, ControlError> {
        self.json("/control/v1/queries", q).await
    }
    pub(crate) async fn command(
        &self,
        c: &CommandEnvelope,
    ) -> Result<ExecutionResult, ControlError> {
        self.json("/control/v1/commands", c).await
    }
    pub(crate) async fn command_status(&self, id: &str) -> Result<CommandStatus, ControlError> {
        if id.is_empty() || id.len() > 128 {
            return Err(invalid_pairing());
        }
        self.json(
            "/control/v1/commands/status",
            &serde_json::json!({"requestId":id}),
        )
        .await
    }
    pub(crate) async fn poll(&self, c: &EventCursor) -> Result<EventBatch, ControlError> {
        self.json("/control/v1/events", c).await
    }
    pub(crate) async fn media(&self, r: &ArtworkReference) -> Result<NativeImage, ControlError> {
        let response = self
            .send(
                &format!("{}/control/v1/resources", self.pairing.base_url()),
                &serde_json::to_vec(r).map_err(|_| invalid_pairing())?,
                5 * 1024 * 1024,
            )
            .await?;
        if !matches!(
            response.mime.as_str(),
            "image/png" | "image/jpeg" | "image/webp"
        ) || response.bytes.is_empty()
        {
            return Err(transport_error(ControlErrorCode::Unsupported));
        }
        let format = image::guess_format(&response.bytes)
            .map_err(|_| transport_error(ControlErrorCode::Unsupported))?;
        let expected = match format {
            image::ImageFormat::Png => "image/png",
            image::ImageFormat::Jpeg => "image/jpeg",
            image::ImageFormat::WebP => "image/webp",
            _ => return Err(transport_error(ControlErrorCode::Unsupported)),
        };
        if expected != response.mime {
            return Err(transport_error(ControlErrorCode::Unsupported));
        }
        let mut reader = image::ImageReader::with_format(
            std::io::Cursor::new(response.bytes.as_slice()),
            format,
        );
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(4096);
        limits.max_image_height = Some(4096);
        limits.max_alloc = Some(64 * 1024 * 1024);
        reader.limits(limits);
        reader
            .decode()
            .map_err(|_| transport_error(ControlErrorCode::Unsupported))?;
        Ok(NativeImage {
            mime: response.mime,
            bytes: response.bytes.to_vec(),
        })
    }
    pub(crate) async fn pair_start(
        &self,
        invitation: &str,
        name: &str,
    ) -> Result<PairPending, ControlError> {
        self.json(
            "/control/v1/pairing",
            &serde_json::json!({"invitation":invitation,"deviceName":name}),
        )
        .await
    }
    pub(crate) async fn pair_status(
        &self,
        invitation: &str,
        id: &str,
    ) -> Result<PairReply, ControlError> {
        self.json(
            "/control/v1/pairing/status",
            &serde_json::json!({"invitation":invitation,"pendingId":id}),
        )
        .await
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PairPending {
    #[serde(rename = "status")]
    _status: Pending,
    pub pending_id: String,
}
#[derive(Deserialize)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum PairReply {
    Pending,
    Approved { device_id: String, secret: String },
}
impl Drop for PairReply {
    fn drop(&mut self) {
        if let Self::Approved { secret, .. } = self {
            zeroize::Zeroize::zeroize(secret);
        }
    }
}

#[cfg(all(test, desktop))]
pub(super) mod tests {
    use super::*;
    use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair};
    use std::{
        io::{Read, Write},
        sync::Arc,
        time::Duration,
    };

    pub(crate) fn loopback_transport(
        pairing: NativePairing,
        authenticated: bool,
        dial: SocketAddr,
    ) -> Result<NativeTransport, ControlError> {
        validate_host_endpoint(pairing.host_id(), pairing.endpoint)?;
        let mut transport = NativeTransport::new(pairing, authenticated)?;
        assert!(dial.ip().is_loopback());
        assert_eq!(dial.port(), transport.pairing.endpoint.port());
        transport.client =
            trusted_client(transport.pairing.host_id(), dial, &transport.pairing.ca_der)?;
        Ok(transport)
    }
    pub(crate) async fn outgoing_url(
        transport: &NativeTransport,
        url: &str,
    ) -> Result<(), ControlError> {
        transport.send(url, b"{}", 1024).await.map(|_| ())
    }
    const HOST: &str = "cf8dd70c-8cc2-4640-bd40-5b06f68cc301";
    #[tokio::test]
    async fn client_refuses_unpaired_names_without_system_dns() {
        let (ca, _) = authority();
        let pairing = NativePairing::new(
            HOST.into(),
            "192.168.1.8:9010".parse().unwrap(),
            ca.der().to_vec(),
            uuid::Uuid::new_v4(),
            [7; 32],
        )
        .unwrap();
        // localhost is resolved locally; this RED fixture never contacts public DNS.
        let error = build_host_client(&pairing)
            .unwrap()
            .get("https://localhost:9")
            .send()
            .await
            .unwrap_err();
        assert!(
            format!("{error:?}").contains("unpaired controller host"),
            "{error:?}"
        );
    }
    #[test]
    fn production_endpoint_change_preserves_identity_and_rejects_non_lan_updates() {
        let (ca, _) = authority();
        let device = uuid::Uuid::new_v4();
        let mut pairing = NativePairing::new(
            HOST.into(),
            "192.168.1.8:9010".parse().unwrap(),
            ca.der().to_vec(),
            device,
            [7; 32],
        )
        .unwrap();
        let _fresh = pairing
            .change_endpoint("10.1.2.3:443".parse().unwrap())
            .unwrap();
        assert_eq!(
            pairing.base_url(),
            format!("https://audion-{HOST}.invalid:443")
        );
        for invalid in [
            "127.0.0.1:9010",
            "8.8.8.8:9010",
            "169.254.1.1:9010",
            "[fd00::1]:9010",
            "10.1.2.3:0",
        ] {
            assert!(pairing.change_endpoint(invalid.parse().unwrap()).is_err());
            assert_eq!(
                pairing.base_url(),
                format!("https://audion-{HOST}.invalid:443")
            );
        }
        assert_eq!(pairing.device_id(), device);
        assert_eq!(pairing.credential(), &[7; 32]);
        assert_eq!(pairing.ca_der, ca.der().to_vec());
    }
    pub(super) fn authority() -> (rcgen::Certificate, KeyPair) {
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new(vec![]).unwrap();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        (params.self_signed(&key).unwrap(), key)
    }
    fn serve(
        ca: &rcgen::Certificate,
        key: &KeyPair,
        name: &str,
        expired: bool,
        reply: &'static str,
        bind: SocketAddr,
    ) -> (SocketAddr, std::thread::JoinHandle<()>) {
        let leaf_key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new(vec![name.into()]).unwrap();
        if expired {
            params.not_before = time::OffsetDateTime::now_utc() - time::Duration::days(2);
            params.not_after = time::OffsetDateTime::now_utc() - time::Duration::days(1);
        }
        let issuer = Issuer::from_ca_cert_der(ca.der(), key).unwrap();
        let cert = params.signed_by(&leaf_key, &issuer).unwrap();
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(leaf_key.serialize_der()).into(),
        )
        .unwrap();
        let listener = std::net::TcpListener::bind(bind).unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let worker = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                if let Ok((socket, _)) = listener.accept() {
                    // Windows accepted sockets inherit the listener's mode.
                    socket.set_nonblocking(false).unwrap();
                    socket
                        .set_read_timeout(Some(Duration::from_secs(3)))
                        .unwrap();
                    socket
                        .set_write_timeout(Some(Duration::from_secs(3)))
                        .unwrap();
                    let connection = rustls::ServerConnection::new(Arc::new(config)).unwrap();
                    let mut stream = rustls::StreamOwned::new(connection, socket);
                    let mut request = [0; 2048];
                    if stream.read(&mut request).is_ok() {
                        let _ = write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            reply.len(),
                            reply
                        );
                        let _ = stream.flush();
                    }
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        (address, worker)
    }
    #[tokio::test]
    async fn client_rejects_wrong_ca_name_or_expiry() {
        let (ca, key) = authority();
        let (other_ca, _) = authority();
        for (wrong_ca, wrong_name, expired) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            let name = format!("audion-{HOST}.invalid");
            let (endpoint, worker) = serve(
                &ca,
                &key,
                if wrong_name { "other.invalid" } else { &name },
                expired,
                "trusted",
                "127.0.0.1:0".parse().unwrap(),
            );
            // Test-only direct construction admits loopback; production callers
            // must pass the private-IPv4 constructor, with no loopback override.
            let pairing = NativePairing {
                host_id: HOST.into(),
                endpoint,
                ca_der: if wrong_ca {
                    other_ca.der().to_vec()
                } else {
                    ca.der().to_vec()
                },
                device_id: uuid::Uuid::new_v4(),
                credential: Zeroizing::new([7; 32]),
            };
            let client = build_host_client(&pairing).expect("paired CA client must build");
            let result = client.get(pairing.base_url()).send().await;
            assert_eq!(result.is_ok(), !(wrong_ca || wrong_name || expired));
            if let Ok(response) = result {
                assert_eq!(response.text().await.unwrap(), "trusted");
            }
            worker.join().unwrap();
        }
    }
    #[tokio::test]
    async fn endpoint_change_rebuilds_trust_bound_pool() {
        let (ca, key) = authority();
        let name = format!("audion-{HOST}.invalid");
        let first_reply = r#"{"protocolVersion":1,"hostId":"cf8dd70c-8cc2-4640-bd40-5b06f68cc301","hostEpoch":"first","capabilities":{"queries":[],"intents":[]},"grants":{"control":true}}"#;
        let second_reply = r#"{"protocolVersion":1,"hostId":"cf8dd70c-8cc2-4640-bd40-5b06f68cc301","hostEpoch":"second","capabilities":{"queries":[],"intents":[]},"grants":{"control":true}}"#;
        let (first, first_worker) = serve(
            &ca,
            &key,
            &name,
            false,
            first_reply,
            "127.0.0.1:0".parse().unwrap(),
        );
        let mut pairing = NativePairing::new(
            HOST.into(),
            format!("192.168.1.8:{}", first.port()).parse().unwrap(),
            ca.der().to_vec(),
            uuid::Uuid::new_v4(),
            [7; 32],
        )
        .unwrap();
        let initial = pairing
            .change_endpoint(format!("192.168.1.8:{}", first.port()).parse().unwrap())
            .unwrap();
        // Test dial override only, after real production endpoint validation.
        let mut initial = initial;
        initial.client = trusted_client(HOST, first, ca.der()).unwrap();
        assert_eq!(initial.handshake().await.unwrap().host_epoch, "first");
        drop(initial);
        first_worker.join().unwrap();
        let (second, second_worker) = serve(
            &ca,
            &key,
            &name,
            false,
            second_reply,
            SocketAddr::new("127.0.0.2".parse().unwrap(), first.port()),
        );
        let mut changed = pairing
            .change_endpoint(format!("10.1.2.3:{}", first.port()).parse().unwrap())
            .unwrap();
        changed.client = trusted_client(HOST, second, ca.der()).unwrap();
        assert_eq!(changed.handshake().await.unwrap().host_epoch, "second");
        second_worker.join().unwrap();
    }
}

#[cfg(all(test, desktop))]
mod facade_tests {
    use super::*;
    #[tokio::test]
    async fn exact_origin_rejects_ip_scheme_and_wrong_port_before_send() {
        let (ca, _) = super::tests::authority();
        let pairing = NativePairing::new(
            "cf8dd70c-8cc2-4640-bd40-5b06f68cc301".into(),
            "192.168.1.8:9010".parse().unwrap(),
            ca.der().to_vec(),
            uuid::Uuid::new_v4(),
            [7; 32],
        )
        .unwrap();
        let transport = NativeTransport::paired(pairing).unwrap();
        for origin in [
            "https://127.0.0.1:9010",
            "http://audion-cf8dd70c-8cc2-4640-bd40-5b06f68cc301.invalid:9010",
            "https://audion-cf8dd70c-8cc2-4640-bd40-5b06f68cc301.invalid:9011",
        ] {
            let error = transport
                .send(&format!("{origin}/control/v1/handshake"), b"{}", 1024)
                .await
                .unwrap_err();
            assert_eq!(error.code, ControlErrorCode::InvalidRequest);
        }
    }
}
