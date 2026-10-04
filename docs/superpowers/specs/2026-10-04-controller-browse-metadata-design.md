# Integrated controller browse metadata

Status: Integrated delivery approved; implementation in progress. Root metadata source/tests implemented; UI integration, refreshed full verification and coordinated deployment remain pending.
Date: 2026-10-04

## Intent and success criteria

Expose the PC-authoritative play count of a local track and the complete duration of an album in the Android controller. Routine playback must remain quiet, and statistics refresh must not clear artwork, rotate media capabilities, reset navigation, or resize the album grid. Keep the PC as the library and accounting authority; do not introduce a phone database, a separate music server, or local play-count increments.

The user accepted integrated delivery after clarification. The implementation plan was reviewed and inline execution approved. Existing shared-checkout source and UI ownership must be preserved. This proposal is not committed.

## Packaging clarification and review status

The user requires this functionality to be integrated into the existing desktop build and the existing Android APK. There is no separately installed extension, plugin, service, add-on APK or second product. The proposed metadata module/route is internal code shipped in those same binaries.

The APK is still pre-release. Supporting older experimental builds is not a requirement. Use one internal typed metadata read in the same binaries, without a separate product, support probe, capability negotiation, legacy fallback cache or old-version compatibility test matrix. Desktop and APK are updated together for this feature. The independent read path exists to prevent statistics refresh from invalidating catalog/artwork, not to package an optional add-on.

## Current evidence

- `src-tauri/src/db/queries.rs`: `Track` contains persisted `play_count` and `duration`.
- `src-tauri/src/controller/query_input.rs`: the restricted track SELECT deliberately omits play_count and constructs it as None.
- `src-tauri/src/controller/queries.rs`: DisplayTrack and AlbumDetail publication omit these requested metrics.
- `src-tauri/src/controller/protocol.rs`: display DTOs use deny_unknown_fields. Adding a supposedly optional JSON property still breaks an older native decoder.
- `src/lib/application/types.ts`: DisplayTrack has duration but no playCount; AlbumDetail has no total duration.
- `src/lib/components/AlbumDetail.svelte`: desktop totals track durations. The controller cannot safely copy that calculation over a bounded, paginated page.
- `src-tauri/src/db/schema.rs`: play_count-only updates no longer advance the controller Library stamp. Restoring that invalidation would undo the flicker correction.
- `src-tauri/src/controller/host.rs`: unknown authenticated routes return NotFound; existing queries/resources use a shared per-device QuerySlot.
- `src-tauri/src/controller/client.rs`: the pinned native transport preserves structured errors and exposes only fixed native route literals.

These are source findings, not claims that the extension already exists or has passed runtime tests.

## Approaches and proposed choice

1. **Integrated typed read-only metadata module (selected):** add one authenticated route inside the existing binaries and refresh its display values independently of catalog/artwork. No support probe or old-build compatibility machinery. Cost: bounded foreground-only reads, rather than pushed statistics.
2. **Statistics revision/event in the main protocol:** pushes updates but expands shared snapshot/event publication and lifecycle paths. More work than these two metrics justify at this stage.
3. **Derive values in the controller:** incomplete album pages cannot produce a correct total and the controller does not own play history. Rejected.

The selected internal read leaves the existing catalog/artwork DTOs unchanged for locality and to avoid accidentally coupling counters to Library invalidation. This is not a promise to support earlier experimental PC/APK pairs.

## Module, interface and seam

One browse metadata module owns bounded PC reads, strict validation, presentation freshness. Its external seam is a typed optional ApplicationPort method:

```ts
queryBrowseMetadata?(query: BrowseMetadataQuery, signal?: AbortSignal): Promise<BrowseMetadataResult>;
```

The controller adapter implements it over a fixed native IPC request and fixed pinned-TLS route. The optional ApplicationPort method is an adapter seam: non-controller desktop adapters need not implement a controller-only display read. The controller build must implement it; optionality is not a runtime extension discovery mechanism. No generic URL, SQL, Tauri command or remote invoke is exposed. UI callers receive values/state; they do not manage native authentication, timers, revision checks or retry policy independently.

A single active browse metadata owner lives with the current controller browse presentation. It reads a bounded set of currently rendered/loaded local track IDs and, for album detail, the album ID. It exposes a small readonly presentation state with track-count map, optional album duration, freshness and an inline read error. The count map is non-authoritative presentation, scoped to the current authenticated session and current page, not a replica of the library.

## Wire contract

New fixed POST route: `/control/v1/browse-metadata`.

Request example:

```json
{
  "metadataVersion": 1,
  "hostEpoch": "confirmed-epoch",
  "libraryRevision": 7,
  "trackIds": [42, 43],
  "albumId": 8
}
```

Reply example:

```json
{
  "metadataVersion": 1,
  "hostEpoch": "confirmed-epoch",
  "libraryRevision": 7,
  "tracks": [
    { "trackId": 42, "playCount": 0 },
    { "trackId": 43, "playCount": 12 }
  ],
  "album": { "albumId": 8, "totalDurationSeconds": 585 }
}
```

Rules:

- metadataVersion is exactly 1. Both DTOs are strict, reject unknown fields, and validate all numeric values before use.
- trackIds has at most 1,000 entries before deduplication. IDs are positive safe integers; duplicates are deduplicated. albumId is optional and positive. Provider/synthetic negative IDs are not local DB keys and are never sent.
- An empty trackIds list is valid only when albumId is present. Empty no-data support probes are not implemented.
- A missing requested local track or missing/invalid persisted count returns playCount:null. A real zero remains zero. Counts are nonnegative safe integers; negative, absent or out-of-range values are unavailable, never fabricated zeros.
- No album property is returned unless albumId was requested. A missing album produces structured NotFound for that data request; it is not treated as an unsupported host.
- totalDurationSeconds is the complete album's duration in seconds, independently of loaded track pages, search text or the liked-only filter. It is zero for a genuinely empty existing album. If any member has missing/invalid duration, or the checked total cannot be represented safely, return null rather than a partial total presented as complete.
- Reply track IDs must belong to the request, with exactly one reply entry per unique requested ID. Album identity must match. A mismatch, duplicate, excess entry, unknown metadata version or invalid number is rejected without adopting any values.
- No filesystem paths, URLs, provider credentials, history records, raw media, cover references, or permission-bearing capabilities are in this response.

## Integrated build rollout

Ship the typed PC read and its Android consumer in the existing desktop build and APK. Do not create another server, plugin, add-on or independent installation. Update both binaries together, preserving pairing/data.

There is no support probe or legacy-host fallback state. A missing route/unsupported native read on a mismatched experimental build is a metadata error that tells the user to update the desktop/APK pair; it must not silently invent values or disconnect otherwise valid playback. Normal entity NotFound, malformed data, authentication, TLS and Busy errors retain their actual semantics. Existing desktop local/native handlers are not removed to simplify this controller feature.

## PC reads and security

Use the same authenticated read admission and QuerySlot as catalog queries/resources, with native lifecycle fencing and authorization rechecks before and after the admitted work. Browse-only paired devices may read metrics; mutation permission is not needed. Revoked/unpaired devices receive no metadata.

Under the existing DB capture discipline, capture requested track counts and the album-wide duration from a consistent read snapshot. Bind IDs with parameters; bounded chunks must respect SQLite parameter limits. Release DB locks before serialization/network work. Aggregate duration with checked arithmetic and explicit unknown-member handling. Recheck catalog stamp/epoch and the requested confirmed library revision before publishing; a catalog change yields resync_required, not mixed-revision values.

The endpoint is always a fresh read, not served from the session navigation page cache. Statistics-only changes deliberately need no Library revision: request/generation fencing and a single in-flight read prevent late adoption; a fresh request reads the latest committed PC counts. No statistics revision table, extra triggers, or host background statistics timer is required for this scope.

Cap track IDs at 1,000, request JSON at 64 KiB, successful response at 256 KiB, and retain the existing bounded structured-error body limit. Share current request/read quotas; do not create an unbounded separate admission lane. No secrets or raw user library contents in diagnostic receipts.

## Freshness and cancellation

- Fetch immediately after the relevant browse page becomes confirmed and after a relevant set of displayed IDs/album changes.
- While that presentation is active and the app is foreground/connected, refresh no more often than every 5 seconds, scheduling the next interval after completion. At most one metadata request is in flight; coalesce changes and abort outdated work.
- Reuse the existing safe-read admission and bounded transient retry implementation. Do not add a new retry loop or let optional statistics starve catalog/artwork requests. If the admission is busy, defer the optional refresh to the next interval.
- In a healthy foreground connection, a newly committed PC count becomes visible on the next refresh, plus read/transport latency. This is not a guarantee during network failure or while suspended.
- Pause and abort on hidden/background state, disconnect, host/epoch/grant scope change, navigation disposal, or library revision change. Clear cross-scope data; reacquire current page IDs after the ordinary catalog refresh. Resume with a fresh read on foreground return.
- A failure can retain already confirmed same-scope metric values with stale status; never replace them with invented zero or total. Mismatched builds show an inline metadata update/error explanation. Do not disconnect playback merely because an optional metadata read fails.
- This module never calls page.refresh for a statistics update, emits a Library event, clears the query/image cache, disposes artwork, or changes an artwork reference.

## UI integration and ownership

Root owns native transport, protocol extension DTOs, PC capture, ApplicationPort/controller adapter and metadata owner. The existing UI session owns ControllerBrowse, shared TrackRows and their presentation tests. Its source changes require an explicit narrow assignment after this design/plan is approved; do not overwrite its current work.

- TrackRows accepts passive count/freshness data and displays PC counts in its existing Plays column. Known 0 is displayed as 0; unknown is a dash with an accessible explanation. Provider tracks remain unknown rather than inheriting a local album count.
- Controller album detail shows the complete PC-supplied duration in its existing metadata row. Unknown total is labeled unavailable; neither a paginated subtotal nor a liked-subset total is called the album duration.
- Remove only the now-obsolete play-count/total-duration missing-projection explanations when the metadata is available. Keep truthful notices about release metadata, favorite mutation and liked-track totals, which are not implemented here.
- Statistics success/pending refreshes introduce no outcome panel. Optional metadata errors are inline in the relevant presentation; existing command error-only feedback is unchanged.
- No desktop play-count accounting changes, global sort controls, statistics-backed ordering, album favorite mutation, release enrichment, liked totals, artwork/theme redesign, or full UI parity claim.

## Validation and acceptance

Implementation uses RED/GREEN tests at the same interfaces callers use:

1. Real DB fixture: local counts 0/positive/null/invalid; complete multi-disc album total across more than one controller page; liked-only/filter independence; empty album; unknown/invalid duration; safe-range aggregate.
2. Strict Rust/TypeScript/native DTO parity: version, IDs, bounds, duplicate/unrequested results, provider IDs, unknown fields, nullable metrics and epoch/revision rejection.
3. Host/auth tests: unpaired/revoked/browse-only devices, shared QuerySlot/Busy, cancellation, bounded requests/responses, no sensitive fields, auth/catalog changes during read.
4. Mismatched-build/error isolation: a missing metadata route produces a truthful inline update/error explanation, not invented counts, pairing reset or a playback-session disconnect. Auth/TLS faults retain their semantics; no support probing or fallback state exists.
5. Owner/session tests with injected clock: one in-flight refresh; interval gating; latest generation only; foreground pause/resume; navigation disposal; query/image cache unchanged; no artwork handle disposed or reference identity changed by count-only updates.
6. Retain the real DB regression proving play_count-only writes leave Library stamp unchanged. Metadata sees the updated count on a subsequent fresh read.
7. Actual shared UI rendering: known 0, unknown, complete duration, stale state and obsolete-copy suppression, with native desktop handlers untouched.
8. Full Vitest/check/build and native tests. Report all failures, including the ten currently documented checker baseline errors; do not silently expand scope or weaken assertions.
9. Updated host + installed APK end-to-end: real natural playback accounting advances a displayed count on the next foreground refresh while the album grid/cover pixels remain available; compare album duration with the full PC membership read. Test background/resume and missing-route/error isolation, preserving pairing/data. Use reversible fixtures/isolated test DBs instead of rewriting the user's actual play history.

The implementation ledger records executed tests. Source verification does not imply deployment or runtime acceptance of the integrated metadata feature.

## Self-review

- Unknown values and genuine zero are distinct.
- No totals from partial pages or client-side statistics accounting.
- Existing catalog/artwork DTOs and event stream stay unchanged for isolation; old experimental-build support is not a requirement.
- No support discovery or compatibility layer; ordinary metadata read errors cannot downgrade authentication.
- Polling is foreground/page-scoped, single-flight and uses existing admission.
- Statistics reads cannot invalidate album artwork.
- UI-owned sources, unrelated features, source baselines and existing user data stay outside root's unapproved edit scope.

Next gate: complete UI integration, fresh integrated verification and coordinated deployment; preserve live playback/pairing/data.
