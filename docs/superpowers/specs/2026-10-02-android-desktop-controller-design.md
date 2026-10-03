# Android desktop controller design

Date: 2026-10-02. Status: written design approved by the user for implementation planning. The implementation plan and execution method still require review before product changes.

## Purpose and confirmed requirements

The Android application becomes an extension of the running Audion desktop application, not an independent player. It reuses Audion's design and functions with responsive layouts for phones and tablets. The desktop owns the library, files, database, queue, playback decisions, settings with domain effects, integrations, and plugin execution.

The primary workflow is to browse the PC's albums on a tablet, choose an album, and start playback on either the PC or a streamer discovered by the PC through Squeeze. Android never becomes an audio output, never copies the music library, and never takes over playback when the PC is unavailable.

The controller connection is LAN-only: no cloud relay, account requirement, router port forwarding, or automatic port mapping. Existing desktop features that contact online metadata or music providers may continue to do so on the PC; the controller connection itself does not depend on those providers.

The following sections record the approved design. They do not describe functionality already implemented.

## Existing code and reusable foundations

The original audit inspected local revision `fb43a01`. This design also checks current revision `d9409cc904c40dff1b418a63b9fbb53c881a8e57`, which adds persistent album grid and list views. Preserve those changes and the existing Squeeze connection and shutdown fixes. The [feasibility review](../../research/2026-10-02-android-desktop-controller-feasibility.md) records local and pinned upstream evidence.

- Shared Svelte views can be reused, but [startup](../../../src/routes/+page.svelte) currently initializes local library, playback, plugins, and Squeeze. Android needs a distinct controller bootstrap.
- [AlbumDetail](../../../src/lib/components/AlbumDetail.svelte) and [AlbumGrid](../../../src/lib/components/AlbumGrid.svelte) currently call desktop-style `playTracks` locally. Forward album intent, not low-level audio calls or phone-built queue state.
- [player.ts](../../../src/lib/stores/player.ts) owns queue and playback orchestration. Some operations return `void`, catch errors, or launch Squeeze calls without awaiting completion. A thin wrapper cannot truthfully report remote success.
- [ConnectPanel](../../../src/lib/components/ConnectPanel.svelte) selects Squeeze with `activateSqueezeTarget`; selecting the current device disconnects the streamer. Reuse desktop output semantics rather than adding a second output manager on Android.
- [Lyrics](../../../src/lib/stores/lyrics.ts), [plugins](../../../src/lib/plugins/runtime.ts), [sleep timers](../../../src/lib/stores/sleepTimer.ts), and [equalizer](../../../src/lib/stores/equalizer.ts) also contain frontend domain execution. Replacing the Tauri invocation wrapper alone is insufficient.
- Upstream adds a library provider seam, but not complete control of the already-running desktop. Reuse that separation selectively; a wholesale upstream upgrade is not a prerequisite.

## Architecture and ownership

The shared UI calls an application interface for queries, typed commands, and subscriptions. A desktop adapter executes existing host behavior. An Android controller adapter delegates to the paired desktop and updates presentation stores from confirmed host state. These are real adapters at the same seam; neither exposes an unrestricted remote Tauri invocation mechanism.

The desktop control module has three internal responsibilities: authenticated LAN transport, command coordination, and authoritative state publication. Rust owns listener lifecycle, pairing, resource access, and network validation. The existing desktop TypeScript runtime continues to own playback and other frontend domain orchestration. Moving all domain logic into Rust is unnecessary.

Rust dispatches correlated commands only to the registered authoritative desktop coordinator. That coordinator must be ready before commands are accepted and must return an explicit result. Startup, window reload, and shutdown mark execution unavailable. Event delivery is not an authorization check or a command ordering guarantee; enforce both in the control module. [Tauri documents the event mechanism and its limitations](https://v2.tauri.app/develop/calling-frontend/).

| State or work | Owner |
| --- | --- |
| Music library, playlists, favorites, history, artwork mutations | Desktop |
| Queue, shuffle order, repeat, playback context and selected output | Desktop |
| Lyrics processing, metadata enrichment, scans, downloads and backups | Desktop |
| Equalizer, sleep timer, integrations, credentials and plugins | Desktop |
| Navigation, expanded panels, grid or list preference, local accessibility preferences | Each UI instance |
| Pairing credentials and bounded non-authoritative display cache | Controller native storage |

Android startup must not initialize a music database, scanner, watcher, audio engine, Squeeze server, plugin runtime, cloud synchronization, scrobbling, playback timers, or restoration of a local queue. Existing Android data is ignored in controller mode, not uploaded or automatically deleted. Desktop startup and playback remain functional when no controller is connected.

## LAN connection and pairing

Desktop settings expose an initially disabled LAN controller switch, the selected LAN interface, paired devices, and a pairing action. The new control listener is separate from Squeeze's legacy listeners. Authentication or TLS changes must not break existing renderer discovery, audio streaming, or CometD clients.

The first release uses a selected private IPv4 LAN interface. It must not bind to all interfaces or automatically create router mappings. The listener rejects non-private peer addresses using the direct socket address, not forwarded headers. The client accepts only direct private LAN endpoints, checks the resolved address, and disables proxies and redirects. IPv6-only LANs and automatic network discovery are excluded from the first release. Firewall or guest Wi-Fi isolation failures produce actionable connection messages rather than an insecure fallback. A changed PC address can be updated without replacing the saved host identity or granting trust to a different certificate authority.

Pairing uses a desktop-generated QR or copyable invitation containing the host identity, LAN endpoint, trusted certificate material and fingerprint, and a cryptographically random single-use secret. The invitation expires after five minutes. Android verifies the supplied trust material before submitting the secret over TLS. The desktop shows the requesting device for explicit approval and then issues an independently revocable device credential. A short numeric code without trusted host identity is not sufficient.

For stable identity despite changing PC addresses, use a private per-host certificate authority and a server certificate for `audion-<hostId>.invalid`. The invitation carries the public authority certificate and its fingerprint. The native client trusts only that authority, validates the certificate chain, validity and server name, and resolves that name directly to the approved private LAN address without public DNS. Renewing a server certificate under the same authority does not change pairing; replacing the authority requires pairing again. Never enable global invalid-certificate or invalid-hostname acceptance. A trusted certificate configuration alone must not accidentally leave unrelated root authorities accepted. [Reqwest provides native TLS, trust, resolution, redirect and timeout configuration](https://docs.rs/reqwest/0.12.28/reqwest/struct.ClientBuilder.html).

All commands, queries, artwork and event requests require authorization. Store controller secrets encrypted with an Android Keystore-backed key, not in WebView localStorage; protect desktop private keys with OS-backed storage, retain hashes rather than raw device credentials on the host, and restrict file access. Revocation terminates active polls and denies subsequent requests; it cannot undo an already executed operation. Invitations, authorization headers and private keys must not appear in logs. Bound pairing attempts, pending approvals, request sizes, page sizes, event history and per-device command concurrency to prevent unbounded work. Android's [network security guidance](https://developer.android.com/privacy-and-security/security-config) and [Keystore guidance](https://developer.android.com/privacy-and-security/keystore) support the security constraints; XML WebView configuration is not a substitute for configuring the Rust transport.

Pairing grants library and playback control. Desktop approval may additionally grant administration. File deletion, backup restoration, plugin installation and credential changes require that administration grant plus an explicit operation confirmation. Confirmations name the PC and the affected resources. They do not imply running the operation on Android.

## Application interface and transport

Use HTTPS for commands, queries, media and one finite long-poll event subscription. This reuses the existing native HTTP client direction without adding a separate WebSocket client stack. Queries and commands remain independent of the outstanding poll. The protocol is a dedicated versioned allowlist, not a proxy for arbitrary command names, file paths, URLs, or scripts.

- A handshake returns `protocolVersion`, stable `hostId`, boot-specific `hostEpoch`, granted permissions, capabilities, and output-specific capabilities. Unsupported protocol versions fail clearly.
- Queries use desktop entity identifiers and paginated projections. Library pages and queue pages carry their domain revision; cursors become invalid when that revision changes. Do not return an entire library with every playback update.
- Commands carry an immutable `requestId`, expected `hostEpoch`, typed intent and validated parameters. Revision-sensitive actions carry an expected queue, library, or output revision. Reject a command for an earlier host epoch before execution. Device identity comes from authentication, not a client-supplied device name or ID.
- Results distinguish a completed operation from an accepted long-running job. Completed results include the resulting revision; jobs return a `jobId` and publish progress and terminal results. Network receipt does not mean playback started.
- Errors include structured codes such as `unauthorized`, `permission_required`, `unsupported`, `not_found`, `revision_conflict`, `output_unavailable`, `host_not_ready`, and `execution_failed`. Report partial effects when an output fails after queue state changes; do not fabricate rollback or successful playback.

The host deduplicates in-flight and completed requests by authenticated device and request ID, rejects ID reuse with a different payload, and retains completed results for at least five minutes. Pending jobs retain their identity until terminal completion. A timed-out request has an unknown outcome; the controller queries its status. It must not replay non-idempotent actions automatically after host restart or result expiry. Deduplication within one running host is not a claim of exactly-once execution across crashes.

The desktop UI and controller playback, queue, and output intents enter the same serialized coordinator. Output selection and playback cannot race in separate execution lanes. Track completion and host timer actions also pass through the coordinator. Scans and backups run as jobs outside that lane. Result-aware changes to existing playback functions are in scope; unrelated player refactoring is not. If the authoritative frontend reloads, rotate the host epoch and invalidate pending execution before rebuilding state so stale commands cannot execute against a new coordinator.

## State synchronization and reconnection

The authoritative snapshot contains host identity and epoch, event revision, current track and playback context, position and duration, playback toggles, queue summary and revision, selected output and output revision, available outputs, settings projections, capabilities, and job summaries. Large queue contents are fetched in revision-consistent pages. Library content is queried separately and invalidated when its revision changes.

Events have monotonically increasing revisions within a host epoch. Keep a bounded history; expired cursors and new host epochs return `resync_required`. Capture the snapshot revision and establish event replay without a gap. A long poll responds immediately when events arrive and ends after at most 25 seconds when idle. Only one poll is outstanding per controller. Coalesce playback position updates; local interpolation is for display only and never advances tracks or triggers domain timers.

On reconnect or Android resume, verify the same paired host, obtain fresh state, and replace stale projections before enabling commands. Use bounded reconnect backoff. Clear host-specific caches when switching PCs. Do not merge a phone queue or settings snapshot into desktop state.

While disconnected, cached views may remain visible with an explicit offline indicator, but mutations are disabled and are not queued for later execution. Closing or suspending the controller must not stop PC or streamer playback. No background Android playback service is required for the controller. Closing the desktop stops the control listener and preserves the existing ordered Squeeze shutdown behavior.

## Album playback and output workflow

1. Android requests albums, tracks and cover resources from the paired PC. Sorting and filtering query the PC library; the UI never resolves a PC music path on Android.
2. The output selector lists the connected PC and streamers discovered by that PC. The PC entry is named explicitly; “This device” must not mean the tablet. Cloud peers and Android outputs are not controller output choices.
3. Selecting an output sends a host intent and waits for confirmation. The confirmed output revision is retained for subsequent playback actions.
4. Selecting Play Album sends its desktop album ID, play mode, optional starting track, relevant library revision and expected output revision. A liked-only album view sends an explicit liked-only play mode, not a phone-generated queue.
5. The desktop validates the album and target, resolves track order and context, applies its shuffle/repeat behavior, builds the queue and invokes its existing output routing. Stale output revisions are rejected so the command cannot accidentally play on a newly selected target.
6. The controller displays the confirmed queue, track, progress and volume, including when the output is Squeeze. Desktop changes and other authorized controllers produce the same state updates.

To switch outputs, validate the requested target, stop the previous owned PC or streamer output, await its result, and only then commit the new selection. Retain the existing real Squeeze stop/disconnect behavior. Simply changing `activeBackend` is not proof that the old audio stopped. If stopping fails or times out, fail the selection, retain the old selected target with failed or unknown status, and do not start the new target. An unreachable renderer may require physical intervention; never claim it has stopped without evidence. If the old output stops but the new target becomes unavailable, report that failure and keep playback stopped. Do not promise seamless transfer at the same playback position. After a confirmed selection, playback begins on the chosen target only when the user requests it. Losing a streamer reports the target failure; the host must not silently switch to PC speakers or Android audio. Equalizer and other target-dependent controls show their supported output capability rather than pretending every renderer supports them.

## Shared UI and capability coverage

Keep one Audion design system and shared library, album, artist, playlist, player, queue, lyrics and settings views. Separate touch-platform detection from viewport layout: Android must not force a narrow phone shell on a wide tablet. Phones use compact navigation and stacked views; wider tablets use expanded navigation and available detail panes. Portrait, landscape, resizing, touch targets and keyboard accessibility must remain usable without horizontal page overflow.

Preserve album grid/list, sorting, context actions, scroll position and reduced-motion behavior from the current revision. Layout preference belongs to the controller; album pins and other library mutations belong to the PC. Settings distinguish “This controller” presentation/connection preferences from “Connected PC” domain settings.

| Desktop capability | Controller adaptation |
| --- | --- |
| Library, albums, artists, playlists, search | PC queries and high-level playback intents; no independent library |
| Playback, queue, shuffle, repeat, selected output | Serialized PC commands and confirmed projections |
| Favorites, listen later, history, statistics and pins | PC reads and mutations; invalidation across connected UIs |
| Artwork and metadata editing | Authorized resources and PC mutations or enrichment jobs |
| Lyrics and source selection | PC fetches, parses, caches and validates; Android renders prepared data |
| Sleep timer and equalizer | PC settings and execution, even while Android sleeps |
| Folder selection, import and downloads | Scoped PC resource selector; PC imports/downloads, not Android storage |
| Scanning, watching, cover migration and maintenance | Host jobs with progress, cancellation where supported, and terminal results |
| Backup, restore and deletion | Administration permission and explicit PC-targeted confirmations |
| Integrations, charts and recommendations | Domain requests and provider credentials stay on PC; results are displayed remotely |
| Plugin management and supported plugin actions | PC installation, permissions and runtime execution; typed remote action contract |
| OS actions and account authorization | Explicit PC-targeted action or visible PC-interaction-required state |

Artwork travels through the native authenticated, host-bound TLS client and becomes a bounded local display resource. Direct WebView image requests to the private desktop certificate are not the media strategy. Cache keys include host and resource revision; do not expose bearer tokens in image URLs. Never convert a desktop file path into an Android asset URL.

The PC resource selector uses host-issued opaque references under roots explicitly granted by the desktop user. Revalidate canonical paths, symlinks/junctions and permissions for each operation; reject traversal and access outside granted roots. Adding a new root or satisfying an OS authorization dialog may require interaction on the PC. Remote download/export refers to a PC destination. Uploading music from Android storage and copying backup archives onto Android are not part of this controller design. Lyrics import may submit bounded text for PC validation; it does not authorize arbitrary file uploads.

Arbitrary existing plugins cannot run JavaScript or WASM on Android without violating the controller-only requirement. Plugin management and exposed actions can be remote. Plugin-provided views require an explicit declarative remote-view contract with host-executed actions; never serve executable plugin code to the controller. Plugins without that contract are visibly desktop-only. Full parity must not be claimed for those plugins or unavoidable native OS prompts.

During staged delivery, unsupported capabilities remain explicit and fail closed. A completed album-playback slice is not complete desktop parity. Before declaring the controller release complete, audit every built-in desktop action against this inventory and document remaining desktop-interaction requirements.

## Delivery sequence

This is one cohesive controller subsystem with incremental verification, not permission to start implementation. The later implementation plan must order the work as follows:

1. Establish the typed application seam, controller-only bootstrap, host coordinator readiness and result-aware execution. Inventory direct native calls and frontend domain execution.
2. Add LAN transport, pairing, revocation, native certificate trust, authorized artwork, snapshots, commands and reconnect. Prove a tablet starts a PC-library album on PC and Squeeze outputs without Android domain execution.
3. Migrate shared playback/queue and library actions, then lyrics, history, metadata, settings, equalizer and sleep timer. Include desktop-originated state updates and concurrent controllers.
4. Add scoped PC administration, maintenance jobs, backup workflows, integrations and plugin management. Add remote plugin views only through the explicit contract; report unsupported third-party plugins honestly.
5. Complete responsive phone/tablet QA, security and regression checks, then generate the signed APK together with a compatible desktop build. Record protocol compatibility and actual remaining parity limits.

Do not merge upstream wholesale, rewrite the entire player, build a separate cloud server, add Android audio streaming, or extend unauthenticated Squeeze routes for general desktop administration as shortcuts.

## Verification and acceptance

Verification must cross the application interface and include both desktop adapter behavior and Android controller behavior. Mocks alone are insufficient for playback, native TLS, artwork and streamer acceptance.

- **Primary workflow:** on a real tablet, browse the PC library and its covers, select the PC or a PC-discovered Squeeze streamer, and start an album. Assert host queue/context and actual output playback; Android produces no audio.
- **Single authority:** instrumentation confirms zero Android music database initialization, scans, watchers, plugin execution, scrobbles, audio engine/service startup or domain sleep timers. Existing Android data and disconnected gestures do not start these paths.
- **Shared state:** desktop-originated playback, queue, output, library and settings changes reach all authorized controllers. Progress also mirrors Squeeze output. Large libraries and queues use consistent pagination.
- **Reliable commands:** exercise playback failure, lost acknowledgements, duplicate request IDs, different payload reuse, concurrent output changes, stale revisions, expired results, host restart and coordinator reload. Test playing PC to Squeeze, Squeeze A to B, and Squeeze to PC; the previous output must be stopped before confirming the new target. Failed or timed-out stops must not start the new target or claim that an unreachable renderer stopped. Unknown outcomes do not produce an automatic duplicate action or false success.
- **Connection security:** reject unpaired/revoked devices, wrong host certificates, incorrect server names, expired invitations, reused invitations, insufficient permissions, redirects and non-LAN endpoints. Revocation closes active polls and prevents artwork access.
- **Files and administration:** test traversal, junction/symlink escape, deleted resources, ungranted roots, denied OS access, destructive confirmation, restore/restart and job failure. No PC action resolves to Android storage.
- **Lifecycle:** controller disconnect/sleep/closure leaves desktop playback and timers intact. Desktop exit retains ordered streamer stop behavior. Resume and restart replace stale state without replaying old commands.
- **Responsive UI:** verify phone and tablet, portrait/landscape, narrow and expanded widths, touch and keyboard interaction, grid/list continuity, accessibility and reduced motion.
- **Regression and release:** run relevant existing desktop/Squeeze tests, new interface and transport tests, frontend checks and both builds. Validate the signed APK on actual hardware and record the compatible desktop version. Do not claim full parity until built-in action coverage and explicit exceptions have been reviewed.

## Review gate

The user approved this written design for the [implementation plan](../plans/2026-10-02-android-desktop-controller.md). Review that plan and select an execution method before product changes. No product code, dependencies, Android package or desktop build has been changed by these documents.
