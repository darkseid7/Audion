# Android Desktop Controller Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the Android APK into a responsive LAN controller of the running desktop, including PC-library album playback on PC or Squeeze output and audited built-in operation coverage.

**Architecture:** Shared presentation uses one typed application interface with desktop and controller adapters. Desktop execution remains in existing Rust and TypeScript implementations behind a result-aware coordinator; Android uses a native authenticated HTTPS client and passive projections. A dedicated desktop listener provides pairing, commands, queries, resources and revision-based long-poll events independently of Squeeze.

**Tech Stack:** Svelte 5, TypeScript, Tauri 2, Rust, Tokio, Axum, existing reqwest/rustls, Vitest, Android Kotlin, AndroidKeyStore and embedded ZXing.

**Spec:** [Approved design](../specs/2026-10-02-android-desktop-controller-design.md).

**Status:** Plan awaiting review and execution-method selection. Design approved by the user on 2026-10-02. No product implementation is authorized by this document alone. Baseline: `779503b`, incorporating `d9409cc` album views and earlier Squeeze fixes.

## Global Constraints

- The controller connection is LAN-only: no cloud relay, account requirement, router port forwarding, or automatic port mapping.
- Android never becomes an audio output, never copies the music library, and never takes over playback when the PC is unavailable.
- Android startup must not initialize a music database, scanner, watcher, audio engine, Squeeze server, plugin runtime, cloud synchronization, scrobbling, playback timers, or restoration of a local queue.
- The desktop owns domain execution; local controller preferences and bounded display caches are non-authoritative.
- Invitations expire after five minutes; completed command results remain available for at least five minutes.
- A long poll responds immediately when events arrive and ends after at most 25 seconds when idle.
- Only one poll is outstanding per controller; queries and commands remain independent of it.
- Switching output stops the previous owned output before confirming the new selection. Failed or unknown stops never start the new target.
- No universal plugin parity, seamless output handoff, or exactly-once execution across crashes is promised.
- Preserve desktop behavior, ordered Squeeze shutdown, album grid/list and reduced-motion behavior. Artifact language is English.
- Use conventional local commits without AI attribution. No push. Stage explicit task files only and preserve unrelated changes.
- Create an isolated checkout from the selected plan commit at execution time using the worktree skill; do not start from an older remote default branch.
- Run tests red then green for each task. Missing SDK, signing credentials, hardware, or OS secret storage is a reported release blocker, never permission to fake verification.

## Review Focus

1. Legacy Android queue, active timer and cloud-login storage must cause zero domain work before or after controller bootstrap; Task 2 tests import-time behavior.
2. Repeated bootstrap and coordinator reload must leave one owner and ignore old-epoch completions; Tasks 2, 6 and 7 test lease rotation.
3. Track completion, gapless advance and timer expiry racing output selection must not deadlock, double-advance or double-scrobble; Tasks 3 and 13 test the shared execution lane.
4. Duplicate tracks and changing pagination revisions must not mix snapshots or mutate the wrong queue occurrence; Tasks 3, 8 and 10 test entry identity.
5. Old query, poll and artwork completions after resume or PC switch must not overwrite the new session; Task 9 tests generation ownership and cache disposal.

## Execution map

Tasks 1 through 11 form the first working delivery: paired phone/tablet, PC library and artwork, album/queue controls and PC/Squeeze output. Verify it on hardware before extending parity in Tasks 12 through 16. Until those later tasks are implemented, their controller capabilities stay unavailable: waveform shows a non-decoding placeholder, and lyrics, integrations and administration never fall back to local execution. Task 17 packages and audits the complete supported release; the first delivery must not be called full parity.

All paths below are repository-relative to `C:/Users/olive/Documents/GitHub/Audion`. For compact file lists, components/, stores/, services/, plugins/ and application/ mean subdirectories of src/lib; desktop/ and controller/ TypeScript names mean subdirectories of src/lib/application. Rust controller/ and commands/ names mean src-tauri/src/controller and src-tauri/src/commands; Cargo.toml/Cargo.lock/build.rs/lib.rs and capabilities/ mean their src-tauri paths. Brace lists expand to the exact individual files named. New code belongs together in `src/lib/application/` and `src-tauri/src/controller/`; existing view imports retain compatibility through passive facades. Keep `src/lib/api/tauri.ts` as the desktop low-level binding, not a remote-invoke dispatcher.

| Files | Responsibility |
| --- | --- |
| application/types.ts, protocol.ts, port.ts, mode.ts | Shared typed interface, validation and installed adapter |
| application/bootstrap.ts, stores/playback-state.ts, existing store facades | Passive state and explicit role startup |
| application/desktop/player-runtime.ts, playback-coordinator.ts, adapter.ts | Existing desktop execution and serial intent handling |
| application/desktop/state-publisher.ts | Confirmed desktop projections and coordinator lease |
| application/controller/adapter.ts, session.ts, media.ts | Native delegation, reconnect ownership and display resources |
| application/library.ts, view-actions.ts, capabilities.ts | Shared queries, view intents and capability checks |
| controller/protocol.rs, identity.rs, pairing.rs, secrets.rs | Wire types, TLS identity, grants and protected keys |
| controller/host.rs, commands.rs, events.rs, queries.rs, resources.rs | Dedicated LAN transport, execution ledger, replay and projected reads |
| controller/client.rs, mobile.rs, commands/controller.rs | Host-bound native client and narrow Tauri bridge |
| ControllerNativePlugin.kt, ControllerSecrets.kt, ControllerCaptureActivity.kt | Native credential protection and QR scanning |
| application/desktop/domain.ts, controller/jobs.rs | Existing domain adapters and long-running jobs |
| application/desktop/plugin-remote.ts, application/plugin-view.ts | Host-only plugins and declarative remote presentation |

## Shared contract decisions

Task 1 owns TypeScript contracts in `application/types.ts` and Rust equivalents in `controller/protocol.rs`; subsequent tasks extend their closed discriminated unions, not arbitrary command strings. JSON uses camelCase fields and snake_case intent names. Shared fixture files under `tests/fixtures/controller/` prove both implementations accept the same valid payloads and reject invalid ones.

- `ApplicationMode = "desktop" | "controller"`; role comes from native build/platform, never `activeBackend`, screen width or localStorage.
- `ApplicationPort.query(query: ApplicationQuery, signal?: AbortSignal): Promise<QueryResult>`.
- `ApplicationPort.execute(intent: ApplicationIntent, preconditions: CommandPreconditions): Promise<ExecutionResult>`.
- `ApplicationPort.subscribe(listener: (update: ApplicationUpdate) => void): () => void`.
- `ApplicationPort.resolveArtwork(reference: ArtworkReference, signal?: AbortSignal): Promise<ArtworkHandle>`.
- `CommandPreconditions = {hostEpoch: string; queueRevision?: number; libraryRevision?: number; outputRevision?: number}`.
- `CommandEnvelope = {protocolVersion: 1; requestId: string; preconditions: CommandPreconditions; intent: ApplicationIntent}`.
- `ExecutionResult` is a union: `{status:"applied"; revision:number}`, `{status:"accepted"; jobId:string; revision:number}`, `{status:"failed"|"superseded"; error:ControlError; revision:number; partialEffects:string[]}`. Network errors are separate: a timeout is not a completed failure.
- `ControlError = {code: string; message: string; retryable: boolean}`, with an enumerated code allowlist from the spec plus `outcome_unknown` and `resync_required`.
- `OutputRef = {kind:"pc"} | {kind:"squeeze"; playerId:string}`. Snapshot output additionally permits `{kind:"desktop_only"; reason:string}` for legacy desktop cloud ownership; controller mutations there fail explicitly until a host-owned output is selected on the PC.
- `DisplayTrack` exposes ID, title, artist, album ID/name, duration, track/disc numbers, format/quality summary and optional `ArtworkReference`, never path, local source, token or provider stream URL. `DisplayAlbum` includes PC-computed quality badges and sort summary; retain the existing grid/list `AlbumView` type unchanged. Desktop domain code retains its original `Track`.
- `QueueEntry = {entryId:string; track:DisplayTrack}`; entry identity distinguishes repeated occurrences of a track. `Page<T> = {items:T[]; nextCursor:string|null; revision:number}`.
- `HostSnapshot` includes hostId, hostEpoch, event revision, domain revisions, playback/context, queue summary, output, capabilities, settings projections and job summaries. `ApplicationUpdate` is a snapshot or ordered event batch. `ArtworkReference = {resourceId:string; revision:number}`; `ArtworkHandle = {src:string; dispose():void}`.

Initial closed queries: `snapshot`, `albums`, `album_tracks`, `tracks`, `artists`, `artist_albums`, `search`, `queue`, `outputs`. Initial intents: `play_album`, `play_playlist`, `play_artist`, `play_liked`, `play_track`, `select_output`, `pause`, `resume`, `next`, `previous`, `seek`, `set_volume`, `set_shuffle`, `set_repeat`, `queue_insert`, `queue_append`, `queue_remove`, `queue_reorder`, `queue_clear_upcoming`, `queue_play`. Queue mutations reference entry IDs and queueRevision, not phone array indices. Entity playback references PC IDs and libraryRevision; the desktop resolves order, filters and shuffle. Seek uses finite seconds; volume uses finite slider values in [0,1].

Chosen implementation bounds: 200 items/page, 256 retained events, 1 MiB command body, 2 KiB encoded invitation, 5 MiB artwork, 50 MiB native artwork cache, 32 paired devices, 4 pending pairing approvals, 1 active pairing attempt per device and 10 attempts/minute/source address. Allow one active mutation per device, at most 32 pending mutations globally, 4 concurrent queries per device and one event poll; duplicates share the existing result rather than consuming another execution slot. Reserve at most 256 ledger entries per device, retain payload fingerprints rather than completed request bodies, and reject new requests as busy instead of evicting unexpired results. Controller-submitted jobs permit 4 running and 16 pending globally. Return explicit size/rate/busy errors, never silently truncate commands. Build compact certificate-bearing invitations within the QR bound; scanning and paste use the same validator. Command-result expiry must not evict active jobs. Use a configurable separate listener port defaulting to 9010; fail visibly on collision rather than borrowing Squeeze port 9000.

## Task 1 Define the shared interface and role contract

**Files:** Create `src/lib/application/{types,protocol,port,mode}.ts`, `src/lib/application/protocol.test.ts`, `src-tauri/src/controller/{mod,protocol}.rs`, and `tests/fixtures/controller/{valid,invalid}.json`. Modify `src-tauri/src/lib.rs` only to declare the module.

**Interfaces:** Produce the shared contracts above, `resolveApplicationMode(platform:string):ApplicationMode`, `parseEnvelope(value:unknown):CommandEnvelope`, `createUnavailablePort():ApplicationPort`, `installApplicationPort(port:ApplicationPort):()=>void`, and `getApplicationPort():ApplicationPort`. The unavailable port rejects queries/commands, never invokes native domain functions, and exposes no usable capabilities. Rust `CommandEnvelope` validates the same fields with strict serde enums. Define the named query/intent/result/state types in the shared contract table; later tasks add their stated union variants. Android is controller, Windows/macOS/Linux are desktop, and unknown platforms fail closed.

- [ ] Before product changes, capture baseline Vitest, frontend check and Rust test results in the isolated checkout; retain failures for Task 17 comparison.
- [ ] Write `rejects_invalid_role_and_unsafe_wire_fields`, `port_never_falls_back` and cross-language fixture tests. Include these assertions:
  ```ts
  expect(resolveApplicationMode("android")).toBe("controller");
  expect(() => parseEnvelope({ command: "audio_play", path: "C:/Music/a.flac" })).toThrow();
  expect(() => getApplicationPort()).toThrow("Application not ready");
  ```
- [ ] Run `npx vitest run src/lib/application/protocol.test.ts` and `cargo test --manifest-path src-tauri/Cargo.toml controller::protocol`; expect failures because the new modules/validators are absent.
- [ ] Implement the contracts, immutable role selection and generation-aware adapter installation. Type-only desktop DTO imports are allowed; wire projections omit host paths. Add explicit missing-field/range/unknown-intent rejection.
- [ ] Repeat both commands; expect zero failing tests. Test every initial intent fixture and compile its equivalent Rust representation.
- [ ] Commit only task files with `feat(controller): define application and wire contracts`.

## Task 2 Isolate passive state and role startup

**Files:** Create `src/lib/application/{bootstrap.ts,bootstrap.test.ts}`, `src/lib/application/desktop/{bootstrap,player-runtime}.ts`, `src/lib/application/controller/bootstrap.ts`, `src/lib/stores/playback-state.ts`, and `src-tauri/src/controller/bootstrap.rs`. Modify `src/routes/{+layout,+page}.svelte`, `src/lib/stores/{player,squeeze,websocket,sleepTimer,equalizer,lyrics,settings,persist,pinned,customArtwork,playlistCovers,plugin-store}.ts`, components/WaveformSeekBar.svelte, LyricsPanel.svelte, DesktopHome.svelte, MobileHome.svelte, `src-tauri/src/lib.rs`, and the existing player/Squeeze/timer tests.

**Interfaces:** Consume Task 1. Produce `bootstrapApplication(mode:ApplicationMode, loaders:BootstrapLoaders):Promise<ApplicationHandle>`, where each loader returns `{port:ApplicationPort; dispose():Promise<void>}`; handle disposal is owner-aware. Export one playback-state singleton with read-only view subscriptions and controlled internal projection writes. Desktop facade actions delegate intents; no facade statically imports a desktop runtime. Native setup returns an immutable application mode before activating either loader.

- [ ] Write `controller_import_and_bootstrap_ignore_poisoned_legacy_storage`: install spies before dynamic imports with old queue, timer and cloud-login records, then assert `expect(forbiddenDomainCalls).toEqual([])` both before and after bootstrap. Spy on native invokes, Audio/AudioContext/WebSocket constructors, plugin loading and domain intervals. Add `old_dispose_cannot_uninstall_new_owner`.
- [ ] Run `npx vitest run src/lib/application/bootstrap.test.ts`; expect existing import-time timers/subscriptions or absent bootstrap to fail.
- [ ] Move existing player implementation without algorithm changes; update relative imports and preserve pluginEvents identity. Make related store initialization explicit and passive in controller role. Separate controller and desktop Rust setup/handler lists: only desktop creates Database, audio, watcher, sync, scanner, backups and Squeeze state. Preserve Android navigation; remove domain startup from shared lifecycle.
- [ ] Use the unavailable port for adapters not yet implemented; Task 3 installs the real desktop adapter and Task 9 installs the controller adapter. Gate unfinished controller capabilities before component effects run, including waveform decoding and home-view provider fetches; preserve their desktop behavior. Migrate existing runtime regression tests to exercise the relocated implementation directly. Do not ship intermediate unavailable-mode commits as a working release.
- [ ] Run bootstrap tests plus `npx vitest run src/lib/stores/player.squeeze.test.ts src/lib/stores/squeeze.test.ts src/lib/stores/sleepTimer.test.ts`; expect zero regressions. Verify controller disposal does not invoke desktop cleanupPlayer/native stop.
- [ ] Commit `refactor(controller): isolate desktop runtime from shared views`.

## Task 3 Make desktop playback and output commands observable

**Files:** Create `src/lib/application/desktop/{playback-coordinator.ts,playback-coordinator.test.ts,adapter.ts}`. Modify desktop/bootstrap.ts, desktop/player-runtime.ts, stores/playback-state.ts, the desktop Squeeze implementation isolated by Task 2, and existing player/Squeeze tests.

**Interfaces:** Consume initial intents and state. Produce `createPlaybackCoordinator(runtime:DesktopPlaybackRuntime, state:HostStateAccess):PlaybackCoordinator`, with `execute(intent:PlaybackIntent, context:CommandPreconditions):Promise<ExecutionResult>`, `enqueueSignal(signal:PlaybackSignal):Promise<void>`, and `dispose():Promise<void>`. Runtime owns `resolvePlayback(intent):Promise<ResolvedPlayback>`, `stopOwnedOutput(output:OutputRef):Promise<RuntimeResult>`, `selectOutput(output:OutputRef):Promise<RuntimeResult>`, and `apply(intent:PlaybackIntent):Promise<RuntimeResult>`. RuntimeResult is applied, failed or superseded with structured error; ResolvedPlayback holds host Track[], context and starting index. HostStateAccess reads current revisions and commits confirmed state; PlaybackSignal carries completion/gapless/timer kind and owning output/playback generation.

- [ ] Add awaited-stop and caught-error tests. Use ordered fake runtime calls and assert `expect(calls).toEqual(["stop:pc","select:squeeze:A"])`; when stop rejects, assert `expect(selectOutput).not.toHaveBeenCalled()`. Test same-output reselect does not stop, Squeeze A to B, stale output revision, repeated queue IDs, and gapless/timer race with exactly one advancement/scrobble.
- [ ] Run `npx vitest run src/lib/application/desktop/playback-coordinator.test.ts`; expect absent result-aware coordinator or current fire-and-forget behavior to fail.
- [ ] Serialize user and host-generated signals in one lane. Internal runtime helpers never reenter the public coordinator. Await playTracks/playTrack/next and Squeeze operations; map stale outcomes instead of swallowing errors. Validate targets before old-output stop, then commit only after acknowledgement. Preserve queue/context/shuffle and existing Squeeze ownership/exit safeguards; disclose partial effects.
- [ ] Run coordinator and existing player/Squeeze tests; expect zero failures. Legacy desktop cloud controls remain desktop-only, not controller mode or silent PC fallback.
- [ ] Commit `feat(controller): coordinate confirmed desktop playback and output`.

## Task 4 Add host identity pairing and protected credentials

**Files:** Create `src-tauri/src/controller/{identity,pairing,secrets}.rs` with inline tests and `src/lib/components/LanControllerSettings.svelte`. Modify Cargo.toml/Cargo.lock, components/Settings.svelte and `packaging/flatpak/com.audion.app.yml`.

**Interfaces:** Produce `load_or_create_identity(store:&dyn SecretStore)->Result<HostIdentity,ControlError>`, `create_invitation(identity:&HostIdentity,endpoint:SocketAddr,now:SystemTime)->Result<PairingInvitation,ControlError>`, `request_pairing(invite:PairingInvitation,device_name:String)->Result<PendingPairing,ControlError>`, `approve_pairing(id:Uuid,grants:Grants)->Result<NativeCredential,ControlError>`, and `revoke_device(id:Uuid)->Result<(),ControlError>`. SecretStore reads/writes/deletes protected bytes; HostIdentity holds ID, public CA, protected keys and leaf; grants distinguish control and administration. NativeCredential never enters frontend DTOs.

- [ ] Write `invitation_expires_after_300_seconds`, `concurrent_consume_issues_once`, `revocation_is_device_scoped`, `lost_issuance_requires_new_pairing`, and `unavailable_vault_keeps_host_disabled`. Assert `assert_eq!(invite.expires_at, now + Duration::from_secs(300))`; only one concurrent consume may succeed and logs contain no invitation/token.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml controller::pairing` and `cargo test --manifest-path src-tauri/Cargo.toml controller::identity`; expect missing implementations to fail.
- [ ] Add axum-server 0.8.0 with tls-rustls-no-provider, rustls 0.23 ring, rcgen 0.14.7, qrcode 0.14.1 and target-specific keyring 3.6.3. Generate private CA and leaf SAN `audion-<hostId>.invalid`; renew leaf under the same CA, require new pairing on CA replacement. Use 256-bit random invitation/device secrets, store device hashes and atomically consume invitations. Persist approvals without retaining raw credentials; an issuance outcome lost to the client requires a fresh pairing, not reusable invitation. LanControllerSettings renders the initially off switch, private interface/port, QR/copy invitation, pending approval/grants and revoke controls through Task 6 host commands; disable unfinished controls until those handlers exist.
- [ ] Repeat tests and add real certificate validation for same-CA renewal/expiry. Enable explicit keyring Windows/macOS/Linux features and Flatpak Secret Service permission; locked/missing vault keeps listener disabled with no mock/plaintext fallback.
- [ ] Commit `feat(controller): secure desktop identity and device pairing`.

## Task 5 Add the Android native trust and credential module

**Files:** Create `src-tauri/src/controller/{client,mobile}.rs`, `src-tauri/gen/android/app/src/main/java/com/audion/app/{ControllerNativePlugin,ControllerSecrets,ControllerCaptureActivity}.kt`, `src-tauri/gen/android/app/src/test/java/com/audion/app/ControllerSecretsEnvelopeTest.kt`, `src-tauri/gen/android/app/src/androidTest/java/com/audion/app/{ControllerSecretsTest,ControllerQrTest}.kt`, and `src-tauri/gen/android/buildSrc/src/test/java/com/audion/app/kotlin/BuildTaskTest.kt`. Modify lib.rs, capabilities/mobile.json, `src-tauri/gen/android/app/src/main/java/com/audion/app/MainActivity.kt`, `src-tauri/gen/android/app/src/main/AndroidManifest.xml`, `src-tauri/gen/android/app/build.gradle.kts`, `src-tauri/gen/android/buildSrc/build.gradle.kts`, and `src-tauri/gen/android/buildSrc/src/main/java/com/audion/app/kotlin/BuildTask.kt`; remove the unused manifest FileProvider and `src-tauri/gen/android/app/src/main/res/xml/file_paths.xml`.

**Interfaces:** Produce `build_host_client(pairing:&NativePairing)->Result<reqwest::Client,ControlError>` and native `saveCredentials(hostId:String,bytes:ByteArray)`, `loadCredentials(hostId:String):ByteArray?`, `deleteCredentials(hostId:String)`, `scanInvitation():String?`. Kotlin @Command methods accept Invoke and decode these payloads; Rust invokes them through the plugin handle. The inline controller-native plugin rejects frontend invocation in its Rust handler and grants no WebView access; only Rust can invoke native credential/scan methods. NativePairing contains validated private endpoint, host ID, CA and credential. Expose scanning through a scoped `controller_scan_pair` Rust app command returning pairing status, never credentials.

- [ ] Write `client_rejects_wrong_ca_name_or_expiry` and `endpoint_change_rebuilds_trust_bound_pool`. Native `roundTrip`, `tamperedEnvelopeRequiresPairing`, `crossHostDecryptRejected` and `missingKeyRequiresPairing` instrumentation tests assert `assertArrayEquals(secret, recovered)` only for the original host and fail closed for ciphertext/IV/AAD changes or missing keys. Assert direct frontend `invoke("plugin:controller-native|loadCredentials")` is denied. Add QR cancel/permission denial/rotation with paste fallback.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml controller::client`; expect missing client behavior to fail. Keep pure client/protocol validation desktop-testable while the Android plugin bridge is target-gated.
- [ ] Configure JDK 17, SDK 36, Build Tools 35.0.0, pinned NDK 27.0.12077973 and Rust Android targets only at execution time; record a prerequisite failure instead of treating it as a feature-test failure.
- [ ] Write `runnerUsesNodeCliAndPreservesTargetArguments` and `debugConfigDoesNotRequireSigningProperties` in BuildTaskTest.kt; run the buildSrc test/Gradle configuration fixtures and confirm current runner/signing failures before repairs.
- [ ] Repair BuildTask to invoke the installed Node Tauri CLI instead of unavailable cargo-tauri and make debug configuration work without signing properties; repeat the runner/configuration fixtures before native feature tests.
- [ ] Prepare fresh same-source JNI output using `npm run tauri -- android build --debug --apk true --aab false --target aarch64 --ci`. Do not rerun android init over custom files blindly.
- [ ] Run `.\gradlew.bat :app:testUniversalDebugUnitTest -x :app:rustBuildUniversalDebug` and `.\gradlew.bat :app:connectedUniversalDebugAndroidTest -x :app:rustBuildUniversalDebug` from gen/android; expect native tests to fail for absent behavior, not toolchain/signing errors. Exclusion requires freshly built JNI from exactly the tested source.
- [ ] Configure AndroidKeyStore AES-256-GCM with fresh IV, 128-bit tag and hostId/schema AAD; keep encrypted data in noBackupFilesDir. Add ZXing embedded 4.3.0, optional camera and unlocked orientation; scanned/pasted input enters the same bounded native validator. Trust only the paired CA, enforce TLS name/validity, HTTPS-only/private address, no proxy/redirect, explicit URL port, and rebuild pools after endpoint changes.
- [ ] Remove existing audio/folder JavaScript bridges, media/storage/notification startup permission requests and media playback service registration. Keep INTERNET; request CAMERA only for optional QR. Make release signing configuration conditional so unsigned debug tests work without key.properties, and set AndroidJUnitRunner.
- [ ] Rebuild native output after source changes and repeat all tests with the verified same-source JNI exclusion; alternatively keep the Tauri Android Studio options server alive while Gradle invokes RustBuild. Standalone Gradle RustBuild must not depend on an expired CLI options server. Missing physical devices remain reported rather than marked passed. Commit `feat(controller): add native Android pairing and secure transport`.

## Task 6 Expose the authorized LAN command interface

**Files:** Create `src-tauri/src/controller/{host,commands}.rs`, `src-tauri/src/commands/controller.rs`, `src-tauri/permissions/controller.toml`, `src/lib/application/desktop/bridge.ts` and bridge.test.ts. Modify commands/mod.rs, build.rs, lib.rs, capabilities/default.json, capabilities/mobile.json, desktop/bootstrap.ts and LanControllerSettings.svelte.

**Interfaces:** Consume Tasks 3 through 5. Produce async `start_host(config:LanConfig,deps:HostDependencies)->Result<HostHandle,ControlError>`, async `submit(device:AuthenticatedDevice,envelope:CommandEnvelope)->Result<ExecutionResult,ControlError>`, `register_coordinator(window:AuthoritativeWindow)->Result<CoordinatorLease,ControlError>`, and `complete(lease:CoordinatorLease,ticket:ExecutionTicket,result:ExecutionResult)->Result<(),ControlError>`. This task owns `CoordinatorLease={leaseId:Uuid;hostEpoch:Uuid}` and server-generated `ExecutionTicket=Uuid`, bound to one authenticated device/request ID in the ledger. The TypeScript bridge receives `HostDispatch={ticket:string;envelope:CommandEnvelope}` and completes that exact ticket; window identity comes from Tauri context, never a caller-supplied name. Tauri commands are `control_host_enable`, `control_host_invitation`, `control_host_approve`, `control_host_revoke`, `control_host_register`, `control_host_complete`; mobile exposes only `controller_pair`, `controller_scan_pair`, `controller_request`, `controller_connection`, `controller_suspend`, `controller_forget`. Scope app command permissions per platform/window and reject host commands in mobile handler registration. Extend the same scoped controller_request union with poll/resource operations in Tasks 7–9; it never accepts arbitrary URLs or command strings.

- [ ] Write `duplicate_submits_execute_once`, `old_lease_cannot_complete_new_command`, `unknown_ticket_cannot_resolve_pending_request`, `bounded_command_admission_returns_busy`, `unexpired_ledger_capacity_returns_busy`, `unauthorized_routes_fail_closed` and `request_id_payload_reuse_is_rejected`. Assert `assert_eq!(execution_count, 1)` for concurrent duplicates, results survive 300 seconds, and unknown-ticket/old-epoch replies never complete new requests. Include non-private direct peer, unauthorized admin and missing coordinator.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml controller::commands` and `npx vitest run src/lib/application/desktop/bridge.test.ts`; expect failures.
- [ ] Bind a prebound private-interface socket with TLS. Routes under /control/v1 cover pairing, handshake, command submission/status, queries, events and resources; only invitation-authorized pairing status is pre-device-auth. Atomically reserve request IDs before execution; check grants/epoch/revisions again at dispatch. Correlate Rust requests with one authoritative window/lease, never globally broadcast authority or blindly invoke Tauri names.
- [ ] Repeat tests plus real TLS host/client loopback fixtures; test-only fixtures must not relax production private-IPv4 validation. Verify port collision, size/rate/concurrency limits and separate Squeeze listener remain intact.
- [ ] Commit `feat(controller): expose authorized LAN command dispatch`.

## Task 7 Publish authoritative snapshots and bounded events

**Files:** Create `src-tauri/src/controller/events.rs`, `src/lib/application/desktop/{state-publisher.ts,state-publisher.test.ts}`. Modify host.rs, commands.rs, desktop/bootstrap.ts, src-tauri/src/app_exit.rs and lib.rs.

**Interfaces:** Produce `capture_snapshot()->HostSnapshot`, `publish(lease:CoordinatorLease,update:HostUpdate)->Result<u64,ControlError>`, async `poll_events(device:AuthenticatedDevice,cursor:EventCursor)->Result<EventBatch,ControlError>` and `startStatePublisher(coordinator:PlaybackCoordinator):()=>void`. HostUpdate contains projections/domain invalidations; EventCursor contains epoch and revision. Rust owns replay ordering and lease validation.

- [ ] Write `snapshot_replay_has_no_gap`, `expired_history_requires_resync`, `idle_poll_finishes_at_25_seconds` and `reload_revokes_old_publisher`. With Tokio paused time assert a pending empty poll remains pending at 24 seconds and returns at 25; `assert_eq!(expired_cursor_error.code, "resync_required")` after 256-event overflow. Assert one publisher after repeated startup, immediate event wakeup, and revocation/exit cancellation.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml controller::events` and state-publisher Vitest tests; expect failures.
- [ ] Capture snapshot plus replay cursor atomically; coalesce position updates without sharing mutation preconditions with progress revision. Publish PC and Squeeze states, output capabilities and domain revisions. Rotate epoch and invalidate pending execution on coordinator reload. Cancel polls and mark host unavailable before existing ordered Squeeze exit cleanup; do not consume its shutdown budget.
- [ ] Repeat tests and existing app_exit/Squeeze Rust tests; verify no old publisher remains after teardown.
- [ ] Commit `feat(controller): publish revisioned desktop state and lifecycle`.

## Task 8 Serve PC library projections and authorized artwork

**Files:** Create `src-tauri/src/controller/{queries,resources}.rs`, `src/lib/application/{library.ts,library.test.ts}`. Modify host.rs, protocol.rs, types.ts, relevant db/queries.rs functions and desktop library invalidation entry points in commands/library.rs, scanner, watcher and sync modules.

**Interfaces:** Produce async `query_library(query:ApplicationQuery)->Result<QueryResult,ControlError>`, async `read_resource(device:AuthenticatedDevice,reference:ArtworkReference)->Result<MediaBytes,ControlError>`, and `queryLibrary(query:LibraryQuery,signal?:AbortSignal):Promise<QueryResult>`. MediaBytes includes allowed MIME and bytes, not a filesystem path. Add revisioned album/artist/search/queue projections and cursors to Task 1 unions.

- [ ] Write `unloaded_albums_have_host_badges_and_order`, `changing_revision_rejects_cursor` and `wire_projection_omits_private_sources`. Assert `assert!(serialized.get("path").is_none())` and the same for art_path/local_src/token/provider audio URL. Include duplicate queue tracks, deleted covers, unauthorized resource, oversize artwork and audio-file resource denial.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml controller::queries` and `controller::resources`, plus library.test.ts; expect failures.
- [ ] Reuse db::queries with bounded spawn_blocking work; never hold Database's connection mutex across awaits. Compute display summaries/order on PC, including albums not locally cached by Android. Hook scanner/watcher/sync/local desktop mutations into library invalidation; controller mutations alone are insufficient. Resources are registered opaque IDs and approved images fetched by the PC, not arbitrary URL/path proxying.
- [ ] Repeat tests with concurrent writes and revision-consistent pages; reject stale cursors instead of mixing results.
- [ ] Commit `feat(controller): expose projected PC library and artwork`.

## Task 9 Implement controller session and confirmed projections

**Files:** Create `src/lib/application/controller/{adapter.ts,session.ts,session.test.ts,media.ts,media.test.ts}` and `src/lib/components/ControllerConnection.svelte`. Modify controller/bootstrap.ts, native client/request commands and route startup.

**Interfaces:** Produce `createControllerAdapter(native:ControllerNativeBridge):ApplicationPort`, `connectController(hostId:string):Promise<void>`, `suspendController():void`, `forgetController(hostId:string):Promise<void>`. ControllerNativeBridge delegates only typed query/command/poll/media/pair operations; tokens never cross it. media.ts makes a Blob URL from bounded native-returned image bytes; ArtworkHandle.dispose revokes it. No general filesystem permission, credential-bearing URL or direct WebView-to-host request is needed.

- [ ] Write `offline_mutations_are_not_queued`, `snapshot_precedes_enabled_controls`, `timeout_queries_outcome_without_replay` and `old_session_results_cannot_replace_new_host`. Resolve an old media/query promise after PC switch and assert `expect(currentHostId).toBe(newHostId)` and `expect(oldHandle.dispose).toHaveBeenCalledOnce()`. Cover one poll and rejected old-epoch state.
- [ ] Run `npx vitest run src/lib/application/controller/session.test.ts src/lib/application/controller/media.test.ts`; expect failures.
- [ ] Start/restart a generation-owned session, replace confirmed state from snapshots and consume ordered batches. Use reconnect delays of 1,2,4,8 seconds capped at 8 with jitter; cancel on suspend/revocation. Native media uses host/resource-revision cache keys, a 50 MiB LRU bound and prompt Blob disposal on view/session teardown. Show PC unavailable, pairing, permission and protocol states; forget removes native credential and host-specific cache.
- [ ] Repeat tests with resume, host restart, certificate change, delayed replies and expired command results. Never restore phone queue/domain settings or call desktop cleanup on controller closure.
- [ ] Commit `feat(controller): mirror paired desktop sessions safely`.

## Task 10 Route shared playback views through typed intents

**Files:** Create `src/lib/application/{view-actions.ts,view-actions.test.ts,capabilities.ts}`. Modify components/AlbumDetail.svelte, AlbumGrid.svelte, ArtistDetail.svelte, PlaylistDetail.svelte, LikedSongs.svelte, TrackList.svelte, PlayerBar.svelte, MiniPlayer.svelte, FullScreenPlayer.svelte, QueuePanel.svelte, ConnectPanel.svelte, stores/library.ts and stores/search.ts; preserve existing component tests.

**Interfaces:** Produce `playAlbum(albumId:number,playMode:"all"|"liked_only",startTrackId?:number):Promise<ExecutionResult>` plus entity play helpers and `selectOutput(output:OutputRef):Promise<ExecutionResult>`. Preconditions derive exclusively from confirmed host projections. Queue helpers accept entry IDs; canExecute(capability:string) checks connection, grant and active-output capability.

- [ ] Test album clicks issue play_album with PC ID, liked_only flag and output/library revisions, not phone Track[]. Test direct volume/shuffle writes are replaced, queue duplicate selection references the right entry, and unsupported cloud target is visibly unavailable. Assert Android/native audio functions are never called.
- [ ] Run `npx vitest run src/lib/application/view-actions.test.ts src/lib/components/AlbumGrid.test.ts src/lib/components/ConnectPanel.test.ts`; expect current handlers or absent helpers to fail.
- [ ] Migrate all listed playback view actions and direct writable-store mutations. Host computes random start/shuffle. Label PC output with host name, omit Android/cloud output choices, and show acknowledged state/pending/errors. UI consumes DisplayTrack rather than host paths; formatting helpers remain pure.
- [ ] Repeat tests and perform the first real PC/tablet/Squeeze vertical workflow. A successful transport receipt is not the acceptance result; assert actual host queue and audio output.
- [ ] Commit `feat(controller): route shared playback UI to desktop intents`.

## Task 11 Adapt layouts without enabling desktop OS features

**Files:** Modify stores/mobile.ts, components/MainView.svelte, Sidebar.svelte, MobileBottomNav.svelte, TitleBar.svelte, GlobalShortcuts.svelte, KeyboardShortcuts.svelte, routes/+layout.svelte, routes/+page.svelte and app.css. Create stores/mobile.test.ts and application/controller-ui.test.ts.

**Interfaces:** Produce `selectLayout(width:number):"compact"|"expanded"`; use expanded at 768 CSS pixels and above while preserving mini-player behavior. Native window/titlebar/global-shortcut actions depend on ApplicationMode/capability, not layout.

- [ ] Add width/rotation and SSR tests: `expect(selectLayout(767)).toBe("compact")`, `expect(selectLayout(768)).toBe("expanded")`; expanded Android renders no PC titlebar or native shortcut registration. Preserve grid/list scroll continuity and zero-duration reduced-motion transition.
- [ ] Run mobile/controller-ui and existing album-view/virtualizer tests; expect current forced Android mobile detection to fail.
- [ ] Reuse existing tokens/views with compact navigation on phones and expanded panels on tablets. Respect safe-area/back navigation/touch and keyboard accessibility; avoid horizontal overflow and duplicate player initialization. Gate native PC controls independently of screen width.
- [ ] Repeat automated tests and real phone/tablet portrait/landscape/resize QA. Existing SSR tests are not proof of touch behavior.
- [ ] Commit `feat(controller): adapt shared phone and tablet layouts`.

## Task 12 Keep lyrics and waveform processing on the PC

**Files:** Create `src/lib/application/desktop/{lyrics-runtime.ts,waveform.ts}`, `src/lib/application/{lyrics.ts,lyrics.test.ts,waveform.test.ts}`. Modify stores/lyrics.ts, components/LyricsPanel.svelte, WaveformSeekBar.svelte, native lyrics command modules and shared protocol unions.

**Interfaces:** Add queries `lyrics` and `waveform`; waveform returns `{trackId:number;revision:number;duration:number;rms:number[];peak:number[]}` with at most 3000 bins each. Add lyrics source/change/import/delete intents with bounded text; `getWaveform(trackId:number,signal?:AbortSignal):Promise<WaveformProjection>` is shared and `buildWaveform(trackId:number):Promise<WaveformProjection>` is desktop-only.

- [ ] Write `controller_renders_prepared_lyrics_without_domain_fetch`, `controller_waveform_never_decodes_audio` and `malformed_lyrics_import_is_rejected_on_host`. Assert `expect(AudioContextSpy).not.toHaveBeenCalled()` and `expect(projection.rms.length).toBeLessThanOrEqual(3000)`; peaks share the bound and all samples are finite in [0,1]. Cover stale track replies.
- [ ] Run lyrics/waveform Vitest tests and relevant Rust lyrics tests; expect local domain loading/decoding behavior to fail.
- [ ] Detach provider/parser/cache execution into desktop runtime, preserving current formats and source selection. Move waveform decoding/caching to PC and return numeric projections only. Controller seeks by intent and renders prepared data; no music resource endpoint is added.
- [ ] Repeat tests with PC/Squeeze progress and source failures; highlight interpolation never triggers domain transitions.
- [ ] Commit `feat(controller): project desktop lyrics and waveforms`.

## Task 13 Separate PC settings timers and presentation preferences

**Files:** Create `src/lib/application/{settings.ts,settings.test.ts}` and desktop/settings-runtime.ts. Modify stores/settings.ts, sleepTimer.ts, equalizer.ts, persist.ts, theme.ts and components/Settings.svelte plus timer/EQ views.

**Interfaces:** Add `settings` query and `set_host_setting`, `set_equalizer`, `set_sleep_timer`, `cancel_sleep_timer` intents. `setHostSetting(key:HostSettingKey,value:HostSettingValue):Promise<ExecutionResult>` uses a closed validated mapping; controller preferences never use that namespace.

- [ ] Write `controller_preferences_do_not_mutate_pc`, `host_timer_survives_controller_suspend` and `unsupported_output_eq_fails_explicitly`. Assert `expect(timerExpiryCount).toBe(1)` when the controller is suspended and output selection races expiry, with no controller timer/audio callbacks. Host timer/EQ changes issue commands; grid/theme changes stay local.
- [ ] Run settings and existing sleepTimer/theme tests; expect controller-local execution/persistence to fail.
- [ ] Publish PC settings projections and retain sensitive credential-set flags, never values. Move domain persistence/timer/EQ execution to explicit desktop runtime and route expiry through Task 3 signals. Label This controller versus Connected PC and distinguish display-only preferences from domain mutations.
- [ ] Repeat tests through disconnect/resume and ensure stale phone preferences cannot overwrite host playback configuration.
- [ ] Commit `feat(controller): separate PC domain settings from presentation`.

## Task 14 Complete built-in library mutation and statistics coverage

**Files:** Create `src/lib/application/desktop/domain.ts`, application/domain.test.ts and `docs/testing/controller-capability-matrix.md`. Modify library/liked/liked-albums/listen-later/activity/pinned/customArtwork/playlistCovers stores, library-related shared views and Rust commands/library.rs, playlist.rs, activity.rs, metadata.rs, covers.rs.

**Interfaces:** Add closed queries for playlists, favorites, listen later, activity/statistics, pins and artwork preferences; intents for playlist CRUD/order/tracks, like/unlike, listen-later, pin/unpin, metadata/artwork edit and enrichment. `dispatchDomainIntent(intent:DomainIntent,context:CommandPreconditions):Promise<ExecutionResult>` adapts existing desktop implementations; every mutation emits domain revision invalidation.

- [ ] Create matrix rows for every built-in exposed action, owned handler, permission, task and executable test. Write `all_builtin_actions_have_handlers_and_tests`, `library_mutations_propagate_to_all_controllers` and `restored_generation_rejects_old_entity_reference`. Assert `expect(missingRegistryHandlers).toEqual([])` and reject success-returning unsupported stubs. Include playlist order, favorites/listen-later, desktop-originated pins and metadata edits.
- [ ] Run domain Vitest and controller query/mutation Rust tests; expect unported facade actions to fail.
- [ ] Replace direct UI native/domain calls with typed actions, preserving current semantics and PC IDs. Derive stats/ordering on PC; update watchers/sync/plugins/local-desktop mutation paths too. Explicitly gate incomplete matrix rows, not silently hide them behind Android platform checks.
- [ ] Repeat tests with desktop and two controllers changing the same entities; conflicts return current revisions and views refresh.
- [ ] Commit `feat(controller): complete shared library operation coverage`.

## Task 15 Add scoped PC administration and observable jobs

**Files:** Create `src-tauri/src/controller/{jobs,administration}.rs`, `src/lib/application/administration.ts`, administration.test.ts, and components/HostResourcePicker.svelte. Modify Settings.svelte, TrackList.svelte, AlbumDetail.svelte, progressiveScan.ts and Rust commands/library.rs, backup.rs, covers.rs, scanner modules and app-exit coordination.

**Interfaces:** Add `host_resources`, `jobs`, `backups` queries; closed scan/rescan/watch/cover-maintenance/download/import/backup/restore/delete/reveal/window intents. `ResourceRef={rootId:string;entryId:string}` is issued by the host. `submit_job(device:AuthenticatedDevice,intent:AdminIntent)->Result<JobReceipt,ControlError>`; receipt contains jobId/revision, terminal states include failed/partial/cancelled where supported.

- [ ] Write `resource_cannot_escape_granted_root`, `admin_confirmation_names_connected_pc`, `restore_rotates_epoch_and_discards_old_ids` and `controller_job_admission_is_bounded`. Assert `assert_eq!(denied.code, "permission_required")` without administration grant, `expect(newEpoch).not.toBe(oldEpoch)` after restore, and no fifth running or seventeenth pending controller job. Include traversal/junction/symlink escape, changed/deleted resource, OS denial, zero Android picker calls and rejected old cached rows.
- [ ] Run administration Vitest and `cargo test --manifest-path src-tauri/Cargo.toml controller::administration` plus jobs tests; expect absent workflows to fail.
- [ ] Build a PC-resource picker under explicitly granted roots with canonical path and resource identity revalidation per operation. Use existing host trash/delete/backup semantics, not arbitrary path execution. Run long work outside playback lane with progress/results; pause/stop as required before restore, rotate epoch and invalidate all data afterward. Native authorization/new roots may require visible PC interaction.
- [ ] Repeat tests under rename/replacement races, restore restart and failed jobs. Keep paired credentials outside music backups; no Android music uploads or backup copying.
- [ ] Commit `feat(controller): authorize PC administration and job workflows`.

## Task 16 Adapt integrations updates and plugin presentation

**Files:** Create `src/lib/application/desktop/{integrations.ts,plugin-remote.ts}`, application/{plugin-view.ts,integrations.test.ts,plugin-view.test.ts}, and components/RemotePluginView.svelte. Modify DesktopHome.svelte, MobileHome.svelte, PluginManager.svelte, PluginDrawer.svelte, PluginUpdateDialog.svelte, UpdatePopup.svelte, stores/updates.ts, sync.ts, plugin-store.ts, services/downloadService.ts, plugins/runtime.ts, marketplace.ts and ui-slots.ts.

**Interfaces:** Add PC-backed charts/recommendations/version/integration-state/plugin queries and typed service/plugin-manager actions. `RemotePluginView={id:string;title:string;nodes:RemotePluginNode[]}`; nodes are text, image reference, list or action controls with registered action IDs, never HTML/script/eval callbacks. `invokePluginAction(pluginId:string,actionId:string,args:JsonValue):Promise<ExecutionResult>` runs only on PC with its existing permissions.

- [ ] Write `controller_integrations_execute_only_on_pc`, `plugin_view_rejects_executable_content` and `version_query_describes_connected_pc`. Assert `expect(androidPluginLoads).toBe(0)` and `expect(hostPluginEventCount).toBe(1)`; account secrets never return, unsupported plugin views show desktop-only reason, malicious script/HTML/action references fail, and PC version differs from APK version in the test fixture.
- [ ] Run integration/plugin-view Vitest plus relevant plugin Rust tests; expect current direct frontend execution to fail.
- [ ] Adapt existing providers and plugin management through PC handlers. Stream resolution, downloads, LRC writing, OAuth/native dialogs and update actions remain host-side. Render only allowlisted declarative nodes; permission changes/install require administration and confirmation. Preserve legacy desktop cloud behavior without adding cloud to LAN controller outputs.
- [ ] Repeat tests and complete capability matrix: every built-in action is implemented/tested or visibly requires PC interaction; unsupported third-party plugins are reported, never executed on Android.
- [ ] Commit `feat(controller): adapt host integrations and safe plugin views`.

## Task 17 Verify and package compatible desktop and Android releases

**Files:** Create `docs/testing/android-desktop-controller.md` for compatibility/test/artifact records and `src/lib/application/controller-architecture.test.ts`. Modify Android app/build.gradle.kts release-signing configuration only where verification requires it; never commit keystores, passwords or generated APKs.

**Interfaces:** Produce signed APK plus matching desktop installer, recorded protocol version 1, verified signing fingerprint, hashes and supported/unsupported capability matrix. Desktop control remains off by default until explicitly enabled.

- [ ] Add architectural regression tests walking the controller/shared import graph and capability matrix; fail if desktop runtime, native audio, plugin execution, cloud socket or audio-resource fetching becomes reachable from controller startup. Add real TLS/permission failure and two-controller lifecycle scenarios to the release checklist.
- [ ] Run `npx vitest run`, `npm run check`, `npm run build`, and `cargo test --manifest-path src-tauri/Cargo.toml --lib`. Compare against the Task 1 baseline; new failures are blockers, unrelated existing failures are documented separately, never reported clean.
- [ ] Run Android UniversalDebug unit/instrumentation tests and physical phone/tablet PC/Squeeze workflows, including permission denial, no-camera/no-GMS pairing, largest invitation, rotation, sleep, two controllers, PC shutdown, restart and streamer stop timeout. Run applicable macOS/Linux/Flatpak secret-store checks or explicitly limit verified desktop targets.
- [ ] Require an owner-controlled external release signing configuration. Existing tracked release.keystore is not evidence of a safe signing identity; do not silently replace installed-app compatibility or log secrets. Missing signer/SDK/hardware blocks release, not unrelated test work. Build `npm run tauri -- android build --apk true --aab false --ci` and `npm run tauri -- build`; inspect actual new output files instead of guessing APK names.
- [ ] Verify APK with apksigner/apkanalyzer, confirm no media/storage/playback-service permissions, install the actual signed artifact using adb on phone/tablet, and verify no Android sound/domain work while PC/Squeeze performs playback. Record artifact SHA-256, desktop/APK versions, protocol, signing fingerprint and capability exceptions.
- [ ] Complete whole-branch review, commit scoped release/test documentation with `test(controller): verify compatible desktop and Android release`, and deliver absolute artifact paths. No push without explicit user request.

## Verified dependency references

These sources validate the chosen library capabilities, not completed integration or hardware behavior.

- [axum-server 0.8.0](https://docs.rs/axum-server/0.8.0/axum_server/) supports prebound TLS listeners and shutdown; the no-provider feature avoids adding a second default crypto provider.
- [rcgen 0.14.7](https://docs.rs/rcgen/0.14.7/rcgen/) supplies configurable CA and signed certificate generation.
- [qrcode 0.14.1](https://docs.rs/qrcode/0.14.1/qrcode/) encodes the compact pairing invitation on the PC; native scanning remains ZXing.
- [keyring 3.6.3](https://docs.rs/keyring/3.6.3/keyring/) requires explicit desktop backends; Android storage uses native [Keystore](https://developer.android.com/privacy-and-security/keystore) and [cryptography guidance](https://developer.android.com/privacy-and-security/cryptography).
- [Reqwest 0.12.28](https://docs.rs/reqwest/0.12.28/reqwest/struct.ClientBuilder.html) provides native trust, timeout, redirect/proxy and DNS resolution configuration; use explicit request URL ports.
- [Tauri native plugins](https://v2.tauri.app/develop/plugins/develop-mobile/) supply the local Android bridge; grant no frontend access to credential methods.
- [ZXing embedded 4.3.0](https://github.com/journeyapps/zxing-android-embedded/tree/v4.3.0) supports API 24 and embedded QR scanning without selecting the Google Play scanner.
- [AGP 8.11 compatibility](https://developer.android.com/build/releases/agp-8-11-0-release-notes) specifies SDK 36 support, JDK 17 and Build Tools 35.0.0; fixed NDK use is an implementation choice, not proof of a completed build.

## Coverage and handoff

Ownership/startup: Tasks 1–3. Pairing/security/native transport: Tasks 4–6. Snapshots/reconnect: Tasks 7–9. Album/output/shared UI: Tasks 3 and 8–11. Lyrics/waveform/settings/timers: Tasks 12–13. Built-in library/admin/integration/plugin coverage: Tasks 14–16. Security, lifecycle, regression and real release acceptance: Task 17.

Recommended execution is subagent-driven with one implementer and a fresh reviewer per task, followed by whole-branch review. The cross-language interfaces, native credentials and remote file administration justify the extra review cost. Direct execution by the primary agent is faster and less expensive but provides independent review only at the end.

Review this plan and select an execution method before implementation. A plan approval does not waive real-device, signing or permission gates.
