import { afterEach, expect, it, vi } from "vitest";
import { get, writable } from "svelte/store";
import { createControllerSession, type ControllerNativeBridge, type ControllerState } from "./session";
import { createControllerPage } from "./views";
import { createReadAdmission } from "./read-recovery";
import type { ApplicationQuery, EventBatch, HostSnapshot, QueryResult, DisplayAlbum } from "../types";
const busy = { code: "busy", message: "PC is busy", retryable: true };
const album: DisplayAlbum = { id: 42, name: "PC album", artist: "Ada", year: 2020, qualityBadges: [], sortSummary: { artist: "Ada", year: 2020, dateAdded: null, name: "PC album" } };
const image = { mime: "image/png", bytes: [137, 80, 78, 71] };
function snapshot(hostId = "pc", hostEpoch = "epoch"): HostSnapshot {
    return { hostId, hostEpoch, revision: 0, revisions: { libraryRevision: 1, queueRevision: 2, outputRevision: 3, settingsRevision: 0 }, playback: { status: "paused", track: null, context: null, position: 0, duration: null, volume: .5, shuffle: false, repeat: "none" }, queue: { count: 0, currentEntryId: null }, output: { kind: "pc" }, outputs: [], capabilities: { queries: ["albums", "queue"], intents: ["pause"] }, settings: {}, jobs: [] };
}
function deferred<T>() { let resolve!: (v: T) => void, reject!: (e: unknown) => void; const promise = new Promise<T>((a, b) => { resolve = a; reject = b; }); return { promise, resolve, reject }; }
const pageResult = (): QueryResult => ({ type: "albums", page: { items: [album], nextCursor: "next", revision: 1 } });
function bridge() {
    return { browseMetadata: vi.fn<ControllerNativeBridge["browseMetadata"]>(), beginScope: vi.fn(async () => "scope"), connect: vi.fn(async (id: string) => ({ snapshot: snapshot(id), grants: { control: true } })), updateEndpoint: vi.fn(), suspend: vi.fn(async () => { }), forget: vi.fn(async () => { }), scan: vi.fn(), pair: vi.fn(), query: vi.fn<ControllerNativeBridge["query"]>(async () => pageResult()), media: vi.fn<ControllerNativeBridge["media"]>(async () => image), poll: vi.fn<ControllerNativeBridge["poll"]>(() => new Promise(() => { })), command: vi.fn<ControllerNativeBridge["command"]>(async () => ({ status: "applied", revision: 1 })), commandStatus: vi.fn<ControllerNativeBridge["commandStatus"]>() } satisfies ControllerNativeBridge;
}
const cleanup: (() => void)[] = [];
afterEach(() => { cleanup.splice(0).reverse().forEach(fn => fn()); vi.useRealTimers(); vi.restoreAllMocks(); });
async function settle() { for (let i = 0; i < 30; i++)
    await Promise.resolve(); }
async function setup() { vi.useFakeTimers(); const native = bridge(), session = createControllerSession(native); cleanup.push(() => session.suspendController()); await session.connectController("pc"); return { native, session }; }
it("optional metadata cannot queue behind two foreground leases", async () => {
    const admit = createReadAdmission(), signal = new AbortController().signal;
    const a = deferred<void>(), b = deferred<void>(), c = deferred<void>();
    const first = admit(signal, () => a.promise), second = admit(signal, () => b.promise);
    await settle();
    const optional = vi.fn(async () => 42);
    await expect(admit.tryRun(signal, optional)).rejects.toMatchObject({ code: "busy", retryable: false });
    expect(optional).not.toHaveBeenCalled();
    const foreground = admit(signal, () => c.promise);
    a.resolve(); await first; await settle();
    await expect(admit.tryRun(signal, optional)).rejects.toMatchObject({ code: "busy" });
    b.resolve(); await second; expect(await admit.tryRun(signal, optional)).toBe(42);
    c.resolve(); await foreground;
});
// Removing the session retry or changing its schedule breaks these real-port outcomes.
it.each(["query", "media"] as const)("%s recovers structured Busy at 200/500/1000ms without disconnect", async (kind) => {
    const { native, session } = await setup();
    let attempts = 0;
    if (kind === "query")
        native.query.mockImplementation(async () => { if (++attempts < 4)
            throw busy; return pageResult(); });
    else
        native.media.mockImplementation(async () => { if (++attempts < 4)
            throw busy; return image; });
    const outcome = (kind === "query" ? session.port.query({ type: "albums" }) : session.port.resolveArtwork({ resourceId: "cover", revision: 1 })).then(v => ({ ok: true, v }), e => ({ ok: false, e }));
    await settle();
    expect(attempts).toBe(1);
    await vi.advanceTimersByTimeAsync(199);
    expect(attempts).toBe(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(attempts).toBe(2);
    await vi.advanceTimersByTimeAsync(499);
    expect(attempts).toBe(2);
    await vi.advanceTimersByTimeAsync(1);
    expect(attempts).toBe(3);
    await vi.advanceTimersByTimeAsync(999);
    expect(attempts).toBe(3);
    await vi.advanceTimersByTimeAsync(1);
    expect(attempts).toBe(4);
    expect(await outcome).toMatchObject({ ok: true });
    expect(get(session.state).ready).toBe(true);
});
it.each(["query", "media"] as const)("%s stops exhausted Busy after four attempts", async (kind) => {
    const { native, session } = await setup();
    native.query.mockRejectedValue(busy);
    native.media.mockRejectedValue(busy);
    const outcome = (kind === "query" ? session.port.query({ type: "albums" }) : session.port.resolveArtwork({ resourceId: "cover", revision: 1 })).catch(e => e);
    await vi.advanceTimersByTimeAsync(1700);
    expect(await outcome).toEqual(busy);
    expect(kind === "query" ? native.query : native.media).toHaveBeenCalledTimes(4);
    await vi.advanceTimersByTimeAsync(10000);
    expect(kind === "query" ? native.query : native.media).toHaveBeenCalledTimes(4);
    expect(get(session.state).ready).toBe(true);
    expect(vi.getTimerCount()).toBe(0);
});
it.each([{ ...busy, retryable: false }, { ...busy, retryable: undefined }, { code: "unauthorized", retryable: true }, { code: "permission_required", retryable: true }, { code: "unsupported", retryable: true }, new Error("busy")])("does not retry non-structured/nonretryable/terminal failure %j", async (error) => {
    const { native, session } = await setup();
    native.query.mockRejectedValue(error);
    native.media.mockRejectedValue(error);
    await expect(session.port.query({ type: "albums" })).rejects.toEqual(error);
    await expect(session.port.resolveArtwork({ resourceId: "cover", revision: 1 })).rejects.toEqual(error);
    await vi.advanceTimersByTimeAsync(5000);
    expect(native.query).toHaveBeenCalledOnce();
    expect(native.media).toHaveBeenCalledOnce();
});
// Real transport entry order is observable: a missing shared limiter admits 4 calls.
it("limits query and media combined to two native calls with FIFO queue", async () => {
    const { native, session } = await setup(), held = [deferred<QueryResult>(), deferred<QueryResult>(), deferred<QueryResult>()], art = deferred<typeof image>(), entries: string[] = [];
    let q = 0;
    native.query.mockImplementation((_f, r) => { entries.push(r.type); return held[q++].promise; });
    native.media.mockImplementation(() => { entries.push("media"); return art.promise; });
    const pending = [session.port.query({ type: "albums" }), session.port.resolveArtwork({ resourceId: "cover", revision: 1 }), session.port.query({ type: "tracks" }), session.port.query({ type: "artists" })];
    pending.forEach(p => p.catch(() => { }));
    await settle();
    expect(entries).toEqual(["albums", "media"]);
    art.resolve(image);
    await settle();
    expect(entries).toEqual(["albums", "media", "tracks"]);
    held[0].resolve(pageResult());
    await settle();
    expect(entries).toEqual(["albums", "media", "tracks", "artists"]);
    held[1].resolve({ type: "tracks", page: { items: [], nextCursor: null, revision: 1 } });
    held[2].resolve({ type: "artists", page: { items: [], nextCursor: null, revision: 1 } });
    await Promise.all(pending);
});
it("retry sleepers release admission and rejoin FIFO behind waiting reads", async () => {
    const { native, session } = await setup(), held = deferred<typeof image>(), second = deferred<QueryResult>(), third = deferred<QueryResult>();
    const entries: string[] = [];
    let a = 0;
    native.media.mockReturnValue(held.promise);
    native.query.mockImplementation((_f, q) => { entries.push(q.type); if (q.type === "albums") {
        if (++a === 1)
            return Promise.reject(busy);
        return Promise.resolve(pageResult());
    } return q.type === "tracks" ? second.promise : third.promise; });
    const art = session.port.resolveArtwork({ resourceId: "cover", revision: 1 }), first = session.port.query({ type: "albums" }), next = session.port.query({ type: "tracks" }), tail = session.port.query({ type: "artists" });
    [art, first, next, tail].forEach(p => p.catch(() => { }));
    await settle();
    expect(entries).toEqual(["albums", "tracks"]);
    await vi.advanceTimersByTimeAsync(200);
    expect(entries).toEqual(["albums", "tracks"]);
    second.resolve({ type: "tracks", page: { items: [], nextCursor: null, revision: 1 } });
    await settle();
    expect(entries).toEqual(["albums", "tracks", "artists"]);
    third.resolve({ type: "artists", page: { items: [], nextCursor: null, revision: 1 } });
    await settle();
    expect(entries).toEqual(["albums", "tracks", "artists", "albums"]);
    held.resolve(image);
    await Promise.all([art, first, next, tail]);
});
it("removes cancelled queued reads before native entry and keeps an in-flight cancelled slot occupied", async () => {
    const { native, session } = await setup(), one = deferred<QueryResult>(), two = deferred<typeof image>(), signal = new AbortController(), inFlight = new AbortController();
    native.query.mockReturnValue(one.promise);
    native.media.mockReturnValue(two.promise);
    const first = session.port.query({ type: "albums" }, inFlight.signal).catch(e => e), art = session.port.resolveArtwork({ resourceId: "cover", revision: 1 }).catch(e => e), queued = session.port.query({ type: "albums" }, signal.signal).catch(e => e), later = session.port.query({ type: "albums" }).catch(e => e);
    await settle();
    signal.abort();
    inFlight.abort();
    await settle();
    expect(native.query).toHaveBeenCalledOnce();
    expect(await queued).toMatchObject({ code: "resync_required" });
    expect(await first).toMatchObject({ code: "resync_required" });
    one.resolve(pageResult());
    await settle();
    expect(native.query).toHaveBeenCalledTimes(2);
    two.resolve(image);
    await Promise.all([art, later]);
});
it.each(["abort", "suspend", "hidden", "host", "epoch", "library", "queue"] as const)("fences pending Busy retry on %s with no surviving backoff", async (reason) => {
    const { native, session } = await setup(), poll = deferred<EventBatch>();
    // Replace the current held poll via a fresh generation before starting the read.
    native.poll.mockReturnValueOnce(poll.promise);
    await session.connectController("pc");
    native.query.mockRejectedValue(busy);
    const signal = new AbortController();
    const pending = session.port.query({ type: reason === "queue" ? "queue" : "albums" }, signal.signal).catch(e => e);
    await settle();
    expect(native.query).toHaveBeenCalledOnce();
    if (reason === "abort")
        signal.abort();
    else if (reason === "suspend")
        session.suspendController();
    else if (reason === "hidden")
        session.visibilityChanged(false, "pc");
    else if (reason === "host" || reason === "epoch") {
        native.connect.mockResolvedValueOnce({ snapshot: snapshot(reason === "host" ? "two" : "pc", "new-epoch"), grants: { control: true } });
        await session.connectController(reason === "host" ? "two" : "pc");
    }
    else {
        poll.resolve({ hostEpoch: "epoch", revision: 1, events: reason === "library" ? [{ type: "library", revision: 1, libraryRevision: 2 }] : [{ type: "queue", revision: 1, queueRevision: 3, queue: { count: 0, currentEntryId: null } }] });
    }
    await settle();
    expect(await pending).toMatchObject({ code: "resync_required" });
    expect(vi.getTimerCount()).toBe(0);
    await vi.advanceTimersByTimeAsync(5000);
    expect(native.query).toHaveBeenCalledOnce();
});
it.each(["library", "queue"] as const)("rejects late %s query results after relevant revision change", async (domain) => {
    const { native, session } = await setup(), poll = deferred<EventBatch>(), held = deferred<QueryResult>();
    native.poll.mockReturnValueOnce(poll.promise);
    await session.connectController("pc");
    native.query.mockReturnValue(held.promise);
    const pending = session.port.query({ type: domain === "queue" ? "queue" : "albums" }).catch(e => e);
    await settle();
    poll.resolve({ hostEpoch: "epoch", revision: 1, events: domain === "library" ? [{ type: "library", revision: 1, libraryRevision: 2 }] : [{ type: "queue", revision: 1, queueRevision: 3, queue: { count: 0, currentEntryId: null } }] });
    await settle();
    held.resolve(domain === "library" ? pageResult() : { type: "queue", page: { items: [], nextCursor: null, revision: 2 } });
    expect(await pending).toMatchObject({ code: "resync_required" });
});
it("poll Busy recovery preserves readiness, uses current cursor sequentially, and never retries commands", async () => {
    const native = bridge();
    vi.useFakeTimers();
    let p = 0;
    native.poll.mockImplementation(async () => { if (++p < 4)
        throw busy; return new Promise(() => { }); });
    native.command.mockRejectedValue(busy);
    const session = createControllerSession(native);
    cleanup.push(() => session.suspendController());
    await session.connectController("pc");
    await settle();
    expect(get(session.state).ready).toBe(true);
    expect(native.poll).toHaveBeenCalledOnce();
    await expect(session.port.execute({ type: "pause" }, { hostEpoch: "epoch", outputRevision: 3 })).rejects.toEqual(busy);
    await vi.advanceTimersByTimeAsync(1700);
    expect(native.poll).toHaveBeenCalledTimes(4);
    expect(native.poll.mock.calls.map(c => c[1])).toEqual(Array(4).fill({ hostEpoch: "epoch", revision: 0 }));
    expect(native.connect).toHaveBeenCalledOnce();
    expect(native.command).toHaveBeenCalledOnce();
    expect(get(session.state).ready).toBe(true);
});
it("exhausted poll Busy falls back to disconnected logic only after fourth attempt", async () => {
    vi.useFakeTimers();
    const native = bridge();
    native.poll.mockRejectedValue(busy);
    const session = createControllerSession(native, { random: () => 1 });
    cleanup.push(() => session.suspendController());
    await session.connectController("pc");
    await settle();
    expect(get(session.state).ready).toBe(true);
    await vi.advanceTimersByTimeAsync(1699);
    expect(get(session.state).ready).toBe(true);
    await vi.advanceTimersByTimeAsync(1);
    expect(native.poll).toHaveBeenCalledTimes(4);
    expect(get(session.state)).toMatchObject({ ready: false, status: "unavailable", error: busy });
    session.suspendController();
    expect(vi.getTimerCount()).toBe(0);
});
it("suspend aborts poll backoff without another native poll or reconnect", async () => {
    const native = bridge();
    vi.useFakeTimers();
    native.poll.mockRejectedValue(busy);
    const session = createControllerSession(native);
    await session.connectController("pc");
    await settle();
    session.suspendController();
    expect(vi.getTimerCount()).toBe(0);
    await vi.advanceTimersByTimeAsync(10000);
    expect(native.poll).toHaveBeenCalledOnce();
    expect(native.connect).toHaveBeenCalledOnce();
});
it.each(["albums", "album_detail"] as const)("unchanged %s refresh retains valid data through loading and exhausted Busy", async (type) => {
    const { native, session } = await setup(), detail = { album, originalYear: null, trackCount: 10, liked: false };
    native.query.mockResolvedValueOnce(type === "albums" ? pageResult() : { type: "album_detail", detail, revision: 1 });
    const page = createControllerPage(() => session.port, session.state);
    cleanup.push(page.dispose);
    page.setQuery(type === "albums" ? { type: "albums" } : { type: "album_detail", albumId: 42 });
    await settle();
    const before = get(page.state);
    native.query.mockRejectedValue(busy);
    page.refresh();
    // An already visible page must not show the initial-loading layout again.
    expect(get(page.state)).toMatchObject({ items: before.items, loading: false });
    expect(get(page.state).detail).toEqual(before.detail);
    page.refresh();
    await settle();
    await vi.advanceTimersByTimeAsync(1700);
    expect(get(page.state)).toMatchObject({ items: before.items, loading: false, error: busy.message });
    expect(get(page.state).detail).toEqual(before.detail);
    expect(native.query).toHaveBeenCalledTimes(5);
});
it("refresh replaces the first page and cursor atomically without mixing old pagination", async () => {
    const { native, session } = await setup(), held = deferred<QueryResult>();
    const page = createControllerPage(() => session.port, session.state);
    cleanup.push(page.dispose);
    page.setQuery({ type: "albums" });
    await settle();
    native.query.mockReturnValue(held.promise);
    page.refresh();
    void page.more();
    page.refresh();
    await settle();
    expect(native.query).toHaveBeenCalledTimes(2);
    expect(native.query.mock.calls[1][1]).toEqual({ type: "albums", limit: 100 });
    held.resolve({ type: "albums", page: { items: [{ ...album, id: 43 }], nextCursor: "replacement", revision: 1 } });
    await settle();
    expect(get(page.state)).toMatchObject({ items: [{ ...album, id: 43 }], nextCursor: "replacement" });
});
it.each(["query", "host", "epoch", "library", "disconnect", "dispose", "queue"] as const)("page drops superseded refresh on %s, retaining presentation only for compatible library changes", async (change) => {
    vi.useFakeTimers();
    const initial = { ready: true, status: "connected", currentHostId: "pc", snapshot: snapshot() } as ControllerState, connection = writable(initial), held = deferred<QueryResult>();
    const q: ApplicationQuery = change === "queue" ? { type: "queue" } : { type: "albums" };
    const entry = { entryId: "entry", track: { id: 42, title: "Track", artist: "Ada", albumId: 42, album: "PC album", duration: 120, trackNumber: 1, discNumber: 1, quality: { format: "flac", bitrate: null, badges: [] } } };
    const initialResult: QueryResult = change === "queue" ? { type: "queue", page: { items: [entry], nextCursor: null, revision: 2 } } : pageResult();
    const query = vi.fn().mockResolvedValueOnce(initialResult).mockReturnValue(held.promise);
    const page = createControllerPage(() => ({ query } as any), connection);
    cleanup.push(page.dispose);
    page.setQuery(q);
    await settle();
    expect(get(page.state).items).toEqual(change === "queue" ? [entry] : [album]);
    page.refresh();
    await settle();
    if (change === "query")
        page.setQuery({ type: "albums", likedOnly: true });
    else if (change === "dispose")
        page.dispose();
    else
        connection.set(change === "disconnect" ? { ...initial, ready: false, snapshot: null } : { ...initial, snapshot: { ...initial.snapshot!, hostId: change === "host" ? "two" : "pc", hostEpoch: change === "epoch" ? "new" : "epoch", revisions: { ...initial.snapshot!.revisions, libraryRevision: change === "library" ? 2 : 1, queueRevision: change === "queue" ? 3 : 2 } } });
    expect(get(page.state).items).toEqual(change === "library" ? [album] : []);
    held.resolve(initialResult);
    await settle();
    if (change !== "query" && change !== "host" && change !== "epoch")
        expect(get(page.state).items).toEqual(change === "library" ? [album] : []);
});
it("disposes artwork cancelled during Blob handoff before consumer ownership", async () => {
    const { native, session } = await setup(), signal = new AbortController();
    vi.spyOn(URL, "createObjectURL").mockImplementation(() => { queueMicrotask(() => signal.abort()); return "blob:handoff"; });
    const revoke = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => { });
    await expect(session.port.resolveArtwork({ resourceId: "cover", revision: 1 }, signal.signal)).rejects.toMatchObject({ code: "resync_required" });
    await settle();
    expect(revoke).toHaveBeenCalledExactlyOnceWith("blob:handoff");
});
it.each(["suspend", "host", "library", "queue"] as const)("queued %s invalidation rejects without native entry", async (change) => {
    const { native, session } = await setup(), poll = deferred<EventBatch>(), held = deferred<QueryResult>(), art = deferred<typeof image>();
    native.poll.mockReturnValueOnce(poll.promise);
    await session.connectController("pc");
    native.query.mockReturnValue(held.promise);
    native.media.mockReturnValue(art.promise);
    const first = session.port.query({ type: "albums" }).catch(e => e), second = session.port.resolveArtwork({ resourceId: "cover", revision: 1 }).catch(e => e);
    await settle();
    const queued = session.port.query({ type: change === "queue" ? "queue" : "albums" }).catch(e => e);
    await settle();
    expect(native.query).toHaveBeenCalledOnce();
    if (change === "suspend")
        session.suspendController();
    else if (change === "host")
        await session.connectController("two");
    else
        poll.resolve({ hostEpoch: "epoch", revision: 1, events: change === "library" ? [{ type: "library", revision: 1, libraryRevision: 2 }] : [{ type: "queue", revision: 1, queueRevision: 3, queue: { count: 0, currentEntryId: null } }] });
    await settle();
    expect(await queued).toMatchObject({ code: "resync_required" });
    held.resolve(pageResult());
    art.resolve(image);
    await Promise.all([first, second]);
    await settle();
    expect(native.query).toHaveBeenCalledOnce();
});
it.each(["library", "queue"] as const)("unrelated %s revision leaves a valid safe read alive", async (changed) => {
    const { native, session } = await setup(), poll = deferred<EventBatch>(), held = deferred<QueryResult>();
    native.poll.mockReturnValueOnce(poll.promise);
    await session.connectController("pc");
    native.query.mockReturnValue(held.promise);
    const pending = session.port.query({ type: changed === "library" ? "queue" : "albums" });
    await settle();
    poll.resolve({ hostEpoch: "epoch", revision: 1, events: changed === "library" ? [{ type: "library", revision: 1, libraryRevision: 2 }] : [{ type: "queue", revision: 1, queueRevision: 3, queue: { count: 0, currentEntryId: null } }] });
    await settle();
    const expected: QueryResult = changed === "library" ? { type: "queue", page: { items: [], nextCursor: null, revision: 2 } } : pageResult();
    held.resolve(expected);
    expect(await pending).toEqual(expected);
});
it("poll retries from the most recently confirmed cursor and has one pending call", async () => {
    vi.useFakeTimers();
    const native = bridge(), held = deferred<EventBatch>();
    let count = 0;
    native.poll.mockImplementation(async () => { count++; if (count === 1)
        return { hostEpoch: "epoch", revision: 1, events: [{ type: "library", revision: 1, libraryRevision: 2 }] }; if (count === 2)
        throw busy; return held.promise; });
    const session = createControllerSession(native);
    cleanup.push(() => session.suspendController());
    await session.connectController("pc");
    await settle();
    expect(native.poll.mock.calls.map(c => c[1])).toEqual([{ hostEpoch: "epoch", revision: 0 }, { hostEpoch: "epoch", revision: 1 }]);
    await vi.advanceTimersByTimeAsync(200);
    expect(native.poll.mock.calls[2][1]).toEqual({ hostEpoch: "epoch", revision: 1 });
    await vi.advanceTimersByTimeAsync(5000);
    expect(native.poll).toHaveBeenCalledTimes(3);
});
it("unchanged page refresh retains data on display-only error, then replaces on manual success", async () => {
    const { native, session } = await setup();
    const page = createControllerPage(() => session.port, session.state);
    cleanup.push(page.dispose);
    page.setQuery({ type: "albums" });
    await settle();
    native.query.mockRejectedValueOnce({ code: "not_found", message: "Missing", retryable: false });
    page.refresh();
    await settle();
    expect(get(page.state)).toMatchObject({ items: [album], error: "Missing", loading: false });
    native.query.mockResolvedValueOnce({ type: "albums", page: { items: [{ ...album, id: 99 }], nextCursor: null, revision: 1 } });
    page.refresh();
    await settle();
    expect(get(page.state)).toMatchObject({ items: [{ ...album, id: 99 }], error: "", nextCursor: null });
});
it("returning to the same library view reuses confirmed session data, but explicit refresh rereads the PC", async () => {
    const { native, session } = await setup();
    const first = createControllerPage(() => session.port, session.state);
    first.setQuery({ type: "albums" }); await settle(); expect(get(first.state).items).toEqual([album]); first.dispose();
    const returned = createControllerPage(() => session.port, session.state); cleanup.push(returned.dispose);
    returned.setQuery({ type: "albums" }); await settle(); expect(get(returned.state).items).toEqual([album]); expect(native.query).toHaveBeenCalledOnce();
    returned.refresh(); await settle(); expect(native.query).toHaveBeenCalledTimes(2);
});
it("sort change aborts retained refresh, clears immediately, and ignores old completion", async () => {
    const { native, session } = await setup(), old = deferred<QueryResult>(), replacement = deferred<QueryResult>();
    const page = createControllerPage(() => session.port, session.state);
    cleanup.push(page.dispose);
    page.setQuery({ type: "albums", sort: "artist-asc" });
    await settle();
    native.query.mockReturnValueOnce(old.promise).mockReturnValueOnce(replacement.promise);
    page.refresh();
    await settle();
    expect(get(page.state).items).toEqual([album]);
    page.setQuery({ type: "albums", sort: "year-desc" });
    expect(get(page.state).items).toEqual([]);
    await settle();
    old.resolve(pageResult());
    await settle();
    expect(get(page.state).items).toEqual([]);
    replacement.resolve({ type: "albums", page: { items: [{ ...album, id: 77 }], nextCursor: null, revision: 1 } });
    await settle();
    expect(get(page.state).items).toEqual([{ ...album, id: 77 }]);
});
