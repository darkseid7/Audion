//! Authenticated command admission and native coordinator ticket ownership.
use super::{
    pairing::PairingService,
    protocol::{CommandEnvelope, ControlError, ControlErrorCode, ExecutionResult},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::watch;
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CoordinatorLease {
    pub lease_id: Uuid,
    pub host_epoch: Uuid,
}
pub type ExecutionTicket = Uuid;
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostDispatch {
    pub ticket: ExecutionTicket,
    pub envelope: CommandEnvelope,
}
pub struct AuthoritativeWindow {
    label: String,
    dispatch: Arc<dyn Fn(HostDispatch) -> Result<(), ControlError> + Send + Sync>,
}
impl AuthoritativeWindow {
    pub(crate) fn new(
        label: String,
        dispatch: Arc<dyn Fn(HostDispatch) -> Result<(), ControlError> + Send + Sync>,
    ) -> Self {
        Self { label, dispatch }
    }
}
pub struct AuthenticatedDevice {
    pub(super) id: Uuid,
    pub(super) secret: Zeroizing<Vec<u8>>,
}
impl AuthenticatedDevice {
    pub fn authenticate(
        pairing: &PairingService,
        id: Uuid,
        secret: &[u8],
    ) -> Result<Self, ControlError> {
        pairing.authenticate(id, secret)?;
        Ok(Self {
            id,
            secret: Zeroizing::new(secret.to_vec()),
        })
    }
}
type Outcome = Result<ExecutionResult, ControlError>;
struct Entry {
    fingerprint: [u8; 32],
    ticket: ExecutionTicket,
    result: watch::Sender<Option<Outcome>>,
    completed: Option<Instant>,
    pinned: bool,
    pending: Option<(AuthenticatedDevice, CommandEnvelope)>,
    claimed: bool,
    executing: bool,
}
pub(super) struct Coordinator {
    window: AuthoritativeWindow,
    lease: CoordinatorLease,
    pub(super) ready: bool,
}
#[derive(Default)]
pub(super) struct State {
    pub(super) events: super::events::EventState,
    coordinator: Option<Coordinator>,
    ledger: HashMap<(Uuid, String), Entry>,
}
impl State {
    pub(super) fn coordinator_ready(&self) -> bool {
        self.coordinator.as_ref().is_some_and(|c| c.ready)
    }
}
pub struct CommandService {
    stopping: AtomicBool,
    pub(super) stopped: watch::Sender<bool>,
    pub(super) polls: Mutex<HashSet<Uuid>>,
    pub(super) pairing: Arc<PairingService>,
    pub(super) host_id: String,
    clock: Arc<dyn Fn() -> Instant + Send + Sync>,
    pub(super) state: Mutex<State>,
}
pub(crate) fn error(code: ControlErrorCode) -> ControlError {
    ControlError {
        retryable: matches!(
            code,
            ControlErrorCode::Busy | ControlErrorCode::HostNotReady | ControlErrorCode::RateLimited
        ),
        message: format!("Controller operation rejected: {code:?}"),
        code,
    }
}
fn invalidate(state: &mut State, now: Instant) {
    state.coordinator = None;
    state.events.invalidate();
    for entry in state.ledger.values_mut() {
        if entry.completed.is_none() {
            entry
                .result
                .send_replace(Some(Err(error(ControlErrorCode::OutcomeUnknown))));
            entry.completed = Some(now);
        }
        entry.executing = false;
        entry.pending = None;
    }
}
pub(super) fn current<'a>(
    state: &'a mut State,
    window: &str,
    lease: &CoordinatorLease,
) -> Result<&'a mut Coordinator, ControlError> {
    state
        .coordinator
        .as_mut()
        .filter(|c| c.window.label == window && c.lease == *lease)
        .ok_or_else(|| error(ControlErrorCode::Unauthorized))
}
impl CommandService {
    /// Exit never waits for a command/credential-store lock or an admitted effect.
    pub fn stop(&self) {
        self.stopping.store(true, Ordering::Release);
        self.stopped.send_replace(true);
    }
    pub(super) fn ensure_running(&self) -> Result<(), ControlError> {
        if self.stopping.load(Ordering::Acquire) {
            Err(error(ControlErrorCode::HostNotReady))
        } else {
            Ok(())
        }
    }
    pub fn claim(
        &self,
        window: &str,
        lease: &CoordinatorLease,
        ticket: ExecutionTicket,
    ) -> Result<CommandEnvelope, ControlError> {
        self.ensure_running()?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| error(ControlErrorCode::HostNotReady))?;
        self.ensure_running()?;
        if !current(&mut state, window, lease)?.ready {
            return Err(error(ControlErrorCode::HostNotReady));
        }
        let entry = state
            .ledger
            .values_mut()
            .find(|entry| entry.ticket == ticket && entry.completed.is_none() && !entry.claimed)
            .ok_or_else(|| error(ControlErrorCode::Unauthorized))?;
        let (device, envelope) = entry
            .pending
            .take()
            .ok_or_else(|| error(ControlErrorCode::Unauthorized))?;
        let authority = self
            .pairing
            .authenticate(device.id, &device.secret)
            .and_then(|grants| {
                if !grants.control {
                    Err(error(ControlErrorCode::PermissionRequired))
                } else if envelope.preconditions.host_epoch != lease.host_epoch.to_string() {
                    Err(error(ControlErrorCode::ResyncRequired))
                } else {
                    Ok(())
                }
            });
        if let Err(failure) = authority {
            entry.result.send_replace(Some(Err(failure.clone())));
            entry.completed = Some((self.clock)());
            return Err(failure);
        }
        self.ensure_running()?;
        entry.claimed = true;
        entry.executing = true;
        Ok(envelope)
    }
    pub fn new(pairing: Arc<PairingService>, host_id: String) -> Self {
        Self {
            pairing,
            host_id,
            stopping: AtomicBool::new(false),
            stopped: watch::channel(false).0,
            polls: Mutex::new(HashSet::new()),
            clock: Arc::new(Instant::now),
            state: Mutex::new(State::default()),
        }
    }
    pub fn register_coordinator(
        &self,
        window: AuthoritativeWindow,
    ) -> Result<CoordinatorLease, ControlError> {
        self.ensure_running()?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| error(ControlErrorCode::HostNotReady))?;
        self.ensure_running()?;
        invalidate(&mut state, (self.clock)());
        let lease = CoordinatorLease {
            lease_id: Uuid::new_v4(),
            host_epoch: Uuid::new_v4(),
        };
        state.coordinator = Some(Coordinator {
            window,
            lease: lease.clone(),
            ready: false,
        });
        Ok(lease)
    }
    pub fn ready(&self, window: &str, lease: &CoordinatorLease) -> Result<(), ControlError> {
        self.ensure_running()?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| error(ControlErrorCode::HostNotReady))?;
        self.ensure_running()?;
        current(&mut state, window, lease)?;
        if state.events.snapshot.is_none() {
            return Err(error(ControlErrorCode::HostNotReady));
        }
        current(&mut state, window, lease)?.ready = true;
        state.events.wake();
        Ok(())
    }
    pub fn release(&self, window: &str, lease: &CoordinatorLease) -> Result<(), ControlError> {
        self.ensure_running()?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| error(ControlErrorCode::HostNotReady))?;
        self.ensure_running()?;
        current(&mut state, window, lease)?;
        invalidate(&mut state, (self.clock)());
        Ok(())
    }
    pub fn invalidate_window(&self, window: &str) {
        if self.ensure_running().is_err() {
            return;
        }
        if let Ok(mut state) = self.state.lock() {
            if state
                .coordinator
                .as_ref()
                .is_some_and(|c| c.window.label == window)
            {
                invalidate(&mut state, (self.clock)());
            }
        }
    }
    pub fn host_epoch(&self) -> Result<Uuid, ControlError> {
        self.ensure_running()?;
        self.state
            .lock()
            .map_err(|_| error(ControlErrorCode::HostNotReady))?
            .coordinator
            .as_ref()
            .filter(|c| c.ready && self.ensure_running().is_ok())
            .map(|c| c.lease.host_epoch)
            .ok_or_else(|| error(ControlErrorCode::HostNotReady))
    }
    pub fn complete(
        &self,
        window: &str,
        lease: &CoordinatorLease,
        ticket: ExecutionTicket,
        mut result: ExecutionResult,
    ) -> Result<(), ControlError> {
        self.ensure_running()?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| error(ControlErrorCode::HostNotReady))?;
        self.ensure_running()?;
        if !current(&mut state, window, lease)?.ready {
            return Err(error(ControlErrorCode::HostNotReady));
        }
        let native_revision = state
            .events
            .snapshot
            .as_ref()
            .ok_or_else(|| error(ControlErrorCode::HostNotReady))?
            .revision;
        match &mut result {
            ExecutionResult::Applied { revision }
            | ExecutionResult::Accepted { revision, .. }
            | ExecutionResult::Failed { revision, .. }
            | ExecutionResult::Superseded { revision, .. } => *revision = native_revision,
        }
        let entry = state
            .ledger
            .values_mut()
            .find(|entry| entry.ticket == ticket && entry.claimed && entry.executing)
            .ok_or_else(|| error(ControlErrorCode::Unauthorized))?;
        entry.executing = false;
        // Revocation can end authorization, not undo a claimed domain effect.
        // Its slot remains occupied until completion (or a drained new epoch).
        if entry.completed.is_some() {
            return Err(error(ControlErrorCode::Unauthorized));
        }
        entry.pinned = matches!(result, ExecutionResult::Accepted { .. });
        entry.result.send_replace(Some(Ok(result)));
        entry.completed = Some((self.clock)());
        Ok(())
    }
    pub async fn submit(&self, device: &AuthenticatedDevice, envelope: CommandEnvelope) -> Outcome {
        self.ensure_running()?;
        let mut receiver = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| error(ControlErrorCode::HostNotReady))?;
            self.ensure_running()?;
            let grants = self.pairing.authenticate(device.id, &device.secret)?;
            self.ensure_running()?;
            if !grants.control {
                return Err(error(ControlErrorCode::PermissionRequired));
            }
            let bytes = serde_json::to_vec(&envelope)
                .map_err(|_| error(ControlErrorCode::InvalidRequest))?;
            let fingerprint: [u8; 32] = Sha256::digest(bytes).into();
            let now = (self.clock)();
            state.ledger.retain(|_, entry| {
                entry.pinned
                    || entry.executing
                    || entry
                        .completed
                        .is_none_or(|at| now.duration_since(at) <= Duration::from_secs(300))
            });
            let key = (device.id, envelope.request_id.clone());
            if let Some(entry) = state.ledger.get(&key) {
                if entry.fingerprint != fingerprint {
                    return Err(error(ControlErrorCode::InvalidRequest));
                }
                entry.result.subscribe()
            } else {
                let coordinator = state
                    .coordinator
                    .as_ref()
                    .filter(|c| c.ready && self.ensure_running().is_ok())
                    .ok_or_else(|| error(ControlErrorCode::HostNotReady))?;
                if envelope.preconditions.host_epoch != coordinator.lease.host_epoch.to_string() {
                    return Err(error(ControlErrorCode::ResyncRequired));
                }
                if state.ledger.len() >= 32 * 256
                    || state
                        .ledger
                        .iter()
                        .filter(|((id, _), _)| *id == device.id)
                        .count()
                        >= 256
                    || state
                        .ledger
                        .iter()
                        .any(|((id, _), entry)| *id == device.id && entry.completed.is_none())
                    || state
                        .ledger
                        .values()
                        .filter(|entry| entry.completed.is_none() || entry.executing)
                        .count()
                        >= 32
                {
                    return Err(error(ControlErrorCode::Busy));
                }
                let dispatch = coordinator.window.dispatch.clone();
                let ticket = Uuid::new_v4();
                let (result, receiver) = watch::channel(None);
                let authorization = AuthenticatedDevice {
                    id: device.id,
                    secret: Zeroizing::new(device.secret.to_vec()),
                };
                state.ledger.insert(
                    key.clone(),
                    Entry {
                        fingerprint,
                        ticket,
                        result,
                        completed: None,
                        pinned: false,
                        pending: Some((authorization, envelope.clone())),
                        claimed: false,
                        executing: false,
                    },
                );
                // Admission, authorization and dispatch share one lock with revoke/release.
                // Domain revision checks happen at execution in the existing TS lane.
                if let Err(failure) = dispatch(HostDispatch { ticket, envelope }) {
                    let entry = state.ledger.get_mut(&key).unwrap();
                    entry.result.send_replace(Some(Err(failure)));
                    entry.completed = Some(now);
                    entry.pending = None;
                }
                receiver
            }
        };
        let mut stopped = self.stopped.subscribe();
        loop {
            if self.ensure_running().is_err() {
                return Err(error(ControlErrorCode::OutcomeUnknown));
            }
            if let Some(result) = receiver.borrow().clone() {
                return result;
            }
            tokio::select! {
                _=stopped.changed()=>return Err(error(ControlErrorCode::OutcomeUnknown)),
                result=receiver.changed()=>{result.map_err(|_|error(ControlErrorCode::OutcomeUnknown))?;}
            }
        }
    }
    pub fn status(
        &self,
        device: &AuthenticatedDevice,
        request_id: &str,
    ) -> Result<Option<Outcome>, ControlError> {
        self.ensure_running()?;
        let state = self
            .state
            .lock()
            .map_err(|_| error(ControlErrorCode::HostNotReady))?;
        self.ensure_running()?;
        self.pairing.authenticate(device.id, &device.secret)?;
        self.ensure_running()?;
        state
            .ledger
            .get(&(device.id, request_id.into()))
            .map(|entry| entry.result.borrow().clone())
            .ok_or_else(|| error(ControlErrorCode::NotFound))
    }
    pub fn revoke(&self, id: Uuid) -> Result<(), ControlError> {
        self.ensure_running()?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| error(ControlErrorCode::HostNotReady))?;
        self.ensure_running()?;
        let revoked = self.pairing.revoke_device(id);
        state.events.wake();
        // Cancel native waiters even if protected-record cleanup failed after membership removal.
        for ((device, _), entry) in &mut state.ledger {
            if *device == id && entry.completed.is_none() {
                entry
                    .result
                    .send_replace(Some(Err(error(ControlErrorCode::Unauthorized))));
                entry.completed = Some((self.clock)());
                entry.pending = None;
            }
        }
        revoked
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::{
        identity::load_or_create_identity,
        pairing::{create_invitation, Grants},
        secrets::tests::MemoryStore,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn fixture() -> (
        CommandService,
        AuthenticatedDevice,
        Arc<AtomicUsize>,
        Arc<Mutex<Vec<HostDispatch>>>,
    ) {
        let store = Arc::new(MemoryStore::default());
        let identity = load_or_create_identity(store.as_ref()).unwrap();
        let pairing = Arc::new(PairingService::new(&identity, store).unwrap());
        let invitation = create_invitation(
            &identity,
            "192.168.1.2:9010".parse().unwrap(),
            std::time::SystemTime::now(),
        )
        .unwrap();
        pairing.register_invitation(&invitation).unwrap();
        let pending = pairing.request_pairing(invitation, "Phone".into()).unwrap();
        let credential = pairing
            .approve_pairing(
                pending.id,
                Grants {
                    control: true,
                    administration: false,
                },
            )
            .unwrap();
        let device = AuthenticatedDevice::authenticate(
            &pairing,
            credential.device_id(),
            credential.secret(),
        )
        .unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let dispatches = Arc::new(Mutex::new(Vec::new()));
        (
            CommandService::new(pairing, "host".into()),
            device,
            count,
            dispatches,
        )
    }
    fn prepare(
        service: &CommandService,
        count: Arc<AtomicUsize>,
        dispatches: Arc<Mutex<Vec<HostDispatch>>>,
    ) -> CoordinatorLease {
        service
            .register_coordinator(AuthoritativeWindow::new(
                "main".into(),
                Arc::new(move |dispatch| {
                    count.fetch_add(1, Ordering::SeqCst);
                    dispatches.lock().unwrap().push(dispatch);
                    Ok(())
                }),
            ))
            .unwrap()
    }
    fn envelope(lease: &CoordinatorLease, id: &str) -> CommandEnvelope {
        serde_json::from_value(serde_json::json!({"protocolVersion":1,"requestId":id,"preconditions":{"hostEpoch":lease.host_epoch.to_string(),"outputRevision":0},"intent":{"type":"pause"}})).unwrap()
    }
    #[test]
    fn exit_does_not_wait_for_command_or_vault_state_lock() {
        let (service, _, _, _) = fixture();
        let service = Arc::new(service);
        let held = service.state.lock().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = service.clone();
        let thread = std::thread::spawn(move || {
            worker.stop();
            tx.send(()).unwrap();
        });
        let returned = rx.recv_timeout(Duration::from_millis(100)).is_ok();
        drop(held);
        thread.join().unwrap();
        assert!(returned, "Exit waited for command/vault state");
    }
    #[test]
    fn ready_requires_initial_projection() {
        let (service, _, count, dispatches) = fixture();
        let lease = prepare(&service, count, dispatches);
        assert_eq!(
            service.ready("main", &lease).unwrap_err().code,
            ControlErrorCode::HostNotReady
        );
    }
    #[tokio::test]
    async fn ticket_claim_is_authoritative_single_use_and_window_scoped() {
        let (service, device, count, dispatches) = fixture();
        let lease = prepare(&service, count, dispatches.clone());
        service
            .publish(
                "main",
                &lease,
                super::super::protocol::HostUpdate::Projection {
                    snapshot: super::super::events::tests::projection(
                        &lease.host_epoch.to_string(),
                    ),
                },
            )
            .unwrap();
        service.ready("main", &lease).unwrap();
        let expected = envelope(&lease, "claim");
        let check = async {
            tokio::task::yield_now().await;
            let ticket = dispatches.lock().unwrap()[0].ticket;
            assert!(service.claim("other", &lease, ticket).is_err());
            assert!(service.claim("main", &lease, Uuid::new_v4()).is_err());
            assert!(service
                .complete(
                    "main",
                    &lease,
                    ticket,
                    ExecutionResult::Applied { revision: 1 }
                )
                .is_err());
            assert_eq!(service.claim("main", &lease, ticket).unwrap(), expected);
            assert!(service.claim("main", &lease, ticket).is_err());
            service
                .complete(
                    "main",
                    &lease,
                    ticket,
                    ExecutionResult::Applied { revision: 1 },
                )
                .unwrap();
        };
        let (result, ()) = tokio::join!(service.submit(&device, expected.clone()), check);
        result.unwrap();
    }
    #[tokio::test]
    async fn revoked_before_claim_never_yields_an_executable_envelope() {
        let (service, device, count, dispatches) = fixture();
        let lease = prepare(&service, count, dispatches.clone());
        service
            .publish(
                "main",
                &lease,
                super::super::protocol::HostUpdate::Projection {
                    snapshot: super::super::events::tests::projection(
                        &lease.host_epoch.to_string(),
                    ),
                },
            )
            .unwrap();
        service.ready("main", &lease).unwrap();
        let check = async {
            tokio::task::yield_now().await;
            let ticket = dispatches.lock().unwrap()[0].ticket;
            service.revoke(device.id).unwrap();
            assert!(service.claim("main", &lease, ticket).is_err());
        };
        let (result, ()) = tokio::join!(
            service.submit(&device, envelope(&lease, "revoked-claim")),
            check
        );
        assert_eq!(result.unwrap_err().code, ControlErrorCode::Unauthorized);
    }
    #[tokio::test]
    async fn revoked_claims_keep_global_execution_slots_until_domain_completion() {
        let store = Arc::new(MemoryStore::default());
        let identity = load_or_create_identity(store.as_ref()).unwrap();
        let pairing = Arc::new(PairingService::new(&identity, store).unwrap());
        let service = CommandService::new(pairing.clone(), "host".into());
        let dispatches = Arc::new(Mutex::new(Vec::<HostDispatch>::new()));
        let lease = prepare(&service, Arc::new(AtomicUsize::new(0)), dispatches.clone());
        service
            .publish(
                "main",
                &lease,
                super::super::protocol::HostUpdate::Projection {
                    snapshot: super::super::events::tests::projection(
                        &lease.host_epoch.to_string(),
                    ),
                },
            )
            .unwrap();
        service.ready("main", &lease).unwrap();
        let pair = || {
            let invitation = create_invitation(
                &identity,
                "192.168.1.2:9010".parse().unwrap(),
                std::time::SystemTime::now(),
            )
            .unwrap();
            pairing.register_invitation(&invitation).unwrap();
            let pending = pairing.request_pairing(invitation, "Phone".into()).unwrap();
            let credential = pairing
                .approve_pairing(
                    pending.id,
                    Grants {
                        control: true,
                        administration: false,
                    },
                )
                .unwrap();
            AuthenticatedDevice::authenticate(&pairing, credential.device_id(), credential.secret())
                .unwrap()
        };
        for _ in 0..32 {
            let device = pair();
            let revoke_after_claim = async {
                tokio::task::yield_now().await;
                let ticket = dispatches.lock().unwrap().last().unwrap().ticket;
                service.claim("main", &lease, ticket).unwrap();
                service.revoke(device.id).unwrap();
            };
            let (result, ()) = tokio::join!(
                service.submit(&device, envelope(&lease, "pending")),
                revoke_after_claim
            );
            assert_eq!(result.unwrap_err().code, ControlErrorCode::Unauthorized);
        }
        let device = pair();
        assert_eq!(
            tokio::time::timeout(
                Duration::from_secs(1),
                service.submit(&device, envelope(&lease, "overflow"))
            )
            .await
            .expect("overflow must not be dispatched")
            .unwrap_err()
            .code,
            ControlErrorCode::Busy
        );
        let first = dispatches.lock().unwrap()[0].ticket;
        assert!(service
            .complete(
                "main",
                &lease,
                first,
                ExecutionResult::Applied { revision: 1 }
            )
            .is_err());
        let complete = async {
            tokio::task::yield_now().await;
            let ticket = dispatches.lock().unwrap().last().unwrap().ticket;
            service.claim("main", &lease, ticket).unwrap();
            service
                .complete(
                    "main",
                    &lease,
                    ticket,
                    ExecutionResult::Applied { revision: 2 },
                )
                .unwrap();
        };
        let (result, ()) = tokio::join!(
            service.submit(&device, envelope(&lease, "after-completion")),
            complete
        );
        assert_eq!(result.unwrap(), ExecutionResult::Applied { revision: 0 });
    }
    #[tokio::test]
    async fn completion_revisions_are_native_for_every_result_variant() {
        let (service, device, count, dispatches) = fixture();
        let lease = prepare(&service, count, dispatches.clone());
        service
            .publish(
                "main",
                &lease,
                super::super::protocol::HostUpdate::Projection {
                    snapshot: super::super::events::tests::projection(
                        &lease.host_epoch.to_string(),
                    ),
                },
            )
            .unwrap();
        service.ready("main", &lease).unwrap();
        for (n,value) in [
            serde_json::json!({"status":"applied","revision":999}),
            serde_json::json!({"status":"accepted","jobId":"job","revision":999}),
            serde_json::json!({"status":"failed","revision":999,"error":{"code":"execution_failed","message":"failed","retryable":false},"partialEffects":["Previous output stopped"]}),
            serde_json::json!({"status":"superseded","revision":999,"error":{"code":"revision_conflict","message":"changed","retryable":false},"partialEffects":[]}),
        ].into_iter().enumerate() {
            let result:ExecutionResult=serde_json::from_value(value).unwrap();
            let complete=async { tokio::task::yield_now().await;let ticket=dispatches.lock().unwrap().last().unwrap().ticket;service.claim("main",&lease,ticket).unwrap();service.complete("main",&lease,ticket,result).unwrap(); };
            let (outcome,())=tokio::join!(service.submit(&device,envelope(&lease,&format!("variant-{n}"))),complete);
            assert_eq!(serde_json::to_value(outcome.unwrap()).unwrap()["revision"],0);
        }
    }
    #[tokio::test]
    async fn duplicate_submits_execute_once() {
        let (service, device, count, dispatches) = fixture();
        let lease = prepare(&service, count.clone(), dispatches.clone());
        service
            .publish(
                "main",
                &lease,
                super::super::protocol::HostUpdate::Projection {
                    snapshot: super::super::events::tests::projection(
                        &lease.host_epoch.to_string(),
                    ),
                },
            )
            .unwrap();
        service.ready("main", &lease).unwrap();
        let request = envelope(&lease, "same");
        let execution = async {
            tokio::task::yield_now().await;
            let ticket = dispatches.lock().unwrap()[0].ticket;
            service.claim("main", &lease, ticket).unwrap();
            service
                .complete(
                    "main",
                    &lease,
                    ticket,
                    ExecutionResult::Applied { revision: 1 },
                )
                .unwrap();
        };
        let (a, b, ()) = tokio::join!(
            service.submit(&device, request.clone()),
            service.submit(&device, request),
            execution
        );
        assert_eq!(a.unwrap(), b.unwrap());
        let execution_count = count.load(Ordering::SeqCst);
        assert_eq!(execution_count, 1);
    }
    #[tokio::test]
    async fn old_lease_cannot_complete_new_command() {
        let (service, device, count, dispatches) = fixture();
        let old = prepare(&service, count.clone(), dispatches.clone());
        let lease = prepare(&service, count, dispatches.clone());
        assert!(service.ready("main", &old).is_err());
        assert!(service.release("main", &old).is_err());
        service
            .publish(
                "main",
                &lease,
                super::super::protocol::HostUpdate::Projection {
                    snapshot: super::super::events::tests::projection(
                        &lease.host_epoch.to_string(),
                    ),
                },
            )
            .unwrap();
        service.ready("main", &lease).unwrap();
        let execution = async {
            tokio::task::yield_now().await;
            let ticket = dispatches.lock().unwrap()[0].ticket;
            assert!(service
                .complete(
                    "main",
                    &old,
                    ticket,
                    ExecutionResult::Applied { revision: 1 }
                )
                .is_err());
            assert!(service
                .complete(
                    "other",
                    &lease,
                    ticket,
                    ExecutionResult::Applied { revision: 1 }
                )
                .is_err());
            service.claim("main", &lease, ticket).unwrap();
            service
                .complete(
                    "main",
                    &lease,
                    ticket,
                    ExecutionResult::Applied { revision: 2 },
                )
                .unwrap();
        };
        let (result, ()) =
            tokio::join!(service.submit(&device, envelope(&lease, "new")), execution);
        assert_eq!(result.unwrap(), ExecutionResult::Applied { revision: 0 });
    }
    #[tokio::test]
    async fn unknown_ticket_cannot_resolve_pending_request() {
        let (service, device, count, dispatches) = fixture();
        let lease = prepare(&service, count, dispatches.clone());
        service
            .publish(
                "main",
                &lease,
                super::super::protocol::HostUpdate::Projection {
                    snapshot: super::super::events::tests::projection(
                        &lease.host_epoch.to_string(),
                    ),
                },
            )
            .unwrap();
        service.ready("main", &lease).unwrap();
        let execution = async {
            tokio::task::yield_now().await;
            assert!(service
                .complete(
                    "main",
                    &lease,
                    Uuid::new_v4(),
                    ExecutionResult::Applied { revision: 99 }
                )
                .is_err());
            let ticket = dispatches.lock().unwrap()[0].ticket;
            service.claim("main", &lease, ticket).unwrap();
            service
                .complete(
                    "main",
                    &lease,
                    ticket,
                    ExecutionResult::Applied { revision: 1 },
                )
                .unwrap();
        };
        let (result, ()) =
            tokio::join!(service.submit(&device, envelope(&lease, "new")), execution);
        assert_eq!(result.unwrap(), ExecutionResult::Applied { revision: 0 });
    }
    #[tokio::test]
    async fn bounded_command_admission_returns_busy() {
        let (service, device, count, dispatches) = fixture();
        let lease = prepare(&service, count, dispatches);
        service
            .publish(
                "main",
                &lease,
                super::super::protocol::HostUpdate::Projection {
                    snapshot: super::super::events::tests::projection(
                        &lease.host_epoch.to_string(),
                    ),
                },
            )
            .unwrap();
        service.ready("main", &lease).unwrap();
        let check = async {
            tokio::task::yield_now().await;
            assert_eq!(
                service
                    .submit(&device, envelope(&lease, "second"))
                    .await
                    .unwrap_err()
                    .code,
                ControlErrorCode::Busy
            );
            service.release("main", &lease).unwrap();
        };
        let (result, ()) = tokio::join!(service.submit(&device, envelope(&lease, "first")), check);
        assert_eq!(result.unwrap_err().code, ControlErrorCode::OutcomeUnknown);
    }
    #[tokio::test]
    async fn unexpired_ledger_capacity_returns_busy() {
        let (mut service, device, count, dispatches) = fixture();
        let time = Arc::new(Mutex::new(std::time::Instant::now()));
        let clock = time.clone();
        service.clock = Arc::new(move || *clock.lock().unwrap());
        let lease = prepare(&service, count.clone(), dispatches.clone());
        service
            .publish(
                "main",
                &lease,
                super::super::protocol::HostUpdate::Projection {
                    snapshot: super::super::events::tests::projection(
                        &lease.host_epoch.to_string(),
                    ),
                },
            )
            .unwrap();
        service.ready("main", &lease).unwrap();
        for i in 0..256 {
            let complete = async {
                tokio::task::yield_now().await;
                let ticket = dispatches.lock().unwrap().last().unwrap().ticket;
                service.claim("main", &lease, ticket).unwrap();
                service
                    .complete(
                        "main",
                        &lease,
                        ticket,
                        ExecutionResult::Applied { revision: i },
                    )
                    .unwrap();
            };
            let (result, ()) = tokio::join!(
                service.submit(&device, envelope(&lease, &format!("request-{i}"))),
                complete
            );
            result.unwrap();
        }
        *time.lock().unwrap() += Duration::from_secs(300);
        assert_eq!(
            service
                .submit(&device, envelope(&lease, "overflow"))
                .await
                .unwrap_err()
                .code,
            ControlErrorCode::Busy
        );
        assert_eq!(
            service
                .submit(&device, envelope(&lease, "request-0"))
                .await
                .unwrap(),
            ExecutionResult::Applied { revision: 0 }
        );
        assert_eq!(count.load(Ordering::SeqCst), 256);
    }
    #[tokio::test]
    async fn request_id_payload_reuse_is_rejected() {
        let (service, device, count, dispatches) = fixture();
        let lease = prepare(&service, count, dispatches);
        service
            .publish(
                "main",
                &lease,
                super::super::protocol::HostUpdate::Projection {
                    snapshot: super::super::events::tests::projection(
                        &lease.host_epoch.to_string(),
                    ),
                },
            )
            .unwrap();
        service.ready("main", &lease).unwrap();
        let check = async {
            tokio::task::yield_now().await;
            let mut altered = envelope(&lease, "same");
            altered.intent = super::super::protocol::ApplicationIntent::Resume {};
            assert_eq!(
                service.submit(&device, altered).await.unwrap_err().code,
                ControlErrorCode::InvalidRequest
            );
            service.release("main", &lease).unwrap();
        };
        let _ = tokio::join!(service.submit(&device, envelope(&lease, "same")), check);
    }
    #[tokio::test]
    async fn missing_coordinator_and_revoked_device_fail_closed() {
        let (service, device, count, dispatches) = fixture();
        let lease = prepare(&service, count, dispatches);
        assert_eq!(
            service
                .submit(&device, envelope(&lease, "no-ready"))
                .await
                .unwrap_err()
                .code,
            ControlErrorCode::HostNotReady
        );
        service
            .publish(
                "main",
                &lease,
                super::super::protocol::HostUpdate::Projection {
                    snapshot: super::super::events::tests::projection(
                        &lease.host_epoch.to_string(),
                    ),
                },
            )
            .unwrap();
        service.ready("main", &lease).unwrap();
        service.revoke(device.id).unwrap();
        assert_eq!(
            service
                .submit(&device, envelope(&lease, "revoked"))
                .await
                .unwrap_err()
                .code,
            ControlErrorCode::Unauthorized
        );
    }
}
