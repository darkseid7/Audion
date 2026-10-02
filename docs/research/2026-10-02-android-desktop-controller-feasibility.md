# Android desktop controller feasibility review

Date: 2026-10-02. Status: source investigation. The architecture conversation has since been approved for [written design drafting](../superpowers/specs/2026-10-02-android-desktop-controller-design.md); that specification awaits user review and is not an approved implementation plan.

## Requested outcome

The Android APK must be only a controller/extension of the existing desktop application. It should reuse the desktop UI and expose desktop operations, while the desktop remains responsible for domain logic, files, database, queue ownership, playback decisions, and its existing output routing. The phone must not become another music player. A standalone Android player or a streaming-library client does not satisfy this requirement.

Source inspection supports the feasibility of this architecture, but it is not already implemented. Existing remote playback and upstream library providers are useful starting points; neither provides full desktop operation parity or desktop-authoritative state across the shared UI.

## Confirmed product scope

The user confirmed LAN-only operation, without cloud dependency or router port forwarding, and authorized-device pairing. The shared desktop design and functionality must adapt responsively to both phones and tablets; this is not a requirement to shrink the literal desktop window onto every screen. The desktop executes domain work and owns application state; the Android extension sends commands and displays confirmed state. These product constraints are approved. The subsequent architecture conversation is approved for written design drafting; the written specification and execution plan still require their own review.

## Inspected revisions and limits

- Local checkout: `fb43a012190e4ef6d086aab59b6056944864d869`, version 1.3.3; inspected read-only.
- Original upstream: `dupitydumb/Audion`, default branch `master`, HEAD `cb5d44ef81ee03194c3627d4e8dd19dbe5fc46c1` dated 2026-09-30. The pinned [package manifest](https://github.com/dupitydumb/Audion/blob/cb5d44ef81ee03194c3627d4e8dd19dbe5fc46c1/package.json#L1-L4) reports version 1.4.1. The [latest published release](https://github.com/dupitydumb/Audion/releases/tag/v1.4.1) was published on 2026-09-28.
- Metadata was checked through GitHub REST: [repository](https://api.github.com/repos/dupitydumb/Audion), [pinned commit](https://api.github.com/repos/dupitydumb/Audion/commits/cb5d44ef81ee03194c3627d4e8dd19dbe5fc46c1), and [complete pinned tree](https://api.github.com/repos/dupitydumb/Audion/git/trees/cb5d44ef81ee03194c3627d4e8dd19dbe5fc46c1?recursive=1).
- This was source inspection, not an end-to-end test of Android-to-desktop control. No fetch, merge, build, checkout, product-code edit, or commit was performed. GitHub's compare endpoint did not resolve the local fork revision against upstream; no ahead/behind count or complete merge recommendation is asserted.

## Upstream findings

### Partial remote playback control is not full desktop control

The client obtains a server URL, access token, and device ID, then opens a WebSocket to that server. It connects only when logged in and remote control is enabled. This is server-mediated device control, not direct discovery/pairing with a desktop-hosted API. See [socket connection](https://github.com/dupitydumb/Audion/blob/cb5d44ef81ee03194c3627d4e8dd19dbe5fc46c1/src/lib/stores/websocket.ts#L51-L85) and [connection gating](https://github.com/dupitydumb/Audion/blob/cb5d44ef81ee03194c3627d4e8dd19dbe5fc46c1/src/lib/stores/websocket.ts#L199-L214).

The remote receiver accepts `resume`, `pause`, `next`, `previous`, `seek`, `volume`, `shuffle`, and `repeat`. Remote player-state handling mirrors the current track, position, and playback toggles. There are no library, folder-management, metadata-editing, playlist, or queue-mutation commands in that receiver. See [remote command handling](https://github.com/dupitydumb/Audion/blob/cb5d44ef81ee03194c3627d4e8dd19dbe5fc46c1/src/lib/stores/player/remote.ts#L44-L106) and [remote state mirror](https://github.com/dupitydumb/Audion/blob/cb5d44ef81ee03194c3627d4e8dd19dbe5fc46c1/src/lib/stores/player/remote.ts#L108-L160).

This partial protocol already exists in the local baseline's `src/lib/stores/websocket.ts` and monolithic `src/lib/stores/player.ts`; upstream reorganizes playback into modules. It is not evidence of a newly completed controller-only app.

### Upstream adds a remote library provider seam

`src-tauri/src/sync/provider.rs` exists upstream and is absent from the inspected local checkout. Upstream chooses between `LocalProvider` and `ServerProvider`; library reads are routed through that provider. See [provider selection](https://github.com/dupitydumb/Audion/blob/cb5d44ef81ee03194c3627d4e8dd19dbe5fc46c1/src-tauri/src/sync/mod.rs#L140-L155) and [library command routing](https://github.com/dupitydumb/Audion/blob/cb5d44ef81ee03194c3627d4e8dd19dbe5fc46c1/src-tauri/src/commands/library.rs#L1095-L1126).

`ServerProvider` calls REST endpoints for tracks, albums, search, playlists, and likes. This is a potentially reusable client-side abstraction for remote data access, not proof that the existing desktop serves those endpoints. See [remote library reads](https://github.com/dupitydumb/Audion/blob/cb5d44ef81ee03194c3627d4e8dd19dbe5fc46c1/src-tauri/src/sync/provider.rs#L710-L783), [playlist mutations](https://github.com/dupitydumb/Audion/blob/cb5d44ef81ee03194c3627d4e8dd19dbe5fc46c1/src-tauri/src/sync/provider.rs#L795-L863), and [likes](https://github.com/dupitydumb/Audion/blob/cb5d44ef81ee03194c3627d4e8dd19dbe5fc46c1/src-tauri/src/sync/provider.rs#L866-L889).

Coverage is incomplete for full operation parity: `ServerProvider::delete_album` returns success without a request, and folder addition/scanning still accesses the calling application's local filesystem and database. See [album deletion stub](https://github.com/dupitydumb/Audion/blob/cb5d44ef81ee03194c3627d4e8dd19dbe5fc46c1/src-tauri/src/sync/provider.rs#L785-L793), [local scanning](https://github.com/dupitydumb/Audion/blob/cb5d44ef81ee03194c3627d4e8dd19dbe5fc46c1/src-tauri/src/commands/library.rs#L366-L389), and [local folder validation](https://github.com/dupitydumb/Audion/blob/cb5d44ef81ee03194c3627d4e8dd19dbe5fc46c1/src-tauri/src/commands/library.rs#L482-L504).

### Track selection still starts playback on the calling device

Upstream `playTrack` has no remote-command branch. Its implementation ultimately invokes HTML5 or native playback on the calling app. In contrast, pause/resume explicitly send commands when the active backend is remote. Therefore, a streaming-server connection plus the existing remote panel does not make every library action control desktop playback. See [track selection implementation](https://github.com/dupitydumb/Audion/blob/cb5d44ef81ee03194c3627d4e8dd19dbe5fc46c1/src/lib/stores/player/playback.ts#L232-L470), [local playback calls](https://github.com/dupitydumb/Audion/blob/cb5d44ef81ee03194c3627d4e8dd19dbe5fc46c1/src/lib/stores/player/playback.ts#L399-L435), and [remote pause/resume](https://github.com/dupitydumb/Audion/blob/cb5d44ef81ee03194c3627d4e8dd19dbe5fc46c1/src/lib/stores/player/playback.ts#L557-L589).

### Server documentation does not prove desktop controller support

The README describes a separate Docker streaming server and references an `audion-server-docker` directory. That directory is absent from the complete pinned default-branch tree. The documentation cannot establish that the server is available in this revision, nor that it controls the already-running desktop application's playback and operations. See [README claim](https://github.com/dupitydumb/Audion/blob/cb5d44ef81ee03194c3627d4e8dd19dbe5fc46c1/README.md#L114-L154) and [pinned repository tree](https://github.com/dupitydumb/Audion/tree/cb5d44ef81ee03194c3627d4e8dd19dbe5fc46c1).

## Upstream conclusion

Upstream has reusable pieces: the shared Svelte frontend, partial server-mediated player control, and a newer local/remote library-provider abstraction. The inspected revision does not satisfy the requested controller-only contract: full desktop operations and desktop-only playback ownership are not consistently routed through a desktop transport. Updating wholesale to version 1.4.1 should not be described as sufficient to deliver the requested APK.

## Local findings

The current Android app is an independent installation. Shared Rust startup creates app data, cover storage, a database, and backups; the mobile command handler exposes library/file and audio operations. See [lib.rs lines 309 to 361 and 729 to 893](C:/Users/olive/Documents/GitHub/Audion/src-tauri/src/lib.rs). The shared UI also loads its local library, starts Squeeze, and loads plugins at startup: [+page.svelte lines 78 to 123](C:/Users/olive/Documents/GitHub/Audion/src/routes/+page.svelte).

The existing cloud player receiver accepts eight basic playback controls, while state broadcasts contain now-playing metadata and playback toggles, not the complete queue or library. Broadcasts are disabled when the desktop uses Squeeze. See [player.ts lines 1104 to 1137 and 3104 to 3155](C:/Users/olive/Documents/GitHub/Audion/src/lib/stores/player.ts). Desktop-controlled streamer output therefore needs to be included explicitly in the proposed controller state contract rather than assumed to work through the current cloud mirror.

Squeeze also exposes a separate browser Now Playing page and HTTP control endpoints. These select an external renderer by MAC; they are not the shared Svelte interface or a general desktop interface. The inspected HTTP router has no pairing, authentication, or TLS layer. See [webui.rs lines 44 to 100 and 317 onward](C:/Users/olive/Documents/GitHub/Audion/src-tauri/src/squeeze/webui.rs) and [streaming.rs lines 349 to 364 and 392 to 408](C:/Users/olive/Documents/GitHub/Audion/src-tauri/src/squeeze/streaming.rs). Reusing that router unchanged for file and administrative operations would require a separate security assessment and is not recommended.

### The seam must cover commands and state ownership

- [tauri.ts lines 116 to 141](C:/Users/olive/Documents/GitHub/Audion/src/lib/api/tauri.ts) — [solid-dip] Invocation and events currently depend on the local Tauri runtime. A local adapter and remote adapter could vary at this seam, but this alone does not cover all application behavior.
- [player.ts lines 1433 to 1714 and 1739 to 1815](C:/Users/olive/Documents/GitHub/Audion/src/lib/stores/player.ts) — [core-separation-of-concerns] Track selection and queue orchestration live in Svelte and lack a remote-controller branch. Forwarding only low-level Rust audio commands would still let the phone own queue behavior and trigger local playback.
- [player.ts lines 2545 to 2580](C:/Users/olive/Documents/GitHub/Audion/src/lib/stores/player.ts) — [core-encapsulation] Queue insertion mutates local stores and schedules local playback preparation unless Squeeze is selected. Controller actions must delegate intent to the desktop and consume its result.
- [persist.ts lines 104 to 153](C:/Users/olive/Documents/GitHub/Audion/src/lib/stores/persist.ts) and [settings.ts lines 43 to 67](C:/Users/olive/Documents/GitHub/Audion/src/lib/stores/settings.ts) — [core-separation-of-concerns] Playback state and settings are persisted locally. A controller must distinguish local presentation/connection preferences from desktop-owned domain settings and never restore a stale phone queue over desktop state.

The shared Svelte views and presentation stores can be reused. However, Android currently chooses a mobile shell and MobileHome rather than literally displaying DesktopHome. See [+page.svelte lines 161 to 176](C:/Users/olive/Documents/GitHub/Audion/src/routes/+page.svelte), [MainView.svelte lines 742 to 748](C:/Users/olive/Documents/GitHub/Audion/src/lib/components/MainView.svelte), and [mobile.ts lines 23 to 26 and 55 to 57](C:/Users/olive/Documents/GitHub/Audion/src/lib/stores/mobile.ts). The user has now confirmed preserving the desktop design and functions with responsive phone and tablet layouts, not a literal scaled desktop window. Existing Android platform detection must not prevent an appropriate expanded tablet layout.

## Recommended direction

This is a proposed direction, not an approved implementation design. Keep one shared UI and place a desktop control module behind a typed interface for high-level commands, queries, and state subscriptions. A desktop adapter executes the existing host operations; an Android adapter sends intent and mirrors the host's confirmed state. Desktop execution may remain in existing TypeScript or Rust modules; moving everything to Rust is not a prerequisite.

The desktop remains the authority for library, queue, playback and selected output, metadata, application settings, and plugin execution. The APK retains only presentation state, connection credentials, and non-authoritative display caches. It must not initialize independent scanning, audio playback, plugin execution, cloud data merges, or its own authoritative music database. Covers should be served through authorized resource identifiers/URLs instead of converting desktop filesystem paths in the phone's Tauri runtime; see [tauri.ts lines 454 to 479](C:/Users/olive/Documents/GitHub/Audion/src/lib/api/tauri.ts).

Use desktop track IDs and explicit operation types, not a network endpoint that blindly exposes every Tauri command. Folder selection, destructive file actions, backups, plugin installation, and OS-specific operations need deliberate host-side workflows and authorization. Selecting a music directory must select the PC's directory, not the phone's storage.

The confirmed scope is LAN-only, so direct desktop connectivity is the recommended direction. The existing cloud relay is not part of the proposed controller path. Internet access and router port forwarding are out of scope. LAN scope still requires pairing, authorization, and deliberate connection security; exposing the existing unauthenticated Squeeze listener is not a substitute. Specific transport and pairing details remain part of the unapproved architecture design.

## Candidate milestones

These milestones describe the work order, not approved tasks or a promise of complete feature parity.

1. Define a capability inventory for every desktop operation, state ownership, and controller-only startup. Identify OS-specific adaptations and responsive phone/tablet layouts within the confirmed LAN-only scope.
2. Establish authenticated pairing, explicit host command dispatch, confirmations/errors, and an initial state snapshot. Demonstrate one track selection and queue action executing only on the desktop.
3. Route shared UI queries/actions/events through local or remote adapters. Add versioned desktop state updates and reconnect/resume rehydration; preserve existing desktop and Squeeze behavior.
4. Add operation parity by category: library/search, playlists/favorites, queue/playback/output selection, lyrics/metadata, settings, and guarded file/admin/plugin actions. Reuse upstream provider concepts selectively rather than merging version 1.4.1 as a supposed finished solution.
5. Validate real PC/Android behavior and build the controller APK. Test desktop-side changes, disconnect/reconnect, PC shutdown/restart, duplicate or failed commands, unauthorized access, desktop-controlled Squeeze output, and the absence of phone-local audio/scanning/database/plugin side effects.

## Open decisions

LAN-only scope and responsive phone/tablet presentation are confirmed. The next step is review of the proposed desktop control module and shared local/remote interface. Pairing behavior, host-side workflows for OS-specific or destructive actions, and the written design still require approval before an implementation plan is prepared. No product-code change, build, or end-to-end success claim is made by this review.
