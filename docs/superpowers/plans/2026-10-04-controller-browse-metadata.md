# Integrated Controller Browse Metadata Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans for the proposed inline execution, or superpowers:subagent-driven-development only if that execution method is explicitly selected. Implement task-by-task using the checkbox steps below.

**Goal:** Display PC-authoritative track play counts and full album duration in the existing Android APK without artwork refresh/flicker.

**Architecture:** One internal typed read in the existing PC/controller module, consumed by a foreground-only presentation owner. Existing catalog queries, Library revision, media references and accounting remain independent. Desktop and APK are deployed together; there is no separately installed extension or legacy-support probing.

**Tech Stack:** Existing Rust, rusqlite, native TLS transport, Tauri IPC, TypeScript, Svelte, Vitest; no new dependencies.

**Spec:** `docs/superpowers/specs/2026-10-04-controller-browse-metadata-design.md` (integrated delivery revision).

Status: Inline execution approved 2026-10-04. Tasks 1–4 source implemented and reviewed; Task 5 assigned to the existing UI owner; Task 6 integrated verification/deployment pending. See the plan-specific SDD ledger for exact evidence.

## Global Constraints

- Ship inside the existing desktop build and APK; preserve pairing/user data. No extra server/plugin/package or phone library.
- Metadata interval: 5seconds after each completed request, one in-flight read, active presentation and foreground only.
- Input: at most 1,000 track IDs before deduplication; positive safe integers, no provider/synthetic IDs; empty IDs only with albumId.
- Route JSON limits: 64KiB request and 256KiB successful reply; existing bounded structured-error limit. Shared existing quotas/admission.
- metadataVersion: 1, strict DTOs and matching host epoch/library revision. True 0 is not unknown null.
- Full album duration in seconds, independent of pagination/text/liked filter; null if incomplete/invalid/unsafe; existing empty album has 0 seconds.
- Keep play-count writes excluded from Library stamp. Metadata changes must not clear page/image caches, rotate/dispose artwork, reset navigation or invoke page.refresh.
- Root does not edit UI-owned files. Task 5 runs through the existing UI owner after a narrow assignment; independent correction loops are internal, not human blockers.
- Preserve desktop native handlers and all existing failure/partial/unknown/disposal/permission assertions. No unrelated baseline repair, commits, or new parallel sessions without authorization.
- Artifact/code/UI text is English unless extending existing localized copy.

## Review Focus

1. A zero count is visible as 0, while provider/missing/invalid counts remain unknown: Task 1/2/5 tests.
2. A 101-track/multi-disc album or liked-filtered page still displays the full PC total: Task 2/5 tests.
3. Background/navigation/epoch change during a slow metadata response cannot adopt old values: Task 3/4 tests.
4. Two active cover/catalog reads cannot be starved by optional statistics traffic: Task 4 admission tests.
5. A play-count write can advance the displayed number without a Library event or any artwork handle disposal: Task 2/4/6 tests.

## File Map

- `src-tauri/src/controller/protocol.rs`: new strict metadata request/reply DTOs, without altering existing wire DTOs.
- `src-tauri/src/controller/browse_metadata.rs` (new): bounded DB capture/aggregation only, desktop-gated in `mod.rs`.
- `src-tauri/src/controller/queries.rs`, `commands.rs`, `host.rs`: admitted read/context/auth plumbing.
- `src-tauri/src/controller/client.rs`, `native_session.rs`, `native_session_tests.rs`, `src-tauri/src/commands/controller.rs`: fixed transport route and fenced IPC request/reply.
- `src/lib/application/types.ts`, `browse-metadata.ts` (new), `browse-metadata.test.ts` (new): shared types and strict validation.
- `src/lib/application/controller/bootstrap.ts`, `session.ts`, `session.test.ts`, `read-recovery.ts`, `read-recovery.test.ts`: native bridge, low-priority read admission, lifecycle fencing.
- `src/lib/application/controller/browse-metadata.ts` and `.test.ts` (new): active presentation owner/freshness.
- UI-owned Task 5: `ControllerBrowse.svelte`, `presentation/TrackRows.svelte`, `ControllerViews.test.ts`, `SharedBrowse.test.ts`; change no other UI paths without an ownership addendum.
- `emulator-ui-runtime/` under the existing SDD directory: root-owned bounded receipts/probes only.

## Task 1: Strict metadata contract

**Files:** Modify `protocol.rs`, `types.ts`; create application `browse-metadata.ts` and `.test.ts`.

**Interfaces:**
- Public `BrowseMetadataQuery = { trackIds: number[]; albumId?: number }`.
- Wire `BrowseMetadataRequest = BrowseMetadataQuery & { metadataVersion: 1; hostEpoch: string; libraryRevision: number }`.
- `TrackPlayCount = { trackId: number; playCount: number | null }`.
- `AlbumDuration = { albumId: number; totalDurationSeconds: number | null }`.
- `BrowseMetadataResult = { metadataVersion: 1; hostEpoch: string; libraryRevision: number; tracks: TrackPlayCount[]; album?: AlbumDuration }`.
- TS `parseBrowseMetadataQuery(value: unknown): BrowseMetadataQuery`, `parseBrowseMetadataResult(value: unknown, request: BrowseMetadataRequest): BrowseMetadataResult`.
- Rust DTOs carry the same names/serialized fields. Validate vectors/cross-field identities on decode and before publication; reuse existing numeric/identifier validators rather than weakening existing protocol rules.
- Add optional `ApplicationPort.queryBrowseMetadata(query, signal): Promise<BrowseMetadataResult>` as the non-controller adapter seam, not runtime support discovery.

- [ ] Write `metadata_preserves_zero_and_null` using request tracks[42,43], reply counts[0,null], expected values exactly0/null; preserve matching epoch/revision7.
- [ ] Write table-driven `metadata_rejects_invalid_request_or_reply`:1001 entries, empty/no album, negative/NaN/unsafe ID/count, duplicate/unrequested reply ID, missing reply ID, mismatched album/epoch/revision, metadataVersion2 and unknown fields. Request duplicates deduplicate, but the initial vector size limit still applies.
A literal parser assertion for the zero/null contract (with a valid matching request/reply fixture defined in this test file):

```ts
expect(parseBrowseMetadataResult(reply, request).tracks).toEqual([
  { trackId: 42, playCount: 0 },
  { trackId: 43, playCount: null }
]);
expect(() => parseBrowseMetadataQuery({ trackIds: Array(1001).fill(42) })).toThrow();
```

- [ ] Run `npx vitest run src/lib/application/browse-metadata.test.ts` and `cargo test --manifest-path src-tauri/Cargo.toml --lib browse_metadata`; capture the expected RED at the actual parser/DTO seam, not a source-text assertion.
- [ ] Implement types/parsers and parity cases. Enforce complete reply coverage per unique requested ID and optional album presence matching the request.
- [ ] Repeat the two commands; all focused tests PASS. Confirm existing catalog/handshake/events/artwork schemas were not changed. Record a review checkpoint; no commit.

## Task 2: PC-authoritative DB capture and authenticated route

**Files:** Create controller `browse_metadata.rs`; modify `mod.rs`, `queries.rs`, `commands.rs`, `host.rs`; use real in-memory DB tests in the new module and host tests.

**Interfaces:**
- `capture_browse_metadata(conn: &rusqlite::Connection, query: &BrowseMetadataQuery) -> Result<BrowseMetadataPayload, ControlError>`; payload is internal `{tracks, album}` without auth/revision fields.
- `LibraryQueries::query_browse_metadata(&self, request: BrowseMetadataRequest, context: QueryContext) -> Result<BrowseMetadataResult, ControlError>` (async, existing work permit + bounded blocking capture).
- `CommandService::query_browse_metadata(&self, request: BrowseMetadataRequest) -> Result<BrowseMetadataResult, ControlError>` (async, validates expected epoch/revision and rechecks catalog context).
- Fixed POST `/control/v1/browse-metadata` uses existing authenticated device/QuerySlot, auth before and after admitted work, and request/reply size limits from Global Constraints.

- [ ] Create test fixtures with `Connection::open_in_memory()` and `db::schema::init_schema`; never open the user's DB. Insert album8 with101 tracks of2seconds each, across two discs; one count 0 and one 12. Request only one track and expect totalDurationSeconds 202. Liked membership must not change that total.
- [ ] Write `metadata_unknowns_are_not_zero`: missing track, NULL/negative/unsafe/text/fractional SQLite count ->null; NULL/negative/text/fractional SQLite duration on one album member ->null total; genuine empty album ->0. Test the same production capture function for each case.
- [ ] Write `metadata_count_write_does_not_advance_library`: capture Library stamp, update only play_count, assert stamp unchanged, then call capture again and assert the new count. Keep existing schema revision tests intact.
- [ ] Add host tests for revoked/unpaired/browse-only devices,64KiB limit,1000-ID bound, shared QuerySlot/Busy, requested stamp/epoch changing during capture, and queue-only changes not invalidating unrelated metadata reads. Verify no paths/URLs/history/media fields escape.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --lib browse_metadata` and the host-specific new test filter; verify RED cases identify missing capture/route behavior.
- [ ] Implement parameter-bound ID reads in bounded chunks and checked whole-album aggregation in one consistent DB capture. Keep locks out of serialization/network work. Reuse LibraryQueries work permits; do not create statistics triggers/revision/events or register resources.
- [ ] Implement the fixed authenticated route and metadata-specific JSON bounds; preserve existing errors, quota ownership until admitted work finishes, and auth/catalog fences. Repeat focused tests to PASS; review the response allowlist and unchanged play-count Library exclusion.

## Task 3: Fixed native transport and IPC lifecycle

**Files:** Modify `client.rs`, `native_session.rs`, `native_session_tests.rs`, commands `controller.rs`, frontend `bootstrap.ts`, `session.ts` bridge interface.

**Interfaces:**
- `NativeTransport::browse_metadata(&self, request: &BrowseMetadataRequest) -> Result<BrowseMetadataResult, ControlError>` uses the fixed route and 256KiB receive bound, never accepts a caller URL.
- `ControllerRequest::BrowseMetadata { request: BrowseMetadataRequest }`; `ControllerReply::BrowseMetadata { result: BrowseMetadataResult }`, serialized type `browse_metadata`.
- `ControllerNativeBridge.browseMetadata(fence: NativeControllerFence, request: BrowseMetadataRequest): Promise<BrowseMetadataResult>`; bootstrap maps it to existing `controller_request` IPC.

- [ ] Extend the local native HTTP fixture for the metadata route, with held response/error modes. Write `late_browse_metadata_is_discarded_after_suspend_or_host_switch`: hold a real response, change scope/epoch, release it, and assert no metadata reply is adopted. Confirm image-cache state is untouched.
- [ ] Write reply identity/version/duplicate/oversized-response rejection and revoked-auth cases. A missing route remains a structured metadata error; it must not invent data, restart pairing, or become successful support discovery.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --lib browse_metadata` with the native test filter and observe RED.
- [ ] Implement transport/IPC branch with existing window-origin, active scope, cancellation, epoch/library checks before native entry and before return. It is a read, not a control mutation. Include the new bridge method in existing complete typed test bridge fixtures without weakening their assertions.
- [ ] Repeat targeted native tests to PASS; build/type-check the bridge. Review request/reply limits and confirm no dynamic route/generic invoke was added.

## Task 4: Session read and bounded active presentation owner

**Files:** Modify controller `session.ts`, `session.test.ts`, `read-recovery.ts`, `read-recovery.test.ts`; create controller `browse-metadata.ts` and `.test.ts`.

**Interfaces:**
- Preserve the existing callable `createReadAdmission()` result; add `tryRun<T>(signal, attempt): Promise<T>` which rejects local nonretryable Busy if two reads are active or foreground reads are queued, without enqueueing metadata. Existing read callers keep their current FIFO behavior.
- Extend private session `read` with a defaulted final admission mode `foreground | optional`; optional mode uses tryRun and all the same lifecycle/read revision fences. Existing query/media callers are unchanged.
- `session.port.queryBrowseMetadata(query, signal)` derives metadataVersion/epoch/libraryRevision from the current authenticated session, invokes native.browseMetadata, strictly validates the reply, bypasses query/image caches and returns a fresh read.
- `createControllerBrowseMetadata(port = getApplicationPort, connection = controllerState, visible?: Readable<boolean>)` returns `{state, setQuery(query: BrowseMetadataQuery), dispose()}`. Default visibility listens to document only in the browser; injected visibility and fake timers are used in tests. SSR starts no timers/network.
- State: `{ counts: ReadonlyMap<number, number | null>; album: AlbumDuration | null; loading: boolean; stale: boolean; error: string }`.

- [ ] In session tests, return counts 0 then 1 from the native read at the same libraryRevision. Assert two native reads (no stale cache hit), valid values, no image revocation and no changes to catalog QueryResult. Native args use current fences, not UI-supplied epoch.
- [ ] In admission tests hold two production read leases; optional tryRun must reject Busy immediately, not enter its attempt. Release one and confirm a later optional read can execute. Aborted work retains its lease until the underlying native operation settles; pending foreground reads retain priority.
- [ ] Owner tests use real stores, fake timers and the real session read with deferred native responses: immediate initial read; no overlapping call while held; next interval 5000 ms after completion; a native count update changes only the count map.
- [ ] Add latest-generation, navigation disposal, hidden/background pause/resume, host/epoch/library/grant change and Busy/error cases. Same-scope failures retain confirmed values marked stale; query change removes unrelated IDs/album. Filter provider IDs before native entry, bound1000 current IDs, and skip empty/no-album state without probing.
- [ ] Run `npx vitest run src/lib/application/controller/browse-metadata.test.ts src/lib/application/controller/session.test.ts src/lib/application/controller/read-recovery.test.ts`; record RED.
- [ ] Implement the session method and owner; scheduling/coalescing are owned here, not repeated in UI handlers. Local admission Busy defers to the next interval without a routine notice; reuse existing bounded transient host-read recovery, never retry commands or add another loop.
- [ ] Repeat focused tests to PASS. Review all dispose/foreground branches; verify no page.refresh, Library publication or artwork/cache mutation is reachable from a count update.

## Task 5: Existing UI-owner integration

**Files (UI-owned):** `src/lib/components/ControllerBrowse.svelte`, `src/lib/components/presentation/TrackRows.svelte`, `src/lib/components/ControllerViews.test.ts`, `src/lib/components/SharedBrowse.test.ts`.

**Interfaces:** Consume Task 4 readonly metadata state. TrackRows adds passive optional props `playCounts: ReadonlyMap<number, number | null>` (default empty map) and `countsStale: boolean` (default false). ControllerBrowse constructs/disposes one owner, feeds current confirmed local track IDs/albumId, and joins display state without rewriting page items or artwork references.

- [ ] Root writes the exact owned-file assignment and sends it to the existing UI task; it includes Task 1/4 interfaces and the design link. Do not create a new UI session or edit owned paths from root.
- [ ] UI writes actual compiled-render regressions: Plays renders 0 and12; null/missing/provider IDs show a dash, not 0; stale count has an accessible explanation; full album 202 seconds persists when only a subset is loaded or liked filter changes.
- [ ] UI tests scope-specific obsolete-copy removal: play-count/total-duration missing-projection copy disappears when valid data is supplied, while release-metadata/favorite-mutation/liked-total explanations remain. Pending/success introduces no notice panel; metadata error is inline, not a command outcome.
- [ ] UI runs `npx vitest run src/lib/components/SharedBrowse.test.ts src/lib/components/ControllerViews.test.ts` to verify RED, implements passive wiring/columns, reruns to PASS and reports exact diff/evidence.
- [ ] Root independently reviews owned diff: preserve real canExecute/dispatch, native desktop slots/handlers, failure/partial/unknown/disposal assertions and exact artwork references. Route findings back to the UI owner; never overwrite concurrent UI work. Record checkpoint, no commit.

## Task 6: Full verification and coordinated PC/APK deployment

**Files:** Existing root review/handoff; new root-owned metadata runtime receipt/probe in `.superpowers/sdd/2026-10-02-android-desktop-controller/emulator-ui-runtime/`.

- [ ] Before implementation verification, record fresh shared-checkout baseline. Compare checker errors by location/message, not just total. Previously documented ten baseline errors are not permission to hide new diagnostics.
- [ ] Run full `npx vitest run`, `npm run check`, `npm run build`, `git diff --check`, and `cargo test --manifest-path src-tauri/Cargo.toml --lib` with the verified project toolchain; record exit codes/counts and report every failure. Review every task against the design and acceptance cases. No pass claims from scoped tests alone.
- [ ] Obtain one independent integrated review of root-owned native/metadata changes using the applicable requesting-code-review workflow. Feed real findings through RED/GREEN corrections and rerun full verification. Keep existing UI ownership separate; no new parallel implementation session without user choice.
- [ ] Build desktop and arm64 APK from the reviewed source snapshot. Use the existing isolated Android toolchain, not global SDK/settings changes. If Windows symlink privilege alone fails after a finished native build, use the established resolved-path/JNI hash-verified copy and Gradle packaging workaround; fail on any other build error.
- [ ] Verify APK/native/frontend fingerprints and final binaries match the reviewed snapshot. Record version/hash/ABI/minSDK; do not call an emulator-tested debug build a stable physical-tablet release.
- [ ] Install-r on the authorized emulator, preserving data. Coordinate any required desktop-host replacement explicitly; no forced process kill, playback interruption or PC suspension. If runtime is in use, agree on a safe deployment window instead of silently restarting it.
- [ ] Runtime: compare full album duration against an isolated PC DB fixture; show 0 then a real persisted count increase on the next healthy foreground read. Use natural authorized playback accounting for real-library checks; never rewrite the user's history/counts to make a probe pass.
- [ ] Measure album/detail cover availability during metadata updates, and natural song transition with Albums active. Recheck real Library invalidation separately, plus background/resume, auth revocation and inline missing-route error isolation. No global refresh/placeholder or grid resize may be caused by statistics success/pending.
- [ ] Dispose own observers/temporary fixtures/ADB forward only; leave shared runtime running. Save receipts and Engram outcome, including unresolved limits. Handoff the same integrated APK; no full-theme/physical-tablet parity claim without corresponding evidence.

## Plan Self-Review

- All selected design requirements map to Tasks 1-6; old-version support/probing was explicitly removed by scope clarification.
- Public vs wire query types and IPC names align across tasks. Epoch/library authority stays in session/native/PC, not UI inputs.
- Each task carries a RED/GREEN seam and a review checkpoint; integration and runtime verification are not implied by a written plan.
- No new dependencies, package, music server, statistics trigger or event domain.
- Root does not borrow UI ownership; desktop accounting and image lifecycle are preserved.
- Execution recommendation: root implements native/module tasks inline; existing assigned UI owner handles Task 5; independent integrated review before deployment. This avoids context/coordination overhead for the sequential root tasks.

Execution approved by user. Continue inline without per-task approval; coordinate live desktop-host replacement before interrupting playback. Completion requires integrated source/build/runtime evidence.
