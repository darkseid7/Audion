//! Native-only private CA and server leaf. No key material is a frontend DTO.
use super::{
    protocol::{ControlError, ControlErrorCode},
    secrets::{vault_unavailable, SecretStore, TRANSACTION},
};
use rcgen::{
    BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    sync::Arc,
    time::{Duration, SystemTime},
};
use time::OffsetDateTime;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

const IDENTITY_KEY: &str = "host-identity-v1";
const LEAF_LIFETIME: Duration = Duration::from_secs(90 * 86400);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProtectedIdentity {
    version: u8,
    id: String,
    ca_pem: String,
    ca_key_pem: String,
    #[serde(with = "der_base64")]
    leaf_der: Vec<u8>,
    leaf_key_pem: String,
    leaf_expires_at: SystemTime,
}
// JSON number arrays exceed Windows Credential Manager's 2560-byte entry
// budget. The certificate is public; base64 is compact encoding, not encryption.
mod der_base64 {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        STANDARD
            .decode(String::deserialize(deserializer)?)
            .map_err(serde::de::Error::custom)
    }
}
impl Drop for ProtectedIdentity {
    fn drop(&mut self) {
        self.ca_key_pem.zeroize();
        self.leaf_key_pem.zeroize();
    }
}

pub struct HostIdentity {
    protected: ProtectedIdentity,
    ca_der: Vec<u8>,
}
impl fmt::Debug for HostIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostIdentity")
            .field("id", &self.protected.id)
            .finish_non_exhaustive()
    }
}

fn identity_error() -> ControlError {
    ControlError {
        code: ControlErrorCode::HostNotReady,
        message: "Host identity is unavailable or invalid. New trust requires new pairing.".into(),
        retryable: false,
    }
}

impl HostIdentity {
    pub fn id(&self) -> &str {
        &self.protected.id
    }
    pub fn ca_der(&self) -> &[u8] {
        &self.ca_der
    }
    pub fn leaf_der(&self) -> &[u8] {
        &self.protected.leaf_der
    }
    pub fn server_name(&self) -> String {
        format!("audion-{}.invalid", self.id())
    }

    /// Native TLS handoff only; callers must never log or send this to a WebView.
    pub fn leaf_private_key(
        &self,
    ) -> Result<rustls::pki_types::PrivateKeyDer<'static>, ControlError> {
        let key = KeyPair::from_pem(&self.protected.leaf_key_pem).map_err(|_| identity_error())?;
        Ok(rustls::pki_types::PrivatePkcs8KeyDer::from(key.serialize_der()).into())
    }

    pub fn renew_leaf(
        &mut self,
        store: &dyn SecretStore,
        now: SystemTime,
    ) -> Result<(), ControlError> {
        let _guard = TRANSACTION.lock().map_err(|_| vault_unavailable())?;
        self.renew_and_persist(store, now)
    }

    fn renew_and_persist(
        &mut self,
        store: &dyn SecretStore,
        now: SystemTime,
    ) -> Result<(), ControlError> {
        let key = KeyPair::from_pem(&self.protected.ca_key_pem).map_err(|_| identity_error())?;
        let issuer =
            Issuer::from_ca_cert_pem(&self.protected.ca_pem, key).map_err(|_| identity_error())?;
        let leaf_key = KeyPair::generate().map_err(|_| identity_error())?;
        let mut params =
            CertificateParams::new(vec![self.server_name()]).map_err(|_| identity_error())?;
        params.not_before = OffsetDateTime::from(now) - time::Duration::minutes(5);
        params.not_after = OffsetDateTime::from(now + LEAF_LIFETIME);
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let leaf = params
            .signed_by(&leaf_key, &issuer)
            .map_err(|_| identity_error())?;
        validate_leaf(&self.ca_der, leaf.der(), &self.server_name(), now)?;
        // Publish the new leaf only after the complete protected record is durable.
        let next = ProtectedIdentity {
            version: 1,
            id: self.protected.id.clone(),
            ca_pem: self.protected.ca_pem.clone(),
            ca_key_pem: self.protected.ca_key_pem.clone(),
            leaf_der: leaf.der().to_vec(),
            leaf_key_pem: leaf_key.serialize_pem(),
            leaf_expires_at: now + LEAF_LIFETIME,
        };
        persist(store, &next)?;
        self.protected = next;
        Ok(())
    }
}

fn persist(store: &dyn SecretStore, identity: &ProtectedIdentity) -> Result<(), ControlError> {
    let bytes = Zeroizing::new(serde_json::to_vec(identity).map_err(|_| identity_error())?);
    store.write(IDENTITY_KEY, &bytes)
}

fn validate_leaf(ca: &[u8], leaf: &[u8], name: &str, now: SystemTime) -> Result<(), ControlError> {
    use rustls::{
        client::danger::ServerCertVerifier,
        pki_types::{CertificateDer, ServerName, UnixTime},
    };
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(CertificateDer::from(ca))
        .map_err(|_| identity_error())?;
    let verifier = rustls::client::WebPkiServerVerifier::builder_with_provider(
        Arc::new(roots),
        Arc::new(rustls::crypto::ring::default_provider()),
    )
    .build()
    .map_err(|_| identity_error())?;
    verifier
        .verify_server_cert(
            &CertificateDer::from(leaf),
            &[],
            &ServerName::try_from(name).map_err(|_| identity_error())?,
            &[],
            UnixTime::since_unix_epoch(
                now.duration_since(SystemTime::UNIX_EPOCH)
                    .map_err(|_| identity_error())?,
            ),
        )
        .map_err(|_| identity_error())?;
    Ok(())
}

pub fn load_or_create_identity(store: &dyn SecretStore) -> Result<HostIdentity, ControlError> {
    let _guard = TRANSACTION.lock().map_err(|_| vault_unavailable())?;
    let now = SystemTime::now();
    if let Some(bytes) = store.read(IDENTITY_KEY)? {
        let bytes = Zeroizing::new(bytes);
        let protected: ProtectedIdentity =
            serde_json::from_slice(&bytes).map_err(|_| identity_error())?;
        if protected.version != 1 || Uuid::parse_str(&protected.id).is_err() {
            return Err(identity_error());
        }
        // Parsing the stored CA does not itself prove that its key matches; renewal
        // validates a signed leaf against the exact persisted public trust anchor.
        use rustls::pki_types::pem::PemObject;
        let ca_der = rustls::pki_types::CertificateDer::from_pem_slice(protected.ca_pem.as_bytes())
            .map_err(|_| identity_error())?
            .to_vec();
        let mut identity = HostIdentity { protected, ca_der };
        if identity.protected.leaf_expires_at <= now + Duration::from_secs(7 * 86400) {
            identity.renew_and_persist(store, now)?;
        } else {
            validate_leaf(
                identity.ca_der(),
                identity.leaf_der(),
                &identity.server_name(),
                now,
            )?;
            // rustls checks that the stored leaf key matches its certificate.
            rustls::ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .map_err(|_| identity_error())?
            .with_no_client_auth()
            .with_single_cert(
                vec![identity.leaf_der().to_vec().into()],
                identity.leaf_private_key()?,
            )
            .map_err(|_| identity_error())?;
        }
        return Ok(identity);
    }
    let id = Uuid::new_v4().to_string();
    let ca_key = KeyPair::generate().map_err(|_| identity_error())?;
    let mut params = CertificateParams::new(Vec::new()).map_err(|_| identity_error())?;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    params.not_before = OffsetDateTime::from(now) - time::Duration::minutes(5);
    params.not_after = OffsetDateTime::from(now) + time::Duration::days(3650);
    let ca = params.self_signed(&ca_key).map_err(|_| identity_error())?;
    let mut identity = HostIdentity {
        ca_der: ca.der().to_vec(),
        protected: ProtectedIdentity {
            version: 1,
            id,
            ca_pem: ca.pem(),
            ca_key_pem: ca_key.serialize_pem(),
            leaf_der: vec![],
            leaf_key_pem: String::new(),
            leaf_expires_at: now,
        },
    };
    identity.renew_and_persist(store, now)?;
    Ok(identity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::secrets::tests::{MemoryStore, UnavailableStore};

    #[test]
    fn unavailable_vault_keeps_host_disabled() {
        let result = load_or_create_identity(&UnavailableStore);
        assert!(matches!(
            result,
            Err(ControlError {
                code: ControlErrorCode::HostNotReady,
                ..
            })
        ));
    }

    #[test]
    fn protected_identity_survives_reload() {
        let store = MemoryStore::default();
        let identity = load_or_create_identity(&store).unwrap();
        let reloaded = load_or_create_identity(&store).unwrap();
        assert_eq!(identity.id(), reloaded.id());
        assert_eq!(identity.ca_der(), reloaded.ca_der());
        assert_eq!(identity.leaf_der(), reloaded.leaf_der());
    }

    #[test]
    fn identity_and_renewal_fit_native_vault_blob_budget() {
        let store = MemoryStore::default();
        let mut identity = load_or_create_identity(&store).unwrap();
        assert!(store.read(IDENTITY_KEY).unwrap().unwrap().len() <= 2560);
        identity
            .renew_leaf(&store, SystemTime::now() + Duration::from_secs(86400))
            .unwrap();
        assert!(store.read(IDENTITY_KEY).unwrap().unwrap().len() <= 2560);
    }

    fn validate(ca: &[u8], leaf: &[u8], name: &str, at: SystemTime) -> Result<(), rustls::Error> {
        use rustls::{
            client::danger::ServerCertVerifier,
            pki_types::{CertificateDer, ServerName, UnixTime},
        };
        let mut roots = rustls::RootCertStore::empty();
        roots.add(CertificateDer::from(ca)).unwrap();
        let verifier = rustls::client::WebPkiServerVerifier::builder_with_provider(
            std::sync::Arc::new(roots),
            std::sync::Arc::new(rustls::crypto::ring::default_provider()),
        )
        .build()
        .unwrap();
        verifier
            .verify_server_cert(
                &CertificateDer::from(leaf),
                &[],
                &ServerName::try_from(name).unwrap(),
                &[],
                UnixTime::since_unix_epoch(at.duration_since(SystemTime::UNIX_EPOCH).unwrap()),
            )
            .map(|_| ())
    }

    #[test]
    fn renewed_leaf_validates_under_same_ca_and_old_leaf_expires() {
        let store = MemoryStore::default();
        let mut identity = load_or_create_identity(&store).unwrap();
        let old_leaf = identity.leaf_der().to_vec();
        let ca = identity.ca_der().to_vec();
        let now = SystemTime::now();
        validate(&ca, &old_leaf, &identity.server_name(), now).unwrap();
        assert!(validate(&ca, &old_leaf, "wrong.invalid", now).is_err());
        let later = now + Duration::from_secs(100 * 86400);
        assert!(matches!(
            validate(&ca, &old_leaf, &identity.server_name(), later),
            Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::Expired | rustls::CertificateError::ExpiredContext { .. }
            ))
        ));
        identity.renew_leaf(&store, later).unwrap();
        assert_eq!(identity.ca_der(), ca);
        assert_ne!(identity.leaf_der(), old_leaf);
        validate(&ca, identity.leaf_der(), &identity.server_name(), later).unwrap();
        let replacement = load_or_create_identity(&MemoryStore::default()).unwrap();
        assert!(validate(&ca, replacement.leaf_der(), &replacement.server_name(), now).is_err());
    }

    #[test]
    fn corrupt_identity_is_not_silently_replaced() {
        let store = MemoryStore::default();
        store.write(IDENTITY_KEY, b"not an identity").unwrap();
        assert!(load_or_create_identity(&store).is_err());
        assert_eq!(
            store.read(IDENTITY_KEY).unwrap().unwrap(),
            b"not an identity"
        );
    }
}
