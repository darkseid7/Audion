use super::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
#[derive(Default)]
struct Memory(Mutex<HashMap<String, Vec<u8>>>);
impl ControllerStore for Memory {
    fn load(&self, id: &str) -> Result<Option<zeroize::Zeroizing<Vec<u8>>>, ControlError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .map(zeroize::Zeroizing::new))
    }
    fn save(&self, id: &str, bytes: &[u8]) -> Result<(), ControlError> {
        self.0.lock().unwrap().insert(id.into(), bytes.into());
        Ok(())
    }
    fn delete(&self, id: &str) -> Result<(), ControlError> {
        self.0.lock().unwrap().remove(id);
        Ok(())
    }
}
#[test]
fn fresh_renderer_scope_rejects_prior_high_generation_and_unsafe_integers() {
    let session = NativeSession::default();
    let old = session.begin_scope().unwrap();
    session
        .suspend(&Fence {
            scope_id: old.clone(),
            generation: 8000,
        })
        .unwrap();
    let fresh = session.begin_scope().unwrap();
    assert_ne!(fresh, old);
    assert!(session
        .suspend(&Fence {
            scope_id: old,
            generation: 9000
        })
        .is_err());
    session
        .suspend(&Fence {
            scope_id: fresh,
            generation: 1,
        })
        .unwrap();
    for n in ["-1", "0.1", "9007199254740992"] {
        assert!(serde_json::from_str::<Fence>(&format!(
            r#"{{"scopeId":"scope","generation":{n}}}"#
        ))
        .is_err());
    }
}
#[test]
fn installation_pair_admission_survives_staging_and_is_cancelled_by_scope() {
    let session = NativeSession::default();
    let f = Fence {
        scope_id: session.begin_scope().unwrap(),
        generation: 1,
    };
    session.reserve_pairing(&f).unwrap();
    assert!(session.reserve_pairing(&f).is_err());
    let fresh = session.begin_scope().unwrap();
    assert!(session.stage_pairing(&f, None).is_err());
    session
        .reserve_pairing(&Fence {
            scope_id: fresh,
            generation: 1,
        })
        .unwrap();
}

use crate::controller::{
    identity::load_or_create_identity,
    pairing::{create_invitation, Grants, NativeCredential, PairingService},
    secrets::SecretStore,
};
use axum::{
    body::to_bytes,
    extract::{Request, State},
    response::{IntoResponse, Response},
    Router,
};
use std::net::SocketAddr;
impl SecretStore for Memory {
    fn read(&self, key: &str) -> Result<Option<Vec<u8>>, ControlError> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }
    fn write(&self, key: &str, bytes: &[u8]) -> Result<(), ControlError> {
        ControllerStore::save(self, key, bytes)
    }
    fn delete(&self, key: &str) -> Result<(), ControlError> {
        ControllerStore::delete(self, key)
    }
}
struct Fixture {
    metadata_missing: AtomicBool,
    metadata_bad: AtomicBool,
    metadata_large: AtomicBool,
    host: String,
    epoch: Mutex<String>,
    pairing: PairingService,
    issued: Mutex<Option<NativeCredential>>,
    requests: Mutex<Vec<String>>,
    query_hold: AtomicBool,
    query_entered: tokio::sync::Notify,
    query_release: tokio::sync::Notify,
    poll_entered: tokio::sync::Notify,
    poll_release: tokio::sync::Notify,
    pair_hold: AtomicBool,
    pair_entered: tokio::sync::Notify,
    pair_release: tokio::sync::Notify,
    art: Vec<u8>,
    revoked: AtomicBool,
    control: AtomicBool,
    omit_grants: AtomicBool,
    resource_denied: AtomicBool,
    outcome_expired: AtomicBool,
    snapshot_hold: AtomicBool,
    snapshot_entered: tokio::sync::Notify,
    snapshot_release: tokio::sync::Notify,
    poll_library: AtomicBool,
}
fn snapshot(host: &str, epoch: &str) -> HostSnapshot {
    serde_json::from_value(serde_json::json!({"hostId":host,"hostEpoch":epoch,"revision":0,"revisions":{"libraryRevision":1,"queueRevision":0,"outputRevision":0,"settingsRevision":0},"playback":{"status":"paused","track":null,"context":null,"position":0,"duration":null,"volume":0.5,"shuffle":false,"repeat":"none"},"queue":{"count":0,"currentEntryId":null},"output":{"kind":"pc"},"outputs":[],"capabilities":{"queries":["snapshot","tracks"],"intents":["pause"]},"settings":{},"jobs":[]})).unwrap()
}
async fn route(State(f): State<Arc<Fixture>>, request: Request) -> Response {
    let path = request.uri().path().to_owned();
    f.requests.lock().unwrap().push(path.clone());
    let auth = request
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .map(str::to_owned);
    let body = to_bytes(request.into_body(), 1024 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    if !path.contains("pairing") {
        let valid = auth
            .and_then(|h| h.strip_prefix("Bearer ").map(str::to_owned))
            .and_then(|a| {
                a.split_once(':')
                    .map(|(id, s)| (id.to_owned(), s.to_owned()))
            })
            .is_some_and(|(id, secret)| {
                uuid::Uuid::parse_str(&id)
                    .ok()
                    .zip(URL_SAFE_NO_PAD.decode(secret).ok())
                    .is_some_and(|(id, secret)| f.pairing.authenticate(id, &secret).is_ok())
            });
        if !valid || f.revoked.load(Ordering::SeqCst) {
            return (
                axum::http::StatusCode::UNAUTHORIZED,
                axum::Json(transport_error(ControlErrorCode::Unauthorized)),
            )
                .into_response();
        }
    }
    match path.as_str(){
  "/control/v1/pairing"=>{let invite=json["invitation"].as_str().unwrap();match f.pairing.request_wire(invite,"Phone".into()){Ok(p)=>{let credential=f.pairing.approve_pairing(p.id,Grants{control:f.control.load(Ordering::SeqCst),administration:true}).unwrap();*f.issued.lock().unwrap()=Some(credential);axum::Json(serde_json::json!({"status":"pending","pendingId":p.id.to_string()})).into_response()},Err(e)=>(axum::http::StatusCode::UNAUTHORIZED,axum::Json(e)).into_response()}},
  "/control/v1/pairing/status"=>{if f.pair_hold.load(Ordering::SeqCst){f.pair_entered.notify_one();f.pair_release.notified().await;}match f.issued.lock().unwrap().take(){Some(c)=>axum::Json(serde_json::json!({"status":"approved","deviceId":c.device_id().to_string(),"secret":URL_SAFE_NO_PAD.encode(c.secret())})).into_response(),None=>(axum::http::StatusCode::UNAUTHORIZED,axum::Json(transport_error(ControlErrorCode::Unauthorized))).into_response()}},
  "/control/v1/handshake"=>{ let mut response=serde_json::json!({"protocolVersion":1,"hostId":f.host,"hostEpoch":*f.epoch.lock().unwrap(),"capabilities":{"queries":["snapshot","tracks"],"intents":["pause"]},"grants":{"control":f.control.load(Ordering::SeqCst)}}); if f.omit_grants.load(Ordering::SeqCst) { response.as_object_mut().unwrap().remove("grants"); } axum::Json(response).into_response() },
  "/control/v1/browse-metadata"=>{
    if f.metadata_missing.load(Ordering::SeqCst){return (axum::http::StatusCode::NOT_FOUND,axum::Json(transport_error(ControlErrorCode::NotFound))).into_response();}
    if f.query_hold.load(Ordering::SeqCst){f.query_entered.notify_one();f.query_release.notified().await;}
    if f.metadata_large.load(Ordering::SeqCst){return ([("content-type","application/json")]," ".repeat(256*1024+1)).into_response();}
    let ids=json["trackIds"].as_array().unwrap();
    let tracks:Vec<_>=ids.iter().map(|id|serde_json::json!({"trackId":id,"playCount":0})).collect();
    let mut reply=serde_json::json!({"metadataVersion":1,"hostEpoch":json["hostEpoch"],"libraryRevision":json["libraryRevision"],"tracks":tracks});
    if let Some(id)=json.get("albumId"){reply["album"]=serde_json::json!({"albumId":id,"totalDurationSeconds":202});}
    if f.metadata_bad.load(Ordering::SeqCst){reply["tracks"][0]["trackId"]=serde_json::json!(999);}
    axum::Json(reply).into_response()
  },  "/control/v1/queries"=>{if json["type"]=="snapshot"{let captured=snapshot(&f.host,&f.epoch.lock().unwrap());if f.snapshot_hold.load(Ordering::SeqCst){f.snapshot_entered.notify_one();f.snapshot_release.notified().await;}axum::Json(QueryResult::Snapshot{snapshot:captured}).into_response()}else{if f.query_hold.load(Ordering::SeqCst){f.query_entered.notify_one();f.query_release.notified().await;}axum::Json(serde_json::json!({"type":"tracks","page":{"items":[],"nextCursor":null,"revision":1}})).into_response()}},
  "/control/v1/events"=>{f.poll_entered.notify_one();f.poll_release.notified().await;if f.poll_library.load(Ordering::SeqCst){axum::Json(serde_json::json!({"hostEpoch":*f.epoch.lock().unwrap(),"revision":1,"events":[{"type":"library","revision":1,"libraryRevision":2}]})).into_response()}else{axum::Json(serde_json::json!({"hostEpoch":*f.epoch.lock().unwrap(),"revision":0,"events":[]})).into_response()}},
  "/control/v1/commands"=>(axum::http::StatusCode::GATEWAY_TIMEOUT,axum::Json(transport_error(ControlErrorCode::OutcomeUnknown))).into_response(),
  "/control/v1/commands/status"=>{if f.outcome_expired.load(Ordering::SeqCst){(axum::http::StatusCode::BAD_REQUEST,axum::Json(transport_error(ControlErrorCode::OutcomeUnknown))).into_response()}else{axum::Json(serde_json::json!({"status":"applied","revision":0})).into_response()}},
  "/control/v1/resources"=>{if f.resource_denied.load(Ordering::SeqCst){(axum::http::StatusCode::NOT_FOUND,axum::Json(transport_error(ControlErrorCode::NotFound))).into_response()}else{([("content-type","image/png")],f.art.clone()).into_response()}},
  _=>axum::http::StatusCode::NOT_FOUND.into_response()
 }
}
struct Server {
    fixture: Arc<Fixture>,
    session: Arc<NativeSession>,
    store: Arc<Memory>,
    invitation: Zeroizing<String>,
    handle: axum_server::Handle<SocketAddr>,
    task: tokio::task::JoinHandle<()>,
    endpoint: SocketAddr,
    ca: Vec<u8>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.handle.shutdown();
        self.task.abort();
    }
}
async fn server() -> Server {
    let identity = load_or_create_identity(&Memory::default()).unwrap();
    let host = identity.id().to_owned();
    let ca = identity.ca_der().to_vec();
    let pairing = PairingService::new(&identity, Arc::new(Memory::default())).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let invitation = create_invitation(
        &identity,
        format!("192.168.1.8:{}", endpoint.port()).parse().unwrap(),
        SystemTime::now(),
    )
    .unwrap();
    pairing.register_invitation(&invitation).unwrap();
    let invitation = invitation.encode().unwrap();
    let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![rustls::pki_types::CertificateDer::from(
            identity.leaf_der().to_vec(),
        )],
        identity.leaf_private_key().unwrap(),
    )
    .unwrap();
    let config = axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(tls));
    let mut image = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(1, 1)
        .write_to(&mut image, image::ImageFormat::Png)
        .unwrap();
    let fixture = Arc::new(Fixture {
        metadata_missing: AtomicBool::new(false),
        metadata_bad: AtomicBool::new(false),
        metadata_large: AtomicBool::new(false),
        host,
        epoch: Mutex::new("epoch".into()),
        pairing,
        issued: Mutex::new(None),
        requests: Mutex::new(vec![]),
        query_hold: AtomicBool::new(false),
        query_entered: Default::default(),
        query_release: Default::default(),
        poll_entered: Default::default(),
        poll_release: Default::default(),
        pair_hold: AtomicBool::new(false),
        pair_entered: Default::default(),
        pair_release: Default::default(),
        art: image.into_inner(),
        revoked: AtomicBool::new(false),
        control: AtomicBool::new(true),
        omit_grants: AtomicBool::new(false),
        resource_denied: AtomicBool::new(false),
        outcome_expired: AtomicBool::new(false),
        snapshot_hold: AtomicBool::new(false),
        snapshot_entered: Default::default(),
        snapshot_release: Default::default(),
        poll_library: AtomicBool::new(false),
    });
    let handle = axum_server::Handle::new();
    let serving = axum_server::from_tcp_rustls(listener, config)
        .unwrap()
        .handle(handle.clone());
    let app = Router::new().fallback(route).with_state(fixture.clone());
    let task = tokio::spawn(async move {
        let _ = serving.serve(app.into_make_service()).await;
    });
    let session = Arc::new(NativeSession {
        inner: Mutex::default(),
        connector: Arc::new(move |p, a| {
            crate::controller::client::tests::loopback_transport(p, a, endpoint)
        }),
    });
    Server {
        fixture,
        session,
        store: Arc::new(Memory::default()),
        invitation,
        handle,
        task,
        endpoint,
        ca,
    }
}
async fn pair(s: &Server) -> Fence {
    let f = Fence {
        scope_id: s.session.begin_scope().unwrap(),
        generation: 1,
    };
    s.session.reserve_pairing(&f).unwrap();
    s.session
        .stage_pairing(&f, Some(Zeroizing::new(s.invitation.to_string())))
        .unwrap();
    assert_eq!(
        s.session
            .pair(s.store.clone(), f.clone(), "Phone".into())
            .await
            .unwrap(),
        s.fixture.host
    );
    f
}
async fn connect(s: &Server) -> Fence {
    let mut f = pair(s).await;
    f.generation += 1;
    let snapshot = s
        .session
        .connect(s.store.clone(), s.fixture.host.clone(), f.clone())
        .await
        .unwrap();
    assert_eq!(snapshot.0.host_id, s.fixture.host);
    assert!(snapshot.1.control);
    f
}
#[tokio::test]
async fn actual_tls_pair_persists_once_resume_forget_and_revocation() {
    let s = server().await;
    let mut f = connect(&s).await;
    assert!(s.store.0.lock().unwrap().contains_key(&s.fixture.host));
    let reference = ArtworkReference {
        resource_id: "opaque-art".into(),
        revision: 1,
    };
    for _ in 0..2 {
        assert!(matches!(
            s.session
                .request(
                    &f,
                    ControllerRequest::Media {
                        reference: reference.clone()
                    }
                )
                .await,
            Ok(ControllerReply::Media { .. })
        ));
    }
    assert_eq!(
        s.fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.ends_with("resources"))
            .count(),
        2
    );
    assert!(s.session.inner.lock().unwrap().cache.bytes > 0);
    s.fixture.revoked.store(true, Ordering::SeqCst);
    assert!(matches!(
        s.session
            .request(&f, ControllerRequest::Media { reference })
            .await,
        Err(ControlError {
            code: ControlErrorCode::Unauthorized,
            ..
        })
    ));
    assert_eq!(s.session.inner.lock().unwrap().cache.bytes, 0);
    s.fixture.revoked.store(false, Ordering::SeqCst);
    f.generation += 1;
    s.session.suspend(&f).unwrap();
    f.generation += 1;
    s.session
        .connect(s.store.clone(), s.fixture.host.clone(), f.clone())
        .await
        .unwrap();
    s.store
        .0
        .lock()
        .unwrap()
        .insert("other-host".into(), vec![1]);
    f.generation += 1;
    s.session
        .forget(s.store.clone(), s.fixture.host.clone(), f)
        .await
        .unwrap();
    assert!(!s.store.0.lock().unwrap().contains_key(&s.fixture.host));
    assert!(s.store.0.lock().unwrap().contains_key("other-host"));
    assert!(s.session.inner.lock().unwrap().active.is_none());
    let requests = s.fixture.requests.lock().unwrap();
    assert_eq!(
        requests
            .iter()
            .filter(|p| p.as_str() == "/control/v1/pairing")
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|p| p.as_str() == "/control/v1/pairing/status")
            .count(),
        1
    );
}
#[tokio::test]
async fn native_one_poll_and_scope_cancellation_discard_late_queries() {
    let s = server().await;
    let f = connect(&s).await;
    let session = s.session.clone();
    let fence = f.clone();
    let poll = tokio::spawn(async move {
        session
            .request(
                &fence,
                ControllerRequest::Poll {
                    cursor: EventCursor {
                        host_epoch: "epoch".into(),
                        revision: 0,
                    },
                },
            )
            .await
    });
    s.fixture.poll_entered.notified().await;
    assert!(matches!(
        s.session
            .request(
                &f,
                ControllerRequest::Poll {
                    cursor: EventCursor {
                        host_epoch: "epoch".into(),
                        revision: 0
                    }
                }
            )
            .await,
        Err(ControlError {
            code: ControlErrorCode::Busy,
            ..
        })
    ));
    s.fixture.query_hold.store(true, Ordering::SeqCst);
    let session = s.session.clone();
    let fence = f.clone();
    let query = tokio::spawn(async move {
        session
            .request(
                &fence,
                ControllerRequest::Query {
                    query: ApplicationQuery::Tracks {
                        limit: None,
                        cursor: None,
                    },
                },
            )
            .await
    });
    s.fixture.query_entered.notified().await;
    s.session.begin_scope().unwrap();
    assert!(matches!(
        query.await.unwrap(),
        Err(ControlError {
            code: ControlErrorCode::ResyncRequired,
            ..
        })
    ));
    assert!(poll.await.unwrap().is_err());
    s.fixture.query_release.notify_one();
    s.fixture.poll_release.notify_one();
    assert!(s.session.inner.lock().unwrap().active.is_none());
}
#[tokio::test]
async fn native_timeout_status_does_not_replay_and_host_restart_invalidates_epoch() {
    let s = server().await;
    let f = connect(&s).await;
    let envelope:CommandEnvelope=serde_json::from_value(serde_json::json!({"protocolVersion":1,"requestId":"once","preconditions":{"hostEpoch":"epoch","outputRevision":0},"intent":{"type":"pause"}})).unwrap();
    assert!(matches!(
        s.session
            .request(&f, ControllerRequest::Command { envelope })
            .await,
        Err(ControlError {
            code: ControlErrorCode::OutcomeUnknown,
            ..
        })
    ));
    assert!(matches!(
        s.session
            .request(
                &f,
                ControllerRequest::CommandStatus {
                    request_id: "once".into()
                }
            )
            .await,
        Ok(ControllerReply::CommandStatus { .. })
    ));
    assert_eq!(
        s.fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.as_str() == "/control/v1/commands")
            .count(),
        1
    );
    s.fixture.outcome_expired.store(true, Ordering::SeqCst);
    assert!(matches!(
        s.session
            .request(
                &f,
                ControllerRequest::CommandStatus {
                    request_id: "expired".into()
                }
            )
            .await,
        Err(ControlError {
            code: ControlErrorCode::OutcomeUnknown,
            ..
        })
    ));
    assert_eq!(
        s.fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.as_str() == "/control/v1/commands")
            .count(),
        1
    );
    *s.fixture.epoch.lock().unwrap() = "restart".into();
    assert!(matches!(
        s.session
            .request(
                &f,
                ControllerRequest::Query {
                    query: ApplicationQuery::Tracks {
                        limit: None,
                        cursor: None
                    }
                }
            )
            .await,
        Err(ControlError {
            code: ControlErrorCode::ResyncRequired,
            ..
        })
    ));
}
#[test]
fn native_cache_exact_fifty_mib_lru_and_five_mib_images() {
    let mut cache = ImageCache::default();
    for i in 0..10 {
        cache
            .insert(
                i.to_string(),
                "host".into(),
                NativeImage {
                    mime: "image/png".into(),
                    bytes: vec![1; 5 * 1024 * 1024],
                },
            )
            .unwrap();
    }
    assert_eq!(cache.bytes, 50 * 1024 * 1024);
    assert!(cache.get("0").is_some());
    cache
        .insert(
            "new".into(),
            "other".into(),
            NativeImage {
                mime: "image/png".into(),
                bytes: vec![1; 5 * 1024 * 1024],
            },
        )
        .unwrap();
    assert!(cache.get("1").is_none());
    assert!(cache.get("0").is_some());
    assert_eq!(cache.bytes, 50 * 1024 * 1024);
    assert!(cache
        .insert(
            "large".into(),
            "other".into(),
            NativeImage {
                mime: "image/png".into(),
                bytes: vec![1; 5 * 1024 * 1024 + 1]
            }
        )
        .is_err());
    cache.remove_host("host");
    assert_eq!(cache.bytes, 5 * 1024 * 1024);
}

#[tokio::test]
async fn cached_media_requires_resource_authorization_even_without_revision_change() {
    let s = server().await;
    let f = connect(&s).await;
    let r = ArtworkReference {
        resource_id: "art".into(),
        revision: 1,
    };
    assert!(s
        .session
        .request(
            &f,
            ControllerRequest::Media {
                reference: r.clone()
            }
        )
        .await
        .is_ok());
    s.fixture.resource_denied.store(true, Ordering::SeqCst);
    assert!(matches!(
        s.session
            .request(&f, ControllerRequest::Media { reference: r })
            .await,
        Err(ControlError {
            code: ControlErrorCode::NotFound,
            ..
        })
    ));
    assert_eq!(
        s.fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.ends_with("resources"))
            .count(),
        2
    );
}

struct HeldLoad {
    memory: Arc<Memory>,
    entered: tokio::sync::Notify,
    gate: (Mutex<bool>, std::sync::Condvar),
}
impl ControllerStore for HeldLoad {
    fn load(&self, id: &str) -> Result<Option<Zeroizing<Vec<u8>>>, ControlError> {
        self.entered.notify_one();
        let (lock, signal) = &self.gate;
        let mut go = lock.lock().unwrap();
        while !*go {
            go = signal.wait(go).unwrap();
        }
        ControllerStore::load(self.memory.as_ref(), id)
    }
    fn save(&self, id: &str, b: &[u8]) -> Result<(), ControlError> {
        ControllerStore::save(self.memory.as_ref(), id, b)
    }
    fn delete(&self, id: &str) -> Result<(), ControlError> {
        ControllerStore::delete(self.memory.as_ref(), id)
    }
}
#[tokio::test]
async fn held_native_record_load_cannot_install_after_renderer_reload() {
    let s = server().await;
    let mut f = pair(&s).await;
    f.generation = 2;
    let held = Arc::new(HeldLoad {
        memory: s.store.clone(),
        entered: Default::default(),
        gate: (Mutex::new(false), std::sync::Condvar::new()),
    });
    let session = s.session.clone();
    let store = held.clone();
    let host = s.fixture.host.clone();
    let pending = tokio::spawn(async move { session.connect(store, host, f).await });
    held.entered.notified().await;
    let scope = s.session.begin_scope().unwrap();
    *held.gate.0.lock().unwrap() = true;
    held.gate.1.notify_all();
    assert!(matches!(
        pending.await.unwrap(),
        Err(ControlError {
            code: ControlErrorCode::ResyncRequired,
            ..
        })
    ));
    assert!(s.session.inner.lock().unwrap().active.is_none());
    assert_eq!(s.session.inner.lock().unwrap().scope, scope);
}
struct FailingSave;
impl ControllerStore for FailingSave {
    fn load(&self, _: &str) -> Result<Option<Zeroizing<Vec<u8>>>, ControlError> {
        Ok(None)
    }
    fn save(&self, _: &str, _: &[u8]) -> Result<(), ControlError> {
        Err(transport_error(ControlErrorCode::HostNotReady))
    }
    fn delete(&self, _: &str) -> Result<(), ControlError> {
        Ok(())
    }
}
#[tokio::test]
async fn lost_native_persistence_does_not_claim_paired_or_reuse_invitation() {
    let s = server().await;
    let f = Fence {
        scope_id: s.session.begin_scope().unwrap(),
        generation: 1,
    };
    s.session.reserve_pairing(&f).unwrap();
    s.session
        .stage_pairing(&f, Some(Zeroizing::new(s.invitation.to_string())))
        .unwrap();
    assert!(s
        .session
        .pair(Arc::new(FailingSave), f.clone(), "Phone".into())
        .await
        .is_err());
    assert!(s.session.inner.lock().unwrap().pair.is_none());
    assert!(s.store.0.lock().unwrap().is_empty());
    let next = Fence { generation: 2, ..f };
    s.session.reserve_pairing(&next).unwrap();
    s.session
        .stage_pairing(&next, Some(Zeroizing::new(s.invitation.to_string())))
        .unwrap();
    assert!(s
        .session
        .pair(s.store.clone(), next, "Phone".into())
        .await
        .is_err());
}
#[tokio::test]
async fn changed_certificate_fails_actual_tls_without_replacing_trust() {
    let s = server().await;
    let mut f = pair(&s).await;
    let other = load_or_create_identity(&Memory::default()).unwrap();
    {
        let mut records = s.store.0.lock().unwrap();
        let bytes = records.get_mut(&s.fixture.host).unwrap();
        let mut record: PairedRecord = serde_json::from_slice(bytes).unwrap();
        record.ca = other.ca_der().to_vec();
        *bytes = serde_json::to_vec(&record).unwrap();
    }
    let before = s.fixture.requests.lock().unwrap().len();
    f.generation += 1;
    assert!(s
        .session
        .connect(s.store.clone(), s.fixture.host.clone(), f)
        .await
        .is_err());
    assert_eq!(s.fixture.requests.lock().unwrap().len(), before);
    assert!(s.session.inner.lock().unwrap().active.is_none());
}

#[tokio::test]
async fn facade_rejects_literal_ip_wrong_port_scheme_without_listener_requests() {
    let s = server().await;
    let pairing = NativePairing::new(
        s.fixture.host.clone(),
        format!("192.168.1.8:{}", s.endpoint.port())
            .parse()
            .unwrap(),
        s.ca.clone(),
        uuid::Uuid::new_v4(),
        [7; 32],
    )
    .unwrap();
    let transport =
        crate::controller::client::tests::loopback_transport(pairing, true, s.endpoint).unwrap();
    for url in [
        format!(
            "https://127.0.0.1:{}/control/v1/handshake",
            s.endpoint.port()
        ),
        format!(
            "https://audion-{}.invalid:{}/control/v1/handshake",
            s.fixture.host,
            if s.endpoint.port() == 65535 {
                65534
            } else {
                s.endpoint.port() + 1
            }
        ),
        format!(
            "http://audion-{}.invalid:{}/control/v1/handshake",
            s.fixture.host,
            s.endpoint.port()
        ),
    ] {
        assert!(matches!(
            crate::controller::client::tests::outgoing_url(&transport, &url).await,
            Err(ControlError {
                code: ControlErrorCode::InvalidRequest,
                ..
            })
        ));
    }
    assert!(s.fixture.requests.lock().unwrap().is_empty());
    for endpoint in ["127.0.0.1:9010", "8.8.8.8:9010"] {
        assert!(NativePairing::new(
            s.fixture.host.clone(),
            endpoint.parse().unwrap(),
            s.ca.clone(),
            uuid::Uuid::new_v4(),
            [7; 32]
        )
        .is_err());
    }
}

#[tokio::test]
async fn delayed_pair_reply_is_cancelled_without_persisting_or_parallel_attempt() {
    let s = server().await;
    s.fixture.pair_hold.store(true, Ordering::SeqCst);
    let f = Fence {
        scope_id: s.session.begin_scope().unwrap(),
        generation: 1,
    };
    s.session.reserve_pairing(&f).unwrap();
    s.session
        .stage_pairing(&f, Some(Zeroizing::new(s.invitation.to_string())))
        .unwrap();
    let session = s.session.clone();
    let store = s.store.clone();
    let fence = f.clone();
    let pair = tokio::spawn(async move { session.pair(store, fence, "Phone".into()).await });
    s.fixture.pair_entered.notified().await;
    assert!(matches!(
        s.session.reserve_pairing(&Fence { generation: 2, ..f }),
        Err(ControlError {
            code: ControlErrorCode::Busy,
            ..
        })
    ));
    s.session.begin_scope().unwrap();
    s.fixture.pair_release.notify_one();
    assert!(matches!(
        pair.await.unwrap(),
        Err(ControlError {
            code: ControlErrorCode::ResyncRequired,
            ..
        })
    ));
    assert!(s.store.0.lock().unwrap().is_empty());
}

#[test]
fn concurrent_native_pair_reservations_admit_only_one_installation_attempt() {
    for _ in 0..64 {
        let session = Arc::new(NativeSession::default());
        let scope = session.begin_scope().unwrap();
        let start = Arc::new(std::sync::Barrier::new(3));
        let workers: Vec<_> = (1..=2)
            .map(|generation| {
                let session = session.clone();
                let scope_id = scope.clone();
                let start = start.clone();
                std::thread::spawn(move || {
                    start.wait();
                    session
                        .reserve_pairing(&Fence {
                            scope_id,
                            generation,
                        })
                        .is_ok()
                })
            })
            .collect();
        start.wait();
        assert_eq!(
            workers
                .into_iter()
                .filter_map(|t| t.join().ok())
                .filter(|ok| *ok)
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn late_native_snapshot_cannot_regress_poll_library_revision_or_release_old_art() {
    let s = server().await;
    let f = connect(&s).await;
    s.fixture.snapshot_hold.store(true, Ordering::SeqCst);
    let session = s.session.clone();
    let fence = f.clone();
    let media = tokio::spawn(async move {
        session
            .request(
                &fence,
                ControllerRequest::Media {
                    reference: ArtworkReference {
                        resource_id: "old".into(),
                        revision: 1,
                    },
                },
            )
            .await
    });
    s.fixture.snapshot_entered.notified().await;
    s.fixture.poll_library.store(true, Ordering::SeqCst);
    s.fixture.poll_release.notify_one();
    assert!(s
        .session
        .request(
            &f,
            ControllerRequest::Poll {
                cursor: EventCursor {
                    host_epoch: "epoch".into(),
                    revision: 0
                }
            }
        )
        .await
        .is_ok());
    s.fixture.snapshot_release.notify_one();
    assert!(matches!(
        media.await.unwrap(),
        Err(ControlError {
            code: ControlErrorCode::RevisionConflict,
            ..
        })
    ));
    assert_eq!(
        s.session
            .inner
            .lock()
            .unwrap()
            .active
            .as_ref()
            .unwrap()
            .library
            .load(Ordering::SeqCst),
        2
    );
}

#[tokio::test]
async fn actual_tls_grants_do_not_infer_control_from_ready_snapshot_or_administration() {
    let s = server().await;
    s.fixture.control.store(false, Ordering::SeqCst);
    let mut f = pair(&s).await; f.generation += 1;
    let (snapshot, grants) = s.session.connect(s.store.clone(), s.fixture.host.clone(), f.clone()).await.unwrap();
    assert_eq!(snapshot.host_id, s.fixture.host); assert!(!grants.control);
    s.fixture.omit_grants.store(true, Ordering::SeqCst); f.generation += 1;
    assert!(s.session.connect(s.store.clone(), s.fixture.host.clone(), f).await.is_err());
}
#[test]
fn endpoint_update_is_a_closed_connection_operation() {
    let value = serde_json::json!({"type":"update_endpoint","hostId":"host","endpoint":"192.168.1.9:1234","fence":{"scopeId":"scope","generation":1}});
    let ConnectionRequest::UpdateEndpoint {
        host_id,
        endpoint,
        fence,
    } = serde_json::from_value::<ConnectionRequest>(value.clone()).unwrap()
    else {
        panic!()
    };
    assert_eq!(host_id, "host");
    assert_eq!(endpoint, "192.168.1.9:1234");
    assert_eq!(fence.generation, 1);
    let mut forbidden = value;
    forbidden["ca"] = serde_json::json!([1, 2]);
    assert!(serde_json::from_value::<ConnectionRequest>(forbidden).is_err());
}
#[tokio::test]
async fn endpoint_update_authenticates_preserves_identity_and_survives_scope_restart() {
    let s = server().await;
    let mut f = connect(&s).await;
    let original: serde_json::Value =
        serde_json::from_slice(&s.store.load(&s.fixture.host).unwrap().unwrap()).unwrap();
    f.generation += 1;
    s.session
        .update_endpoint(
            s.store.clone(),
            s.fixture.host.clone(),
            format!("192.168.1.9:{}", s.endpoint.port()),
            f.clone(),
        )
        .await
        .unwrap();
    let mut expected = original;
    expected["endpoint"] = serde_json::json!(format!("192.168.1.9:{}", s.endpoint.port()));
    let saved: serde_json::Value =
        serde_json::from_slice(&s.store.load(&s.fixture.host).unwrap().unwrap()).unwrap();
    assert_eq!(saved, expected);
    f = Fence {
        scope_id: s.session.begin_scope().unwrap(),
        generation: 1,
    };
    assert!(s
        .session
        .connect(s.store.clone(), s.fixture.host.clone(), f)
        .await
        .is_ok());
}
#[tokio::test]
async fn endpoint_update_rejects_wrong_ca_and_interruption_without_persisting_candidate() {
    let s = server().await;
    let mut f = connect(&s).await;
    let original = s.store.load(&s.fixture.host).unwrap().unwrap().to_vec();
    s.fixture.snapshot_hold.store(true, Ordering::SeqCst);
    f.generation += 1;
    let (session, store, host, fence) = (
        s.session.clone(),
        s.store.clone(),
        s.fixture.host.clone(),
        f.clone(),
    );
    let address = format!("192.168.1.9:{}", s.endpoint.port());
    let pending =
        tokio::spawn(async move { session.update_endpoint(store, host, address, fence).await });
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        s.fixture.snapshot_entered.notified(),
    )
    .await
    .unwrap();
    f.generation += 1;
    s.session.suspend(&f).unwrap();
    assert!(pending.await.unwrap().is_err());
    s.fixture.snapshot_release.notify_one();
    assert_eq!(
        s.store.load(&s.fixture.host).unwrap().unwrap().as_slice(),
        original
    );
    assert!(s.session.inner.lock().unwrap().active.is_none());

    let other = server().await;
    let endpoint = other.endpoint;
    let wrong = Arc::new(NativeSession {
        inner: Mutex::default(),
        connector: Arc::new(move |p, a| {
            crate::controller::client::tests::loopback_transport(p, a, endpoint)
        }),
    });
    let f = Fence {
        scope_id: wrong.begin_scope().unwrap(),
        generation: 1,
    };
    assert!(wrong
        .update_endpoint(
            s.store.clone(),
            s.fixture.host.clone(),
            format!("192.168.1.9:{}", endpoint.port()),
            f
        )
        .await
        .is_err());
    assert!(other.fixture.requests.lock().unwrap().is_empty());
    assert_eq!(
        s.store.load(&s.fixture.host).unwrap().unwrap().as_slice(),
        original
    );
}
#[tokio::test]
async fn endpoint_update_rejects_urls_public_hosts_and_non_ipv4_without_effects() {
    let s = server().await;
    let f = connect(&s).await;
    let original = s.store.load(&s.fixture.host).unwrap().unwrap().to_vec();
    for endpoint in [
        "https://192.168.1.9:2345",
        "pc.local:2345",
        "8.8.8.8:2345",
        "[::1]:2345",
        "192.168.1.9:0",
    ] {
        let mut next = f.clone();
        next.generation += 1;
        assert!(s
            .session
            .update_endpoint(
                s.store.clone(),
                s.fixture.host.clone(),
                endpoint.into(),
                next
            )
            .await
            .is_err());
        assert!(s.session.check(&f).is_ok());
    }
    assert_eq!(
        s.store.load(&s.fixture.host).unwrap().unwrap().as_slice(),
        original
    );
}
#[tokio::test]
async fn endpoint_update_cancels_old_poll_and_does_not_adopt_failed_persistence() {
    struct FailSave(Arc<Memory>);
    impl ControllerStore for FailSave {
        fn load(&self, host: &str) -> Result<Option<Zeroizing<Vec<u8>>>, ControlError> {
            self.0.load(host)
        }
        fn save(&self, _: &str, _: &[u8]) -> Result<(), ControlError> {
            Err(transport_error(ControlErrorCode::HostNotReady))
        }
        fn delete(&self, host: &str) -> Result<(), ControlError> {
            ControllerStore::delete(&*self.0, host)
        }
    }
    let s = server().await;
    let mut f = connect(&s).await;
    let original = s.store.load(&s.fixture.host).unwrap().unwrap().to_vec();
    let session = s.session.clone();
    let old = f.clone();
    let poll = tokio::spawn(async move {
        session
            .request(
                &old,
                ControllerRequest::Poll {
                    cursor: EventCursor {
                        host_epoch: "epoch".into(),
                        revision: 0,
                    },
                },
            )
            .await
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        s.fixture.poll_entered.notified(),
    )
    .await
    .unwrap();
    f.generation += 1;
    assert!(s
        .session
        .update_endpoint(
            Arc::new(FailSave(s.store.clone())),
            s.fixture.host.clone(),
            format!("192.168.1.9:{}", s.endpoint.port()),
            f
        )
        .await
        .is_err());
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(5), poll)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    assert!(s.session.inner.lock().unwrap().active.is_none());
    assert_eq!(
        s.store.load(&s.fixture.host).unwrap().unwrap().as_slice(),
        original
    );
}

#[tokio::test]
async fn browse_metadata_native_fences_and_bounds() {
    let s=server().await;let f=connect(&s).await;
    let reference=ArtworkReference{resource_id:"opaque-art".into(),revision:1};
    assert!(matches!(s.session.request(&f,ControllerRequest::Media{reference}).await,Ok(ControllerReply::Media{..})));
    let art_bytes=s.session.inner.lock().unwrap().cache.bytes;assert!(art_bytes>0);
    let request=BrowseMetadataRequest{metadata_version:1,host_epoch:"epoch".into(),library_revision:1,track_ids:vec![42],album_id:Some(8)};
    let reply=s.session.request(&f,ControllerRequest::BrowseMetadata{request:request.clone()}).await.unwrap();
    let ControllerReply::BrowseMetadata{result}=reply else{panic!()};assert!(result.matches(&request));
    assert_eq!(s.session.inner.lock().unwrap().cache.bytes,art_bytes);
    s.fixture.metadata_bad.store(true,Ordering::SeqCst);
    assert!(s.session.request(&f,ControllerRequest::BrowseMetadata{request:request.clone()}).await.is_err());
    assert_eq!(s.session.inner.lock().unwrap().cache.bytes,art_bytes);
    s.fixture.metadata_bad.store(false,Ordering::SeqCst);s.fixture.metadata_large.store(true,Ordering::SeqCst);
    assert!(matches!(s.session.request(&f,ControllerRequest::BrowseMetadata{request:request.clone()}).await,Err(ControlError{code:ControlErrorCode::TooLarge,..})));
    s.fixture.metadata_large.store(false,Ordering::SeqCst);
    s.fixture.query_hold.store(true,Ordering::SeqCst);let session=s.session.clone();let fence=f.clone();
    let held=tokio::spawn(async move{session.request(&fence,ControllerRequest::BrowseMetadata{request}).await});
    s.fixture.query_entered.notified().await;s.session.begin_scope().unwrap();
    assert!(matches!(held.await.unwrap(),Err(ControlError{code:ControlErrorCode::ResyncRequired,..})));
    s.fixture.query_release.notify_one();
}
#[tokio::test]
async fn browse_metadata_late_reply_discarded_after_suspend_or_host_switch() {
    for host_switch in [false,true] {
        let s=server().await;let f=connect(&s).await;
        s.fixture.query_hold.store(true,Ordering::SeqCst);
        let session=s.session.clone();let fence=f.clone();
        let held=tokio::spawn(async move{session.request(&fence,ControllerRequest::BrowseMetadata{request:BrowseMetadataRequest{metadata_version:1,host_epoch:"epoch".into(),library_revision:1,track_ids:vec![42],album_id:None}}).await});
        tokio::time::timeout(std::time::Duration::from_secs(5),s.fixture.query_entered.notified()).await.unwrap();
        let next=Fence{scope_id:f.scope_id,generation:f.generation+1};
        if host_switch {
            assert!(s.session.connect(s.store.clone(),uuid::Uuid::new_v4().to_string(),next).await.is_err());
        } else {s.session.suspend(&next).unwrap();}
        assert!(matches!(held.await.unwrap(),Err(ControlError{code:ControlErrorCode::ResyncRequired,..})));
        s.fixture.query_release.notify_one();assert!(s.session.inner.lock().unwrap().active.is_none());
    }
}

#[tokio::test]
async fn browse_metadata_missing_route_isolated_and_revocation_is_terminal() {
    let s=server().await;let f=connect(&s).await;
    s.session.request(&f,ControllerRequest::Media{reference:ArtworkReference{resource_id:"opaque-art".into(),revision:1}}).await.unwrap();
    let before=s.session.inner.lock().unwrap().cache.bytes;assert!(before>0);
    let request=BrowseMetadataRequest{metadata_version:1,host_epoch:"epoch".into(),library_revision:1,track_ids:vec![42],album_id:None};
    s.fixture.metadata_missing.store(true,Ordering::SeqCst);
    assert!(matches!(s.session.request(&f,ControllerRequest::BrowseMetadata{request:request.clone()}).await,Err(ControlError{code:ControlErrorCode::NotFound,..})));
    assert!(s.session.inner.lock().unwrap().active.is_some());assert_eq!(s.session.inner.lock().unwrap().cache.bytes,before);
    s.fixture.metadata_missing.store(false,Ordering::SeqCst);s.fixture.query_hold.store(true,Ordering::SeqCst);
    let session=s.session.clone();let fence=f.clone();let held=tokio::spawn(async move{session.request(&fence,ControllerRequest::BrowseMetadata{request}).await});
    tokio::time::timeout(std::time::Duration::from_secs(5),s.fixture.query_entered.notified()).await.unwrap();
    s.fixture.revoked.store(true,Ordering::SeqCst);s.fixture.query_release.notify_one();
    assert!(matches!(held.await.unwrap(),Err(ControlError{code:ControlErrorCode::Unauthorized,..})));
    assert!(s.session.inner.lock().unwrap().active.is_none());assert_eq!(s.session.inner.lock().unwrap().cache.bytes,0);
}
