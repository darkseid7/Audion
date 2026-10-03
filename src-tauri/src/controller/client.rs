//! Host-bound native transport. No credential is a frontend DTO.
use super::protocol::{ControlError, ControlErrorCode};
use std::net::SocketAddr;
use zeroize::Zeroizing;

pub struct NativePairing {
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
    pub fn change_endpoint(
        &mut self,
        endpoint: SocketAddr,
    ) -> Result<reqwest::Client, ControlError> {
        validate_host_endpoint(&self.host_id, endpoint)?;
        let client = trusted_client(&self.host_id, endpoint, &self.ca_der)?;
        self.endpoint = endpoint;
        Ok(client)
    }
}

impl std::fmt::Debug for NativePairing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativePairing([REDACTED])")
    }
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

pub(crate) fn trusted_client(
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
pub fn build_host_client(pairing: &NativePairing) -> Result<reqwest::Client, ControlError> {
    trusted_client(&pairing.host_id, pairing.endpoint, &pairing.ca_der)
}

#[cfg(all(test, desktop))]
mod tests {
    use super::*;
    use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair};
    use std::{
        io::{Read, Write},
        sync::Arc,
        time::Duration,
    };

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
    fn authority() -> (rcgen::Certificate, KeyPair) {
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
        let (first, first_worker) = serve(
            &ca,
            &key,
            &name,
            false,
            "first",
            "127.0.0.1:0".parse().unwrap(),
        );
        let mut pairing = NativePairing {
            host_id: HOST.into(),
            endpoint: first,
            ca_der: ca.der().to_vec(),
            device_id: uuid::Uuid::new_v4(),
            credential: Zeroizing::new([7; 32]),
        };
        let first_client = build_host_client(&pairing).unwrap();
        assert_eq!(
            first_client
                .get(pairing.base_url())
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap(),
            "first"
        );
        first_worker.join().unwrap();
        // The origin (including port) is unchanged. A retained resolver/pool
        // would still target 127.0.0.1 instead of the new loopback fixture.
        let (second, second_worker) = serve(
            &ca,
            &key,
            &name,
            false,
            "second",
            SocketAddr::new("127.0.0.2".parse().unwrap(), first.port()),
        );
        pairing.endpoint = second;
        let second_client = build_host_client(&pairing).unwrap();
        assert_eq!(
            second_client
                .get(pairing.base_url())
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap(),
            "second"
        );
        second_worker.join().unwrap();
    }
}
