//! One-use invitations and explicit desktop approvals. No raw device credential
//! is serializable, persisted, or available for recovery after issuance.
use super::{
    identity::HostIdentity,
    protocol::{ControlError, ControlErrorCode},
    secrets::{vault_unavailable, SecretStore, TRANSACTION},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::{rngs::OsRng, TryRngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fmt,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};
use subtle::ConstantTimeEq;
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct PairingInvitation {
    id: Uuid,
    host_id: String,
    ca_der: Vec<u8>,
    endpoint: SocketAddr,
    secret: Zeroizing<[u8; 32]>,
    pub expires_at: SystemTime,
}
impl fmt::Debug for PairingInvitation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PairingInvitation([REDACTED])")
    }
}
impl PairingInvitation {
    /// Sensitive, short-lived QR/copy payload. Never log or persist this string.
    /// `expiresAt` is `{ unixSeconds, nanoseconds }`: integer seconds since the
    /// Unix epoch (0..=2^53-1) plus canonical nanoseconds (0..=999_999_999).
    /// Native consumers must reconstruct both parts without rounding, reject
    /// out-of-range fields, and use checked time arithmetic. The WebView treats
    /// the complete invitation as an opaque string, not a floating-point date.
    pub fn encode(&self) -> Result<Zeroizing<String>, ControlError> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Expiration {
            unix_seconds: u64,
            nanoseconds: u32,
        }
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Payload<'a> {
            version: u8,
            invitation_id: String,
            host_id: &'a str,
            ca: String,
            endpoint: String,
            secret: String,
            expires_at: Expiration,
        }
        let expiry = self
            .expires_at
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|_| invalid_request())?;
        if expiry.as_secs() > 9_007_199_254_740_991 {
            return Err(invalid_request());
        }
        let mut payload = Payload {
            version: 1,
            invitation_id: self.id.to_string(),
            host_id: &self.host_id,
            ca: URL_SAFE_NO_PAD.encode(&self.ca_der),
            endpoint: self.endpoint.to_string(),
            secret: URL_SAFE_NO_PAD.encode(self.secret.as_ref()),
            expires_at: Expiration {
                unix_seconds: expiry.as_secs(),
                nanoseconds: expiry.subsec_nanos(),
            },
        };
        let encoded = serde_json::to_string(&payload)
            .map(Zeroizing::new)
            .map_err(|_| invalid_request());
        use zeroize::Zeroize;
        payload.secret.zeroize();
        encoded
    }
    pub fn qr_svg(&self) -> Result<Zeroizing<String>, ControlError> {
        let encoded = self.encode()?;
        let code = qrcode::QrCode::new(encoded.as_bytes()).map_err(|_| invalid_request())?;
        Ok(Zeroizing::new(
            code.render::<qrcode::render::svg::Color>().build(),
        ))
    }
}

fn invalid_request() -> ControlError {
    ControlError {
        code: ControlErrorCode::InvalidRequest,
        message: "Invalid pairing request.".into(),
        retryable: false,
    }
}
fn unauthorized() -> ControlError {
    ControlError {
        code: ControlErrorCode::Unauthorized,
        message: "Pairing or device authorization is unavailable. Pair again.".into(),
        retryable: false,
    }
}
fn random_secret() -> Result<Zeroizing<[u8; 32]>, ControlError> {
    let mut bytes = Zeroizing::new([0u8; 32]);
    OsRng
        .try_fill_bytes(bytes.as_mut())
        .map_err(|_| vault_unavailable())?;
    Ok(bytes)
}
fn hash(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
fn private_endpoint(endpoint: SocketAddr) -> bool {
    endpoint.port() != 0
        && match endpoint.ip() {
            IpAddr::V4(ip) => ip.is_private() || ip.is_link_local(),
            IpAddr::V6(ip) => ip.is_unique_local() || ip.is_unicast_link_local(),
        }
}

pub fn create_invitation(
    identity: &HostIdentity,
    endpoint: SocketAddr,
    now: SystemTime,
) -> Result<PairingInvitation, ControlError> {
    if !private_endpoint(endpoint) {
        return Err(invalid_request());
    }
    Ok(PairingInvitation {
        id: Uuid::new_v4(),
        host_id: identity.id().into(),
        ca_der: identity.ca_der().into(),
        endpoint,
        secret: random_secret()?,
        expires_at: now
            .checked_add(Duration::from_secs(300))
            .ok_or_else(invalid_request)?,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grants {
    pub control: bool,
    pub administration: bool,
}

#[derive(Debug, Clone)]
pub struct PendingPairing {
    pub id: Uuid,
    pub device_name: String,
    pub expires_at: SystemTime,
}

/// Must stay on the native side of the TLS pairing exchange. No Serialize/Clone.
pub struct NativeCredential {
    device_id: Uuid,
    secret: Zeroizing<[u8; 32]>,
}
impl fmt::Debug for NativeCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NativeCredential([REDACTED])")
    }
}
impl NativeCredential {
    pub fn device_id(&self) -> Uuid {
        self.device_id
    }
    pub fn secret(&self) -> &[u8] {
        self.secret.as_ref()
    }
}

#[derive(Debug, Clone)]
pub struct PairedDevice {
    pub id: Uuid,
    pub name: String,
    pub grants: Grants,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeviceRecord {
    name: String,
    secret_hash: [u8; 32],
    grants: Grants,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Approvals {
    devices: Vec<String>,
    // Write-ahead cleanup slot bounds orphan records to one, even after a crash.
    // A pending ID is never authorized by membership.
    cleanup: Option<String>,
}
struct InvitationRecord {
    secret_hash: [u8; 32],
    endpoint: SocketAddr,
    expires_at: SystemTime,
    consumed: bool,
}
#[derive(Default)]
struct PairingState {
    invitations: HashMap<Uuid, InvitationRecord>,
    pending: HashMap<Uuid, PendingPairing>,
}

pub struct PairingService {
    host_id: String,
    ca_hash: [u8; 32],
    approvals_key: String,
    store: Arc<dyn SecretStore>,
    state: Mutex<PairingState>,
}

impl PairingService {
    pub fn new(identity: &HostIdentity, store: Arc<dyn SecretStore>) -> Result<Self, ControlError> {
        let ca_hash = hash(identity.ca_der());
        // Replacing the CA changes the credential namespace, even if a host ID
        // is retained. Old credentials cannot authenticate under new trust.
        let service = Self {
            host_id: identity.id().into(),
            ca_hash,
            approvals_key: format!(
                "devices-v1-{}-{}",
                identity.id(),
                URL_SAFE_NO_PAD.encode(ca_hash)
            ),
            store,
            state: Mutex::new(PairingState::default()),
        };
        let _guard = TRANSACTION.lock().map_err(|_| vault_unavailable())?;
        service.read_approvals()?;
        Ok(service)
    }

    fn read_approvals(&self) -> Result<Approvals, ControlError> {
        let mut approvals: Approvals = match self.store.read(&self.approvals_key)? {
            Some(bytes) => {
                serde_json::from_slice(&Zeroizing::new(bytes)).map_err(|_| vault_unavailable())?
            }
            None => Approvals::default(),
        };
        if approvals.devices.len() > 32
            || approvals
                .devices
                .iter()
                .any(|id| Uuid::parse_str(id).is_err())
        {
            return Err(vault_unavailable());
        }
        if let Some(id) = approvals.cleanup.as_ref() {
            if Uuid::parse_str(id).is_err() || approvals.devices.contains(id) {
                return Err(vault_unavailable());
            }
            self.store.delete(&self.device_key(id))?;
            approvals.cleanup = None;
            self.persist(&approvals)?;
        }
        Ok(approvals)
    }
    fn device_key(&self, id: &str) -> String {
        format!("{}-{id}", self.approvals_key)
    }
    fn read_device(&self, id: &str) -> Result<DeviceRecord, ControlError> {
        let bytes = self
            .store
            .read(&self.device_key(id))?
            .ok_or_else(vault_unavailable)?;
        serde_json::from_slice(&Zeroizing::new(bytes)).map_err(|_| vault_unavailable())
    }
    fn persist(&self, approvals: &Approvals) -> Result<(), ControlError> {
        let bytes = Zeroizing::new(serde_json::to_vec(approvals).map_err(|_| vault_unavailable())?);
        self.store.write(&self.approvals_key, &bytes)
    }

    /// Only called for an invitation created by this host's trusted native UI.
    /// Registrations retain consumed tombstones until expiry to prevent reuse.
    pub fn register_invitation(&self, invite: &PairingInvitation) -> Result<(), ControlError> {
        let now = SystemTime::now();
        if invite.host_id != self.host_id
            || hash(&invite.ca_der) != self.ca_hash
            || invite.expires_at <= now
            || invite.expires_at > now + Duration::from_secs(300)
            || !private_endpoint(invite.endpoint)
        {
            return Err(unauthorized());
        }
        let mut state = self.state.lock().map_err(|_| vault_unavailable())?;
        state.invitations.retain(|_, value| value.expires_at > now);
        if state.invitations.len() >= 64 || state.invitations.contains_key(&invite.id) {
            return Err(invalid_request());
        }
        state.invitations.insert(
            invite.id,
            InvitationRecord {
                secret_hash: hash(invite.secret.as_ref()),
                endpoint: invite.endpoint,
                expires_at: invite.expires_at,
                consumed: false,
            },
        );
        Ok(())
    }

    pub fn request_pairing(
        &self,
        invite: PairingInvitation,
        device_name: String,
    ) -> Result<PendingPairing, ControlError> {
        let name = device_name.trim();
        if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
            return Err(invalid_request());
        }
        if invite.host_id != self.host_id || hash(&invite.ca_der) != self.ca_hash {
            return Err(unauthorized());
        }
        let now = SystemTime::now();
        let mut state = self.state.lock().map_err(|_| vault_unavailable())?;
        state.pending.retain(|_, pending| pending.expires_at > now);
        if state.pending.len() >= 32 {
            return Err(invalid_request());
        }
        let record = state
            .invitations
            .get_mut(&invite.id)
            .ok_or_else(unauthorized)?;
        if record.consumed
            || record.expires_at <= now
            || record.expires_at != invite.expires_at
            || record.endpoint != invite.endpoint
            || !bool::from(record.secret_hash.ct_eq(&hash(invite.secret.as_ref())))
        {
            return Err(unauthorized());
        }
        // Consume before approval: concurrent requests cannot create two owners
        // and a lost response never turns the invitation into a recovery token.
        record.consumed = true;
        let pending = PendingPairing {
            id: Uuid::new_v4(),
            device_name: name.into(),
            expires_at: record.expires_at,
        };
        state.pending.insert(pending.id, pending.clone());
        Ok(pending)
    }

    pub fn approve_pairing(
        &self,
        id: Uuid,
        grants: Grants,
    ) -> Result<NativeCredential, ControlError> {
        let mut state = self.state.lock().map_err(|_| vault_unavailable())?;
        let pending = state.pending.remove(&id).ok_or_else(unauthorized)?;
        if pending.expires_at <= SystemTime::now() || (!grants.control && !grants.administration) {
            return Err(unauthorized());
        }
        let credential = NativeCredential {
            device_id: Uuid::new_v4(),
            secret: random_secret()?,
        };
        let _guard = TRANSACTION.lock().map_err(|_| vault_unavailable())?;
        let mut approvals = self.read_approvals()?;
        if approvals.devices.len() >= 32 {
            return Err(invalid_request());
        }
        let id = credential.device_id.to_string();
        approvals.cleanup = Some(id.clone());
        self.persist(&approvals)?;
        let record = DeviceRecord {
            name: pending.device_name,
            secret_hash: hash(credential.secret()),
            grants,
        };
        let bytes = Zeroizing::new(serde_json::to_vec(&record).map_err(|_| vault_unavailable())?);
        self.store.write(&self.device_key(&id), &bytes)?;
        approvals.devices.push(id);
        approvals.cleanup = None;
        self.persist(&approvals)?;
        Ok(credential)
    }

    pub fn revoke_device(&self, id: Uuid) -> Result<(), ControlError> {
        let _guard = TRANSACTION.lock().map_err(|_| vault_unavailable())?;
        let mut approvals = self.read_approvals()?;
        let id = id.to_string();
        approvals.devices.retain(|device| device != &id);
        approvals.cleanup = Some(id);
        self.persist(&approvals)?;
        // Membership is durably removed before any fallible record cleanup.
        self.read_approvals()?;
        Ok(())
    }

    pub fn authenticate(&self, id: Uuid, secret: &[u8]) -> Result<Grants, ControlError> {
        if secret.len() != 32 {
            return Err(unauthorized());
        }
        let _guard = TRANSACTION.lock().map_err(|_| vault_unavailable())?;
        let approvals = self.read_approvals()?;
        let id = id.to_string();
        if !approvals.devices.contains(&id) {
            return Err(unauthorized());
        }
        let device = self.read_device(&id)?;
        if !bool::from(device.secret_hash.ct_eq(&hash(secret))) {
            return Err(unauthorized());
        }
        Ok(device.grants)
    }

    pub fn paired_devices(&self) -> Result<Vec<PairedDevice>, ControlError> {
        let _guard = TRANSACTION.lock().map_err(|_| vault_unavailable())?;
        self.read_approvals()?
            .devices
            .into_iter()
            .map(|id| {
                let record = self.read_device(&id)?;
                Ok(PairedDevice {
                    id: Uuid::parse_str(&id).map_err(|_| vault_unavailable())?,
                    name: record.name,
                    grants: record.grants,
                })
            })
            .collect()
    }
    pub fn pending_pairings(&self) -> Result<Vec<PendingPairing>, ControlError> {
        let mut state = self.state.lock().map_err(|_| vault_unavailable())?;
        state
            .pending
            .retain(|_, pending| pending.expires_at > SystemTime::now());
        Ok(state.pending.values().cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::{identity::load_or_create_identity, secrets::tests::MemoryStore};
    use std::sync::{Arc, Barrier};

    fn fixture() -> (Arc<MemoryStore>, HostIdentity, PairingService) {
        let store = Arc::new(MemoryStore::default());
        let identity = load_or_create_identity(store.as_ref()).unwrap();
        let service = PairingService::new(&identity, store.clone()).unwrap();
        (store, identity, service)
    }
    fn invite(identity: &HostIdentity, service: &PairingService) -> PairingInvitation {
        let invite = create_invitation(
            identity,
            "192.168.1.20:45123".parse().unwrap(),
            SystemTime::now(),
        )
        .unwrap();
        service.register_invitation(&invite).unwrap();
        invite
    }
    fn pair(identity: &HostIdentity, service: &PairingService) -> NativeCredential {
        let request = service
            .request_pairing(invite(identity, service), "Phone".into())
            .unwrap();
        service
            .approve_pairing(
                request.id,
                Grants {
                    control: true,
                    administration: false,
                },
            )
            .unwrap()
    }

    #[test]
    fn invitation_expires_after_300_seconds() {
        let (_, identity, _) = fixture();
        let now = SystemTime::now();
        let invite =
            create_invitation(&identity, "192.168.1.20:45123".parse().unwrap(), now).unwrap();
        assert_eq!(invite.expires_at, now + Duration::from_secs(300));
    }

    #[test]
    fn concurrent_consume_issues_once() {
        let (_, identity, service) = fixture();
        let invite = invite(&identity, &service);
        let service = Arc::new(service);
        let barrier = Arc::new(Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let (service, invite, barrier) = (service.clone(), invite.clone(), barrier.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    service
                        .request_pairing(invite, "Phone".into())
                        .and_then(|pending| {
                            service.approve_pairing(
                                pending.id,
                                Grants {
                                    control: true,
                                    administration: false,
                                },
                            )
                        })
                })
            })
            .collect();
        assert_eq!(
            handles
                .into_iter()
                .filter_map(|h| h.join().unwrap().ok())
                .count(),
            1
        );
    }

    #[test]
    fn revocation_is_device_scoped() {
        let (store, identity, service) = fixture();
        let first = pair(&identity, &service);
        let second = pair(&identity, &service);
        service.revoke_device(first.device_id()).unwrap();
        let reloaded = PairingService::new(&identity, store).unwrap();
        assert!(reloaded
            .authenticate(first.device_id(), first.secret())
            .is_err());
        assert_eq!(
            reloaded
                .authenticate(second.device_id(), second.secret())
                .unwrap(),
            Grants {
                control: true,
                administration: false
            }
        );
    }

    #[test]
    fn lost_issuance_requires_new_pairing() {
        let (store, identity, service) = fixture();
        let invite = invite(&identity, &service);
        let pending = service
            .request_pairing(invite.clone(), "Phone".into())
            .unwrap();
        let credential = service
            .approve_pairing(
                pending.id,
                Grants {
                    control: true,
                    administration: false,
                },
            )
            .unwrap();
        let raw = credential.secret().to_vec();
        drop(credential);
        assert!(service
            .approve_pairing(
                pending.id,
                Grants {
                    control: true,
                    administration: false
                }
            )
            .is_err());
        assert!(service
            .request_pairing(invite.clone(), "Phone".into())
            .is_err());
        for bytes in store.0.lock().unwrap().values() {
            assert!(!bytes.windows(raw.len()).any(|window| window == raw));
        }
        let restarted = PairingService::new(&identity, store).unwrap();
        assert!(restarted.request_pairing(invite, "Phone".into()).is_err());
        assert!(pair(&identity, &restarted).secret().len() == 32);
    }

    #[test]
    fn expired_invitation_cannot_be_consumed_or_approved() {
        let (_, identity, service) = fixture();
        let old = create_invitation(
            &identity,
            "192.168.1.20:45123".parse().unwrap(),
            SystemTime::now() - Duration::from_secs(300),
        )
        .unwrap();
        assert!(service.register_invitation(&old).is_err());
        assert!(service.request_pairing(old, "Phone".into()).is_err());
        let pending = service
            .request_pairing(invite(&identity, &service), "Phone".into())
            .unwrap();
        service
            .state
            .lock()
            .unwrap()
            .pending
            .get_mut(&pending.id)
            .unwrap()
            .expires_at = SystemTime::now();
        assert!(service
            .approve_pairing(
                pending.id,
                Grants {
                    control: true,
                    administration: false
                }
            )
            .is_err());
    }

    #[test]
    fn unknown_or_changed_invitation_and_public_endpoints_are_rejected() {
        let (_, identity, service) = fixture();
        for endpoint in [
            "0.0.0.0:45123",
            "8.8.8.8:45123",
            "127.0.0.1:45123",
            "192.168.1.20:0",
            "[::]:45123",
            "[::1]:45123",
        ] {
            assert!(
                create_invitation(&identity, endpoint.parse().unwrap(), SystemTime::now()).is_err()
            );
        }
        let mut forged = create_invitation(
            &identity,
            "192.168.1.20:45123".parse().unwrap(),
            SystemTime::now(),
        )
        .unwrap();
        assert!(service
            .request_pairing(forged.clone(), "Phone".into())
            .is_err());
        service.register_invitation(&forged).unwrap();
        forged.secret[0] ^= 1;
        assert!(service.request_pairing(forged, "Phone".into()).is_err());
    }

    #[test]
    fn ca_replacement_requires_new_pairing_and_debug_is_redacted() {
        let (store, identity, service) = fixture();
        let invitation = invite(&identity, &service);
        let text = invitation.encode().unwrap();
        let credential = pair(&identity, &service);
        let debug = format!("{invitation:?} {credential:?} {identity:?}");
        assert!(!debug.contains(text.as_str()));
        assert!(!debug.contains(&base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            credential.secret()
        )));
        // Preserve the host ID while replacing only its CA/key pair; trusting a
        // host ID alone must not revive approvals created under the old anchor.
        let replacement_store = MemoryStore::default();
        load_or_create_identity(&replacement_store).unwrap();
        {
            let mut values = replacement_store.0.lock().unwrap();
            let bytes = values.values_mut().next().unwrap();
            let mut record: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            record["id"] = serde_json::json!(identity.id());
            record["leaf_expires_at"] = serde_json::to_value(SystemTime::UNIX_EPOCH).unwrap();
            *bytes = serde_json::to_vec(&record).unwrap();
        }
        let replacement = load_or_create_identity(&replacement_store).unwrap();
        assert_eq!(replacement.id(), identity.id());
        assert_ne!(replacement.ca_der(), identity.ca_der());
        let replaced = PairingService::new(&replacement, store).unwrap();
        assert!(replaced
            .authenticate(credential.device_id(), credential.secret())
            .is_err());
        assert!(replaced
            .request_pairing(invitation, "Phone".into())
            .is_err());
    }

    #[test]
    fn maximum_devices_fit_native_vault_blob_budget() {
        let (store, identity, service) = fixture();
        for _ in 0..32 {
            let pending = service
                .request_pairing(invite(&identity, &service), "x".repeat(128))
                .unwrap();
            service
                .approve_pairing(
                    pending.id,
                    Grants {
                        control: true,
                        administration: true,
                    },
                )
                .unwrap();
        }
        assert_eq!(service.paired_devices().unwrap().len(), 32);
        for bytes in store.0.lock().unwrap().values() {
            assert!(
                bytes.len() <= 2560,
                "protected record exceeds native blob budget"
            );
        }
    }

    struct FaultStore {
        memory: MemoryStore,
        fail_write: Mutex<Option<usize>>,
        fail_delete: std::sync::atomic::AtomicBool,
    }
    impl SecretStore for FaultStore {
        fn read(&self, key: &str) -> Result<Option<Vec<u8>>, ControlError> {
            self.memory.read(key)
        }
        fn write(&self, key: &str, bytes: &[u8]) -> Result<(), ControlError> {
            let mut remaining = self.fail_write.lock().unwrap();
            if let Some(count) = remaining.as_mut() {
                if *count == 0 {
                    *remaining = None;
                    return Err(vault_unavailable());
                }
                *count -= 1;
            }
            self.memory.write(key, bytes)
        }
        fn delete(&self, key: &str) -> Result<(), ControlError> {
            if self.fail_delete.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(vault_unavailable());
            }
            self.memory.delete(key)
        }
    }
    fn fault_fixture() -> (Arc<FaultStore>, HostIdentity, PairingService) {
        let store = Arc::new(FaultStore {
            memory: MemoryStore::default(),
            fail_write: Mutex::new(None),
            fail_delete: std::sync::atomic::AtomicBool::new(false),
        });
        let identity = load_or_create_identity(store.as_ref()).unwrap();
        let service = PairingService::new(&identity, store.clone()).unwrap();
        (store, identity, service)
    }

    #[test]
    fn partial_approval_never_authorizes_and_orphans_are_recovered() {
        for fail_after in 0..3 {
            let (store, identity, service) = fault_fixture();
            let pending = service
                .request_pairing(invite(&identity, &service), "Phone".into())
                .unwrap();
            *store.fail_write.lock().unwrap() = Some(fail_after);
            assert!(service
                .approve_pairing(
                    pending.id,
                    Grants {
                        control: true,
                        administration: false
                    }
                )
                .is_err());
            let recovered = PairingService::new(&identity, store.clone()).unwrap();
            assert!(recovered.paired_devices().unwrap().is_empty());
            // Only the identity and optional empty membership index remain.
            assert!(store.memory.0.lock().unwrap().len() <= 2);
        }
    }

    #[test]
    fn revocation_survives_cleanup_failure_and_reload() {
        let (store, identity, service) = fault_fixture();
        let credential = pair(&identity, &service);
        store
            .fail_delete
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(service.revoke_device(credential.device_id()).is_err());
        assert!(service
            .authenticate(credential.device_id(), credential.secret())
            .is_err());
        store
            .fail_delete
            .store(false, std::sync::atomic::Ordering::SeqCst);
        let recovered = PairingService::new(&identity, store.clone()).unwrap();
        assert!(recovered
            .authenticate(credential.device_id(), credential.secret())
            .is_err());
        assert!(recovered.paired_devices().unwrap().is_empty());
        assert_eq!(store.memory.0.lock().unwrap().len(), 2);
    }

    #[test]
    fn pairing_debug_logs_never_include_invitation_or_device_secrets() {
        #[derive(Clone, Default)]
        struct Capture(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Capture {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let (_, identity, service) = fixture();
        let invitation = invite(&identity, &service);
        let credential = pair(&identity, &service);
        let capture = Capture::default();
        let writer = capture.clone();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(?invitation, ?credential, ?identity, "Pairing objects");
        });
        let logs = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
        assert!(logs.contains("REDACTED"));
        assert!(!logs.contains(invitation.encode().unwrap().as_str()));
        assert!(!logs.contains(&URL_SAFE_NO_PAD.encode(invitation.secret.as_ref())));
        assert!(!logs.contains(&URL_SAFE_NO_PAD.encode(credential.secret())));
        assert!(!logs.contains(&format!("{:?}", credential.secret())));
    }

    #[test]
    fn qr_contains_only_a_short_lived_invitation_not_a_device_credential() {
        let (_, identity, service) = fixture();
        let invitation = invite(&identity, &service);
        let payload: serde_json::Value =
            serde_json::from_str(&invitation.encode().unwrap()).unwrap();
        assert_eq!(payload["hostId"], identity.id());
        assert_eq!(
            URL_SAFE_NO_PAD
                .decode(payload["secret"].as_str().unwrap())
                .unwrap()
                .len(),
            32
        );
        assert!(invitation.qr_svg().unwrap().starts_with("<?xml"));
        assert_eq!(service.paired_devices().unwrap().len(), 0);
    }

    // Test-only reconstruction from emitted data, not a transport decoder.
    fn reconstruct_encoded_invitation(encoded: &str) -> PairingInvitation {
        let payload: serde_json::Value = serde_json::from_str(encoded).unwrap();
        let seconds = payload["expiresAt"]["unixSeconds"]
            .as_u64()
            .expect("expiry must encode integer Unix seconds");
        let nanos = payload["expiresAt"]["nanoseconds"]
            .as_u64()
            .expect("expiry must encode integer nanoseconds");
        assert!(seconds <= 9_007_199_254_740_991);
        assert!(nanos < 1_000_000_000);
        PairingInvitation {
            id: Uuid::parse_str(payload["invitationId"].as_str().unwrap()).unwrap(),
            host_id: payload["hostId"].as_str().unwrap().into(),
            ca_der: URL_SAFE_NO_PAD
                .decode(payload["ca"].as_str().unwrap())
                .unwrap(),
            endpoint: payload["endpoint"].as_str().unwrap().parse().unwrap(),
            secret: Zeroizing::new(
                URL_SAFE_NO_PAD
                    .decode(payload["secret"].as_str().unwrap())
                    .unwrap()
                    .try_into()
                    .unwrap_or_else(|_| panic!("invitation secret must contain 32 bytes")),
            ),
            expires_at: SystemTime::UNIX_EPOCH
                .checked_add(Duration::new(seconds, nanos as u32))
                .unwrap(),
        }
    }

    #[test]
    fn encoded_fractional_invitation_round_trip_preserves_deadline_and_one_use() {
        let (_, identity, service) = fixture();
        let seconds = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 1;
        let now = SystemTime::UNIX_EPOCH + Duration::new(seconds, 123_456_789);
        let invitation =
            create_invitation(&identity, "192.168.1.20:45123".parse().unwrap(), now).unwrap();
        assert_eq!(invitation.expires_at, now + Duration::from_secs(300));
        service.register_invitation(&invitation).unwrap();
        let encoded = invitation.encode().unwrap();
        let decoded = reconstruct_encoded_invitation(&encoded);
        assert_eq!(decoded.expires_at, now + Duration::from_secs(300));
        let pending = service.request_pairing(decoded, "Phone".into()).unwrap();
        assert_eq!(pending.expires_at, now + Duration::from_secs(300));
        let grants = Grants {
            control: true,
            administration: false,
        };
        let credential = service.approve_pairing(pending.id, grants).unwrap();
        assert_eq!(
            service
                .authenticate(credential.device_id(), credential.secret())
                .unwrap(),
            grants
        );
        assert!(service
            .request_pairing(reconstruct_encoded_invitation(&encoded), "Phone".into())
            .is_err());
    }

    #[test]
    fn encoded_fractional_invitation_cannot_extend_expired_host_deadline() {
        let (_, identity, service) = fixture();
        let seconds = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 301;
        let now = SystemTime::UNIX_EPOCH + Duration::new(seconds, 987_654_321);
        let invitation =
            create_invitation(&identity, "192.168.1.20:45123".parse().unwrap(), now).unwrap();
        let encoded = invitation.encode().unwrap();
        let decoded = reconstruct_encoded_invitation(&encoded);
        assert_eq!(decoded.expires_at, now + Duration::from_secs(300));
        assert!(service.register_invitation(&decoded).is_err());
        // Simulate a previously registered record surviving until its deadline,
        // without sleeps or a new production clock API. Every field comes from
        // the emitted payload, including the exact fractional expiration.
        service.state.lock().unwrap().invitations.insert(
            decoded.id,
            InvitationRecord {
                secret_hash: hash(decoded.secret.as_ref()),
                endpoint: decoded.endpoint,
                expires_at: decoded.expires_at,
                consumed: false,
            },
        );
        assert!(service.request_pairing(decoded, "Phone".into()).is_err());
        assert!(service.pending_pairings().unwrap().is_empty());
    }
}
