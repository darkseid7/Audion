//! Native paired records, cancellation and bounded display cache. No domain owner.
use super::{
    client::{
        invalid_pairing, transport_error, CommandStatus, ControlGrants, Handshake, NativeImage, NativePairing,
        NativeTransport, PairReply,
    },
    mobile::{validate_invitation, NativeInvitation},
    protocol::*,
};
use crate::commands::controller::ControllerRequest;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::SystemTime,
};
use zeroize::{Zeroize, Zeroizing};

pub(crate) trait ControllerStore: Send + Sync + 'static {
    fn load(&self, host: &str) -> Result<Option<Zeroizing<Vec<u8>>>, ControlError>;
    fn save(&self, host: &str, bytes: &[u8]) -> Result<(), ControlError>;
    fn delete(&self, host: &str) -> Result<(), ControlError>;
}
fn generation<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let n = u64::deserialize(d)?;
    if n > 9_007_199_254_740_991 {
        return Err(serde::de::Error::custom("Invalid generation"));
    }
    Ok(n)
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Fence {
    pub scope_id: String,
    #[serde(deserialize_with = "generation")]
    pub generation: u64,
}
#[derive(Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum ConnectionRequest {
    BeginScope {},
    Connect { host_id: String, fence: Fence },
}
#[derive(Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub(crate) enum ConnectionReply {
    Scope { scope_id: String },
    Connected { snapshot: HostSnapshot, grants: ControlGrants },
}
#[derive(Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub(crate) enum ControllerReply {
    Query { result: QueryResult },
    Command { result: ExecutionResult },
    CommandStatus { result: CommandStatus },
    Handshake { result: Handshake },
    Poll { batch: EventBatch },
    Media { image: NativeImage },
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PairedRecord {
    version: u8,
    host_id: String,
    endpoint: String,
    ca: Vec<u8>,
    device_id: String,
    secret: Vec<u8>,
}
impl Drop for PairedRecord {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}
impl PairedRecord {
    fn pairing(&self, host: &str) -> Result<NativePairing, ControlError> {
        let device = uuid::Uuid::parse_str(&self.device_id).map_err(|_| invalid_pairing())?;
        if self.version != 1
            || self.host_id != host
            || device.is_nil()
            || device.to_string() != self.device_id
        {
            return Err(invalid_pairing());
        }
        NativePairing::new(
            self.host_id.clone(),
            self.endpoint.parse().map_err(|_| invalid_pairing())?,
            self.ca.clone(),
            device,
            self.secret
                .as_slice()
                .try_into()
                .map_err(|_| invalid_pairing())?,
        )
    }
}
#[derive(Clone)]
struct Cancellation(tokio::sync::watch::Sender<bool>);
impl Cancellation {
    fn new() -> Self {
        Self(tokio::sync::watch::channel(false).0)
    }
    fn cancel(&self) {
        self.0.send_replace(true);
    }
    async fn cancelled(&self) {
        let mut r = self.0.subscribe();
        if *r.borrow() {
            return;
        }
        let _ = r.changed().await;
    }
}
struct Active {
    host: String,
    device: String,
    epoch: String,
    transport: NativeTransport,
    poll: AtomicBool,
    library: AtomicU64,
}
struct PairAttempt {
    fence: Fence,
    input: Option<Zeroizing<String>>,
    invitation: Option<NativeInvitation>,
    exchanging: bool,
}
struct CacheEntry {
    key: String,
    host: String,
    image: NativeImage,
}
#[derive(Default)]
struct ImageCache {
    entries: VecDeque<CacheEntry>,
    bytes: usize,
}
impl ImageCache {
    fn remove_host(&mut self, host: &str) {
        self.entries.retain(|entry| entry.host != host);
        self.bytes = self.entries.iter().map(|e| e.image.bytes.len()).sum();
    }
    fn get(&mut self, key: &str) -> Option<NativeImage> {
        let at = self.entries.iter().position(|e| e.key == key)?;
        let entry = self.entries.remove(at)?;
        let image = entry.image.clone();
        self.entries.push_back(entry);
        Some(image)
    }
    fn insert(
        &mut self,
        key: String,
        host: String,
        image: NativeImage,
    ) -> Result<(), ControlError> {
        if image.bytes.is_empty() || image.bytes.len() > 5 * 1024 * 1024 {
            return Err(transport_error(ControlErrorCode::TooLarge));
        }
        if let Some(at) = self.entries.iter().position(|e| e.key == key) {
            let old = self.entries.remove(at).unwrap();
            self.bytes -= old.image.bytes.len();
        }
        while self.bytes + image.bytes.len() > 50 * 1024 * 1024 {
            let old = self.entries.pop_front().unwrap();
            self.bytes -= old.image.bytes.len();
        }
        self.bytes += image.bytes.len();
        self.entries.push_back(CacheEntry { key, host, image });
        Ok(())
    }
}
struct Inner {
    scope: String,
    generation: u64,
    cancel: Cancellation,
    active: Option<Arc<Active>>,
    pair: Option<PairAttempt>,
    cache: ImageCache,
}
impl Default for Inner {
    fn default() -> Self {
        Self {
            scope: String::new(),
            generation: 0,
            cancel: Cancellation::new(),
            active: None,
            pair: None,
            cache: ImageCache::default(),
        }
    }
}
type Connector =
    Arc<dyn Fn(NativePairing, bool) -> Result<NativeTransport, ControlError> + Send + Sync>;
pub(crate) struct NativeSession {
    inner: Mutex<Inner>,
    connector: Connector,
}
impl Default for NativeSession {
    fn default() -> Self {
        Self {
            inner: Mutex::default(),
            connector: Arc::new(|pairing, authenticated| {
                if authenticated {
                    NativeTransport::paired(pairing)
                } else {
                    NativeTransport::invitation(pairing)
                }
            }),
        }
    }
}
impl NativeSession {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Inner>, ControlError> {
        self.inner
            .lock()
            .map_err(|_| transport_error(ControlErrorCode::HostNotReady))
    }
    fn matches(inner: &Inner, f: &Fence) -> bool {
        !inner.scope.is_empty()
            && inner.scope == f.scope_id
            && inner.generation == f.generation
            && f.generation <= 9_007_199_254_740_991
    }
    fn check(&self, f: &Fence) -> Result<(), ControlError> {
        if !Self::matches(&*self.lock()?, f) {
            return Err(transport_error(ControlErrorCode::ResyncRequired));
        }
        Ok(())
    }
    fn transition_locked(inner: &mut Inner, f: &Fence) -> Result<Cancellation, ControlError> {
        if inner.scope != f.scope_id
            || inner.scope.is_empty()
            || f.generation <= inner.generation
            || f.generation > 9_007_199_254_740_991
        {
            return Err(transport_error(ControlErrorCode::ResyncRequired));
        }
        inner.cancel.cancel();
        inner.cancel = Cancellation::new();
        inner.generation = f.generation;
        inner.active = None;
        inner.pair = None;
        Ok(inner.cancel.clone())
    }
    fn transition(&self, f: &Fence) -> Result<Cancellation, ControlError> {
        Self::transition_locked(&mut *self.lock()?, f)
    }
    pub(crate) fn begin_scope(&self) -> Result<String, ControlError> {
        let mut inner = self.lock()?;
        inner.cancel.cancel();
        *inner = Inner::default();
        inner.scope = uuid::Uuid::new_v4().to_string();
        Ok(inner.scope.clone())
    }
    pub(crate) fn suspend(&self, f: &Fence) -> Result<(), ControlError> {
        self.transition(f).map(|_| ())
    }
    pub(crate) async fn connect(
        self: &Arc<Self>,
        store: Arc<dyn ControllerStore>,
        host: String,
        f: Fence,
    ) -> Result<(HostSnapshot, ControlGrants), ControlError> {
        uuid_host(&host)?;
        let cancel = self.transition(&f)?;
        let id = host.clone();
        let load = tokio::task::spawn_blocking(move || store.load(&id));
        let bytes=tokio::select!{_ = cancel.cancelled()=>return Err(transport_error(ControlErrorCode::ResyncRequired)), result=load=>result.map_err(|_|transport_error(ControlErrorCode::HostNotReady))??}.ok_or_else(||transport_error(ControlErrorCode::Unauthorized))?;
        self.check(&f)?;
        if bytes.len() > 16 * 1024 {
            return Err(invalid_pairing());
        }
        let record: PairedRecord = serde_json::from_slice(&bytes).map_err(|_| invalid_pairing())?;
        let device = record.device_id.clone();
        let transport = (self.connector)(record.pairing(&host)?, true)?;
        let result = async {
            let handshake = transport.handshake().await?;
            if handshake.protocol_version != 1 || handshake.host_id != host {
                return Err(transport_error(ControlErrorCode::Unsupported));
            }
            let QueryResult::Snapshot { snapshot } =
                transport.query(&ApplicationQuery::Snapshot {}).await?
            else {
                return Err(transport_error(ControlErrorCode::Unsupported));
            };
            if snapshot.host_id != host || snapshot.host_epoch != handshake.host_epoch {
                return Err(transport_error(ControlErrorCode::ResyncRequired));
            }
            Ok((snapshot, handshake.grants))
        };
        let (snapshot, grants) = tokio::select! {_ = cancel.cancelled()=>return Err(transport_error(ControlErrorCode::ResyncRequired)),result=result=>result?};
        let mut inner = self.lock()?;
        if !Self::matches(&inner, &f) {
            return Err(transport_error(ControlErrorCode::ResyncRequired));
        }
        inner.active = Some(Arc::new(Active {
            host,
            device,
            epoch: snapshot.host_epoch.clone(),
            transport,
            poll: AtomicBool::new(false),
            library: AtomicU64::new(snapshot.revisions.library_revision),
        }));
        Ok((snapshot, grants))
    }
    pub(crate) async fn forget(
        self: &Arc<Self>,
        store: Arc<dyn ControllerStore>,
        host: String,
        f: Fence,
    ) -> Result<(), ControlError> {
        uuid_host(&host)?;
        self.transition(&f)?;
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut inner = this.lock()?;
            if !Self::matches(&inner, &f) {
                return Err(transport_error(ControlErrorCode::ResyncRequired));
            }
            inner.cache.remove_host(&host);
            store.delete(&host)
        })
        .await
        .map_err(|_| transport_error(ControlErrorCode::HostNotReady))?
    }
    pub(crate) fn reserve_pairing(&self, f: &Fence) -> Result<(), ControlError> {
        let mut inner = self.lock()?;
        if inner.pair.is_some() {
            return Err(transport_error(ControlErrorCode::Busy));
        }
        Self::transition_locked(&mut inner, f)?;
        inner.pair = Some(PairAttempt {
            fence: f.clone(),
            input: None,
            invitation: None,
            exchanging: false,
        });
        Ok(())
    }
    pub(crate) fn stage_pairing(
        &self,
        f: &Fence,
        input: Option<Zeroizing<String>>,
    ) -> Result<super::mobile::PairingStatus, ControlError> {
        let mut inner = self.lock()?;
        if !Self::matches(&inner, f) {
            return Err(transport_error(ControlErrorCode::ResyncRequired));
        }
        let Some(attempt) = inner
            .pair
            .as_mut()
            .filter(|p| p.fence == *f && !p.exchanging)
        else {
            return Err(transport_error(ControlErrorCode::Busy));
        };
        let Some(input) = input else {
            inner.pair = None;
            return Ok(super::mobile::PairingStatus::Cancelled);
        };
        let invitation = match validate_invitation(&input, SystemTime::now()) {
            Ok(i) => i,
            Err(e) => {
                inner.pair = None;
                return Err(e);
            }
        };
        let host_id = invitation.host_id.clone();
        attempt.invitation = Some(invitation);
        attempt.input = Some(input);
        Ok(super::mobile::PairingStatus::InvitationReady { host_id })
    }
    pub(crate) async fn pair(
        self: &Arc<Self>,
        store: Arc<dyn ControllerStore>,
        f: Fence,
        name: String,
    ) -> Result<String, ControlError> {
        if name.is_empty() || name.len() > 256 {
            return Err(invalid_pairing());
        }
        let (input, invitation, cancel) = {
            let mut inner = self.lock()?;
            if !Self::matches(&inner, &f) {
                return Err(transport_error(ControlErrorCode::ResyncRequired));
            }
            let cancel = inner.cancel.clone();
            let attempt = inner
                .pair
                .as_mut()
                .filter(|p| p.fence == f && !p.exchanging)
                .ok_or_else(|| transport_error(ControlErrorCode::Busy))?;
            let input = attempt.input.take().ok_or_else(invalid_pairing)?;
            let invitation = attempt.invitation.take().ok_or_else(invalid_pairing)?;
            attempt.exchanging = true;
            (input, invitation, cancel)
        };
        let result = async {
            if invitation.expires_at <= SystemTime::now() {
                return Err(invalid_pairing());
            }
            let pairing = NativePairing::new(
                invitation.host_id.clone(),
                invitation.endpoint,
                invitation.ca_der.clone(),
                uuid::Uuid::nil(),
                [0; 32],
            )?;
            let transport = (self.connector)(pairing, false)?;
            let pending = transport.pair_start(&input, &name).await?;
            uuid_host(&pending.pending_id)?;
            loop {
                if invitation.expires_at <= SystemTime::now() {
                    return Err(invalid_pairing());
                }
                let reply = transport.pair_status(&input, &pending.pending_id).await?;
                if let PairReply::Approved { device_id, secret } = &reply {
                    if invitation.expires_at <= SystemTime::now() {
                        return Err(invalid_pairing());
                    }
                    let bytes = Zeroizing::new(
                        URL_SAFE_NO_PAD
                            .decode(secret)
                            .map_err(|_| invalid_pairing())?,
                    );
                    let record = PairedRecord {
                        version: 1,
                        host_id: invitation.host_id.clone(),
                        endpoint: invitation.endpoint.to_string(),
                        ca: invitation.ca_der.clone(),
                        device_id: device_id.clone(),
                        secret: bytes.to_vec(),
                    };
                    record.pairing(&invitation.host_id)?;
                    let encoded =
                        Zeroizing::new(serde_json::to_vec(&record).map_err(|_| invalid_pairing())?);
                    let host = invitation.host_id.clone();
                    let this = self.clone();
                    let fence = f.clone();
                    let store = store.clone();
                    let expires = invitation.expires_at;
                    tokio::task::spawn_blocking(move || {
                        let mut inner = this.lock()?;
                        if !Self::matches(&inner, &fence) || expires <= SystemTime::now() {
                            return Err(transport_error(ControlErrorCode::ResyncRequired));
                        }
                        store.save(&host, &encoded)?;
                        inner.cache.remove_host(&host);
                        Ok(())
                    })
                    .await
                    .map_err(|_| transport_error(ControlErrorCode::HostNotReady))??;
                    return Ok(invitation.host_id.clone());
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        };
        let result = tokio::select! {_ = cancel.cancelled()=>Err(transport_error(ControlErrorCode::ResyncRequired)),result=result=>result};
        let mut inner = self.lock()?;
        if Self::matches(&inner, &f) {
            inner.pair = None;
        }
        result
    }
    async fn validate_epoch(active: &Active) -> Result<Handshake, ControlError> {
        let h = active.transport.handshake().await?;
        if h.protocol_version != 1 {
            return Err(transport_error(ControlErrorCode::Unsupported));
        }
        if h.host_id != active.host || h.host_epoch != active.epoch {
            return Err(transport_error(ControlErrorCode::ResyncRequired));
        }
        Ok(h)
    }
    pub(crate) async fn request(
        &self,
        f: &Fence,
        request: ControllerRequest,
    ) -> Result<ControllerReply, ControlError> {
        let (active, cancel) = {
            let inner = self.lock()?;
            if !Self::matches(&inner, f) {
                return Err(transport_error(ControlErrorCode::ResyncRequired));
            }
            (
                inner
                    .active
                    .clone()
                    .ok_or_else(|| transport_error(ControlErrorCode::HostNotReady))?,
                inner.cancel.clone(),
            )
        };
        let operation = async {
            let reply = match request {
                ControllerRequest::Handshake {} => ControllerReply::Handshake {
                    result: Self::validate_epoch(&active).await?,
                },
                ControllerRequest::Query { query } => {
                    let result = active.transport.query(&query).await?;
                    Self::validate_epoch(&active).await?;
                    if let QueryResult::Snapshot { snapshot } = &result {
                        if snapshot.host_id != active.host || snapshot.host_epoch != active.epoch {
                            return Err(transport_error(ControlErrorCode::ResyncRequired));
                        }
                        active
                            .library
                            .fetch_max(snapshot.revisions.library_revision, Ordering::SeqCst);
                    }
                    ControllerReply::Query { result }
                }
                ControllerRequest::Command { envelope } => {
                    if envelope.preconditions.host_epoch != active.epoch {
                        return Err(transport_error(ControlErrorCode::ResyncRequired));
                    }
                    let result = active.transport.command(&envelope).await?;
                    Self::validate_epoch(&active).await?;
                    ControllerReply::Command { result }
                }
                ControllerRequest::CommandStatus { request_id } => {
                    let result = active.transport.command_status(&request_id).await?;
                    Self::validate_epoch(&active).await?;
                    ControllerReply::CommandStatus { result }
                }
                ControllerRequest::Poll { cursor } => {
                    if cursor.host_epoch != active.epoch {
                        return Err(transport_error(ControlErrorCode::ResyncRequired));
                    }
                    if active.poll.swap(true, Ordering::SeqCst) {
                        return Err(transport_error(ControlErrorCode::Busy));
                    }
                    struct Poll<'a>(&'a AtomicBool);
                    impl Drop for Poll<'_> {
                        fn drop(&mut self) {
                            self.0.store(false, Ordering::SeqCst);
                        }
                    }
                    let _guard = Poll(&active.poll);
                    let batch = active.transport.poll(&cursor).await?;
                    if batch.host_epoch != active.epoch
                        || batch.events.len() > 256
                        || batch.revision != cursor.revision + batch.events.len() as u64
                    {
                        return Err(transport_error(ControlErrorCode::ResyncRequired));
                    }
                    for (i, event) in batch.events.iter().enumerate() {
                        if event_revision(event) != cursor.revision + i as u64 + 1 {
                            return Err(transport_error(ControlErrorCode::ResyncRequired));
                        }
                        if let HostEvent::Library {
                            library_revision, ..
                        } = event
                        {
                            active
                                .library
                                .fetch_max(*library_revision, Ordering::SeqCst);
                        }
                    }
                    ControllerReply::Poll { batch }
                }
                ControllerRequest::Media { reference } => {
                    // Every cache read reauthenticates device and epoch. A fresh PC
                    // snapshot prevents a missed library event authorizing old bytes.
                    let QueryResult::Snapshot { snapshot } = active
                        .transport
                        .query(&ApplicationQuery::Snapshot {})
                        .await?
                    else {
                        return Err(transport_error(ControlErrorCode::Unsupported));
                    };
                    if snapshot.host_epoch != active.epoch || snapshot.host_id != active.host {
                        return Err(transport_error(ControlErrorCode::ResyncRequired));
                    }
                    if snapshot.revisions.library_revision != reference.revision {
                        return Err(transport_error(ControlErrorCode::RevisionConflict));
                    }
                    active
                        .library
                        .fetch_max(snapshot.revisions.library_revision, Ordering::SeqCst);
                    let key = format!(
                        "{}:{}:{}:{}:{}",
                        active.host,
                        active.device,
                        active.epoch,
                        reference.resource_id,
                        reference.revision
                    );
                    // No conditional resource protocol exists yet. Revalidate the
                    // actual resource even on cache hits; never offline-fallback.
                    let validated = active.transport.media(&reference).await?;
                    Self::validate_epoch(&active).await?;
                    if active.library.load(Ordering::SeqCst) != reference.revision {
                        return Err(transport_error(ControlErrorCode::RevisionConflict));
                    }
                    let mut inner = self.lock()?;
                    if !Self::matches(&inner, f) {
                        return Err(transport_error(ControlErrorCode::ResyncRequired));
                    }
                    let image = match inner.cache.get(&key) {
                        Some(cached)
                            if cached.mime == validated.mime && cached.bytes == validated.bytes =>
                        {
                            cached
                        }
                        _ => validated,
                    };
                    inner
                        .cache
                        .insert(key, active.host.clone(), image.clone())?;
                    ControllerReply::Media { image }
                }
            };
            self.check(f)?;
            Ok(reply)
        };
        let result = tokio::select! {_ = cancel.cancelled()=>Err(transport_error(ControlErrorCode::ResyncRequired)),result=operation=>result};
        if result.as_ref().err().is_some_and(|e| {
            matches!(
                e.code,
                ControlErrorCode::Unauthorized | ControlErrorCode::ResyncRequired
            )
        }) {
            let mut inner = self.lock()?;
            if Self::matches(&inner, f) {
                inner.cancel.cancel();
                inner.active = None;
                inner.cache.remove_host(&active.host);
            }
        }
        result
    }
}
fn uuid_host(id: &str) -> Result<(), ControlError> {
    let parsed = uuid::Uuid::parse_str(id).map_err(|_| invalid_pairing())?;
    if parsed.is_nil() || parsed.to_string() != id {
        return Err(invalid_pairing());
    }
    Ok(())
}
#[cfg(all(test, desktop))]
#[path = "native_session_tests.rs"]
mod tests;

fn event_revision(event: &HostEvent) -> u64 {
    match event {
        HostEvent::Playback { revision, .. }
        | HostEvent::Queue { revision, .. }
        | HostEvent::Library { revision, .. }
        | HostEvent::Output { revision, .. }
        | HostEvent::Settings { revision, .. }
        | HostEvent::Capabilities { revision, .. }
        | HostEvent::Job { revision, .. } => *revision,
    }
}
