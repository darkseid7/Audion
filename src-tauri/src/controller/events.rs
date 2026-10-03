//! Native snapshot/replay authority. All access shares CommandService's lease mutex.
use super::commands::{current, error, AuthenticatedDevice, CommandService, CoordinatorLease};
use super::protocol::*;
use std::{collections::VecDeque, time::Duration};
use tokio::sync::watch;
use uuid::Uuid;

pub(super) struct EventState {
    pub snapshot: Option<HostSnapshot>,
    history: VecDeque<HostEvent>,
    changed: watch::Sender<u64>,
}
impl Default for EventState {
    fn default() -> Self {
        Self {
            snapshot: None,
            history: VecDeque::new(),
            changed: watch::channel(0).0,
        }
    }
}
impl EventState {
    pub fn wake(&self) {
        self.changed
            .send_modify(|generation| *generation = generation.wrapping_add(1));
    }
    pub fn invalidate(&mut self) {
        self.snapshot = None;
        self.history.clear();
        self.wake();
    }
}
fn revision(event: &HostEvent) -> u64 {
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
struct PollSlot<'a> {
    service: &'a CommandService,
    device: Uuid,
}
impl Drop for PollSlot<'_> {
    fn drop(&mut self) {
        if let Ok(mut polls) = self.service.polls.lock() {
            polls.remove(&self.device);
        }
    }
}
impl CommandService {
    pub fn publish(
        &self,
        window: &str,
        lease: &CoordinatorLease,
        update: HostUpdate,
    ) -> Result<u64, ControlError> {
        self.ensure_running()?;
        // Validate even internally constructed projections, not only IPC deserialization.
        let bytes =
            serde_json::to_vec(&update).map_err(|_| error(ControlErrorCode::InvalidRequest))?;
        if bytes.len() > 1024 * 1024 {
            return Err(error(ControlErrorCode::TooLarge));
        }
        let HostUpdate::Projection { mut snapshot } =
            serde_json::from_slice(&bytes).map_err(|_| error(ControlErrorCode::InvalidRequest))?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| error(ControlErrorCode::HostNotReady))?;
        self.ensure_running()?;
        current(&mut state, window, lease)?;
        if snapshot.host_id != self.host_id || snapshot.host_epoch != lease.host_epoch.to_string() {
            return Err(error(ControlErrorCode::InvalidRequest));
        }
        if snapshot.outputs.len() > 200 || !snapshot.jobs.is_empty() {
            return Err(error(ControlErrorCode::TooLarge));
        }
        // Library/artwork and jobs are not installed yet. Do not advertise them.
        if snapshot
            .capabilities
            .queries
            .iter()
            .any(|q| !matches!(q, QueryType::Snapshot | QueryType::Outputs))
        {
            return Err(error(ControlErrorCode::Unsupported));
        }
        let events = &mut state.events;
        let mut changes: Vec<HostEvent> = Vec::new();
        let mut next = events.snapshot.as_ref().map_or(0, |s| s.revision);
        if let Some(old) = &events.snapshot {
            if snapshot.revisions.library_revision < old.revisions.library_revision
                || snapshot.revisions.queue_revision < old.revisions.queue_revision
                || snapshot.revisions.output_revision < old.revisions.output_revision
                || snapshot.revisions.settings_revision < old.revisions.settings_revision
            {
                return Err(error(ControlErrorCode::RevisionConflict));
            }
            let mut allocate = || {
                next += 1;
                next
            };
            if snapshot.playback != old.playback {
                changes.push(HostEvent::Playback {
                    revision: allocate(),
                    playback: snapshot.playback.clone(),
                });
            }
            if snapshot.queue != old.queue
                || snapshot.revisions.queue_revision != old.revisions.queue_revision
            {
                changes.push(HostEvent::Queue {
                    revision: allocate(),
                    queue_revision: snapshot.revisions.queue_revision,
                    queue: snapshot.queue.clone(),
                });
            }
            if snapshot.revisions.library_revision != old.revisions.library_revision {
                changes.push(HostEvent::Library {
                    revision: allocate(),
                    library_revision: snapshot.revisions.library_revision,
                });
            }
            if snapshot.output != old.output
                || snapshot.outputs != old.outputs
                || snapshot.revisions.output_revision != old.revisions.output_revision
            {
                changes.push(HostEvent::Output {
                    revision: allocate(),
                    output_revision: snapshot.revisions.output_revision,
                    output: snapshot.output.clone(),
                    outputs: snapshot.outputs.clone(),
                });
            }
            if snapshot.settings != old.settings
                || snapshot.revisions.settings_revision != old.revisions.settings_revision
            {
                changes.push(HostEvent::Settings {
                    revision: allocate(),
                    settings_revision: snapshot.revisions.settings_revision,
                    settings: snapshot.settings.clone(),
                });
            }
            if snapshot.capabilities != old.capabilities {
                changes.push(HostEvent::Capabilities {
                    revision: allocate(),
                    capabilities: snapshot.capabilities.clone(),
                });
            }
        }
        if next > 9_007_199_254_740_991 {
            return Err(error(ControlErrorCode::ResyncRequired));
        }
        snapshot.revision = next;
        events.snapshot = Some(snapshot);
        for change in changes {
            events.history.push_back(change);
            if events.history.len() > 256 {
                events.history.pop_front();
            }
        }
        events.wake();
        Ok(next)
    }
    pub fn capture_snapshot(&self) -> Result<HostSnapshot, ControlError> {
        self.ensure_running()?;
        let state = self
            .state
            .lock()
            .map_err(|_| error(ControlErrorCode::HostNotReady))?;
        self.ensure_running()?;
        if !state.coordinator_ready() {
            return Err(error(ControlErrorCode::HostNotReady));
        }
        state
            .events
            .snapshot
            .clone()
            .ok_or_else(|| error(ControlErrorCode::HostNotReady))
    }
    pub fn authenticated_snapshot(
        &self,
        device: &AuthenticatedDevice,
    ) -> Result<HostSnapshot, ControlError> {
        self.ensure_running()?;
        let state = self
            .state
            .lock()
            .map_err(|_| error(ControlErrorCode::HostNotReady))?;
        self.ensure_running()?;
        self.pairing.authenticate(device.id, &device.secret)?;
        self.ensure_running()?;
        if !state.coordinator_ready() {
            return Err(error(ControlErrorCode::HostNotReady));
        }
        state
            .events
            .snapshot
            .clone()
            .ok_or_else(|| error(ControlErrorCode::HostNotReady))
    }
    pub async fn poll_events(
        &self,
        device: &AuthenticatedDevice,
        cursor: EventCursor,
    ) -> Result<EventBatch, ControlError> {
        self.ensure_running()?;
        let mut stopped = self.stopped.subscribe();
        let mut changed = {
            let state = self
                .state
                .try_lock()
                .map_err(|_| error(ControlErrorCode::Busy))?;
            self.ensure_running()?;
            self.pairing.authenticate(device.id, &device.secret)?;
            self.ensure_running()?;
            let mut polls = self
                .polls
                .lock()
                .map_err(|_| error(ControlErrorCode::HostNotReady))?;
            if !polls.insert(device.id) {
                return Err(error(ControlErrorCode::Busy));
            }
            state.events.changed.subscribe()
        };
        let _slot = PollSlot {
            service: self,
            device: device.id,
        };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(25);
        loop {
            self.ensure_running()?;
            let batch = {
                let state = self
                    .state
                    .try_lock()
                    .map_err(|_| error(ControlErrorCode::Busy))?;
                self.ensure_running()?;
                self.pairing.authenticate(device.id, &device.secret)?;
                self.ensure_running()?;
                if !state.coordinator_ready() {
                    return Err(error(ControlErrorCode::HostNotReady));
                }
                let snapshot = state
                    .events
                    .snapshot
                    .as_ref()
                    .ok_or_else(|| error(ControlErrorCode::HostNotReady))?;
                let floor = state
                    .events
                    .history
                    .front()
                    .map_or(snapshot.revision, |event| revision(event) - 1);
                if cursor.host_epoch != snapshot.host_epoch
                    || cursor.revision < floor
                    || cursor.revision > snapshot.revision
                {
                    return Err(error(ControlErrorCode::ResyncRequired));
                }
                EventBatch {
                    host_epoch: snapshot.host_epoch.clone(),
                    revision: snapshot.revision,
                    events: state
                        .events
                        .history
                        .iter()
                        .filter(|event| revision(event) > cursor.revision)
                        .cloned()
                        .collect(),
                }
            };
            if !batch.events.is_empty() || tokio::time::Instant::now() >= deadline {
                return Ok(batch);
            }
            tokio::select! { biased; _=stopped.changed()=>return Err(error(ControlErrorCode::HostNotReady)), _=tokio::time::sleep_until(deadline)=>{}, result=changed.changed()=>{ result.map_err(|_|error(ControlErrorCode::HostNotReady))?; } }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::controller::{
        commands::AuthoritativeWindow,
        identity::load_or_create_identity,
        pairing::{create_invitation, Grants, PairingService},
        secrets::tests::MemoryStore,
    };
    use std::{
        sync::Arc,
        time::{Duration, SystemTime},
    };
    pub(crate) fn projection(epoch: &str) -> HostSnapshot {
        serde_json::from_value(serde_json::json!({"hostId":"host","hostEpoch":epoch,"revision":999,"revisions":{"libraryRevision":0,"queueRevision":0,"outputRevision":0,"settingsRevision":0},"playback":{"status":"stopped","track":null,"context":null,"position":0,"duration":null,"volume":0.5,"shuffle":false,"repeat":"none"},"queue":{"count":0,"currentEntryId":null},"output":{"kind":"pc"},"outputs":[],"capabilities":{"queries":["snapshot","outputs"],"intents":["pause"]},"settings":{},"jobs":[]})).unwrap()
    }
    fn fixture() -> (CommandService, AuthenticatedDevice, CoordinatorLease) {
        let store = Arc::new(MemoryStore::default());
        let identity = load_or_create_identity(store.as_ref()).unwrap();
        let pairing = Arc::new(PairingService::new(&identity, store).unwrap());
        let invitation = create_invitation(
            &identity,
            "192.168.1.2:9010".parse().unwrap(),
            SystemTime::now(),
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
        let service = CommandService::new(pairing, "host".into());
        let lease = service
            .register_coordinator(AuthoritativeWindow::new(
                "main".into(),
                Arc::new(|_| Ok(())),
            ))
            .unwrap();
        (service, device, lease)
    }
    fn initial(service: &CommandService, lease: &CoordinatorLease) {
        assert_eq!(
            service
                .publish(
                    "main",
                    lease,
                    HostUpdate::Projection {
                        snapshot: projection(&lease.host_epoch.to_string())
                    }
                )
                .unwrap(),
            0
        );
        service.ready("main", lease).unwrap();
    }
    fn cursor(service: &CommandService) -> EventCursor {
        let snapshot = service.capture_snapshot().unwrap();
        EventCursor {
            host_epoch: snapshot.host_epoch,
            revision: snapshot.revision,
        }
    }
    fn progress(service: &CommandService, lease: &CoordinatorLease, position: f64) {
        let mut snapshot = projection(&lease.host_epoch.to_string());
        snapshot.playback.position = position;
        service
            .publish("main", lease, HostUpdate::Projection { snapshot })
            .unwrap();
    }
    #[tokio::test]
    async fn snapshot_replay_has_no_gap() {
        let (s, d, l) = fixture();
        initial(&s, &l);
        let c = cursor(&s);
        assert_eq!(c.revision, 0);
        progress(&s, &l, 1.0);
        progress(&s, &l, 2.0);
        let batch = s.poll_events(&d, c).await.unwrap();
        assert_eq!(batch.events.len(), 2);
        assert_eq!(batch.revision, 2);
        let snapshot = s.capture_snapshot().unwrap();
        assert_eq!(snapshot.revision, 2);
        assert_eq!(snapshot.playback.position, 2.0);
        assert_eq!(snapshot.revisions.queue_revision, 0);
    }
    #[tokio::test]
    async fn expired_history_requires_resync() {
        let (s, d, l) = fixture();
        initial(&s, &l);
        let c = cursor(&s);
        for n in 1..=257 {
            progress(&s, &l, n as f64);
        }
        let expired_cursor_error = s.poll_events(&d, c).await.unwrap_err();
        assert_eq!(
            serde_json::to_value(expired_cursor_error).unwrap()["code"],
            "resync_required"
        );
        let batch = s
            .poll_events(
                &d,
                EventCursor {
                    host_epoch: l.host_epoch.to_string(),
                    revision: 1,
                },
            )
            .await
            .unwrap();
        assert_eq!(batch.events.len(), 256);
    }
    #[tokio::test(start_paused = true)]
    async fn idle_poll_finishes_at_25_seconds() {
        let (s, d, l) = fixture();
        initial(&s, &l);
        let poll = s.poll_events(&d, cursor(&s));
        tokio::pin!(poll);
        tokio::select! { biased; result=&mut poll=>panic!("early completion: {result:?}"), _=tokio::time::sleep(Duration::from_secs(24))=>{} }
        tokio::select! { biased; result=&mut poll=>panic!("pending at 24: {result:?}"), _=tokio::task::yield_now()=>{} }
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(poll.await.unwrap().events.is_empty());
    }
    #[tokio::test(start_paused = true)]
    async fn immediate_wakeup_single_poll_and_cancellation() {
        let (s, d, l) = fixture();
        initial(&s, &l);
        let poll = s.poll_events(&d, cursor(&s));
        tokio::pin!(poll);
        tokio::select! { biased; _=&mut poll=>panic!("not pending"), _=tokio::task::yield_now()=>{} }
        assert_eq!(
            s.poll_events(&d, cursor(&s)).await.unwrap_err().code,
            ControlErrorCode::Busy
        );
        let before = tokio::time::Instant::now();
        progress(&s, &l, 2.0);
        assert_eq!(poll.await.unwrap().events.len(), 1);
        assert_eq!(before, tokio::time::Instant::now());
        let poll = s.poll_events(&d, cursor(&s));
        tokio::pin!(poll);
        tokio::select! { biased; _=&mut poll=>panic!("not pending"), _=tokio::task::yield_now()=>{} }
        let exit = crate::app_exit::AppExit::default();
        assert_eq!(
            exit.request(None, || s.invalidate_window("main")),
            crate::app_exit::ExitAction::StopPlayers(0)
        );
        assert!(s.capture_snapshot().is_err());
        assert_eq!(poll.await.unwrap_err().code, ControlErrorCode::HostNotReady);
    }
    #[tokio::test]
    async fn reload_revokes_old_publisher() {
        let (s, _, old) = fixture();
        initial(&s, &old);
        let new = s
            .register_coordinator(AuthoritativeWindow::new(
                "main".into(),
                Arc::new(|_| Ok(())),
            ))
            .unwrap();
        assert_ne!(old.host_epoch, new.host_epoch);
        assert_eq!(
            s.publish(
                "main",
                &old,
                HostUpdate::Projection {
                    snapshot: projection(&old.host_epoch.to_string())
                }
            )
            .unwrap_err()
            .code,
            ControlErrorCode::Unauthorized
        );
        assert!(s.capture_snapshot().is_err());
        initial(&s, &new);
        assert_eq!(
            s.capture_snapshot().unwrap().host_epoch,
            new.host_epoch.to_string()
        );
    }
    #[tokio::test(start_paused = true)]
    async fn revocation_wakes_pending_poll_and_dropped_poll_releases_slot() {
        let (s, d, l) = fixture();
        initial(&s, &l);
        {
            let poll = s.poll_events(&d, cursor(&s));
            tokio::pin!(poll);
            tokio::select! { biased; _=&mut poll=>panic!("not pending"),_=tokio::task::yield_now()=>{} }
        }
        let poll = s.poll_events(&d, cursor(&s));
        tokio::pin!(poll);
        tokio::select! { biased; _=&mut poll=>panic!("not pending"),_=tokio::task::yield_now()=>{} }
        s.revoke(d.id).unwrap();
        assert_eq!(poll.await.unwrap_err().code, ControlErrorCode::Unauthorized);
    }
    #[test]
    fn publication_rejects_wrong_window_identity_bounds_and_regressing_domains() {
        let (s, _, l) = fixture();
        initial(&s, &l);
        let update = || HostUpdate::Projection {
            snapshot: projection(&l.host_epoch.to_string()),
        };
        assert_eq!(
            s.publish("other", &l, update()).unwrap_err().code,
            ControlErrorCode::Unauthorized
        );
        let mut wrong = projection("wrong");
        wrong.host_id = "other".into();
        assert!(s
            .publish("main", &l, HostUpdate::Projection { snapshot: wrong })
            .is_err());
        let mut invalid = projection(&l.host_epoch.to_string());
        invalid.playback.volume = 2.0;
        assert!(s
            .publish("main", &l, HostUpdate::Projection { snapshot: invalid })
            .is_err());
        let mut newer = projection(&l.host_epoch.to_string());
        newer.revisions.queue_revision = 2;
        s.publish("main", &l, HostUpdate::Projection { snapshot: newer })
            .unwrap();
        assert_eq!(
            s.publish("main", &l, update()).unwrap_err().code,
            ControlErrorCode::RevisionConflict
        );
        assert_eq!(s.capture_snapshot().unwrap().revisions.queue_revision, 2);
    }
    #[tokio::test]
    async fn exit_cancels_poll_without_waiting_for_command_lock_or_slot_drop() {
        let (service, device, lease) = fixture();
        let service = Arc::new(service);
        initial(&service, &lease);
        let poll = service.poll_events(&device, cursor(&service));
        tokio::pin!(poll);
        tokio::select! { biased; _=&mut poll=>panic!("not pending"), _=tokio::task::yield_now()=>{} }
        let (locked_tx, locked_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let worker = service.clone();
        let thread = std::thread::spawn(move || {
            let _held = worker.state.lock().unwrap();
            locked_tx.send(()).unwrap();
            let _ = release_rx.recv_timeout(Duration::from_secs(1));
        });
        locked_rx.recv().unwrap();
        let exit = crate::app_exit::AppExit::default();
        let start = std::time::Instant::now();
        assert_eq!(
            exit.request(None, || service.stop()),
            crate::app_exit::ExitAction::StopPlayers(0)
        );
        let failure = poll.await.unwrap_err();
        let elapsed = start.elapsed();
        let _ = release_tx.send(());
        thread.join().unwrap();
        assert_eq!(failure.code, ControlErrorCode::HostNotReady);
        assert!(
            elapsed < Duration::from_millis(100),
            "Poll cancellation/drop waited for command lock: {elapsed:?}"
        );
        assert!(service.polls.lock().unwrap().is_empty());
        assert!(service.capture_snapshot().is_err());
        assert!(service.ready("main", &lease).is_err());
        assert!(service
            .publish(
                "main",
                &lease,
                HostUpdate::Projection {
                    snapshot: projection(&lease.host_epoch.to_string())
                }
            )
            .is_err());
    }
}
