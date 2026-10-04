import { afterEach, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import { createControllerSession, type AuthenticatedConnection, type ControllerNativeBridge } from "./session";
import type { HostSnapshot, EventBatch, QueryResult } from "../types";
const snapshot = (hostId = "one", hostEpoch = "epoch"): HostSnapshot => ({
    hostId, hostEpoch, revision: 0, revisions: { libraryRevision: 1, queueRevision: 2, outputRevision: 3, settingsRevision: 0 }, playback: {
        status: "paused", track: null, context: null, position: 0, duration: null, volume: 0.5, shuffle: false, repeat: "none"
    }, queue: { count: 0, currentEntryId: null }, output: { kind: "pc" }, outputs: [], capabilities: { queries: ["snapshot", "tracks"], intents: ["pause"] }, settings: {}, jobs: []
});
function deferred<T>() {
    let resolve!: (value: T) => void;
    let reject!: (error: unknown) => void;
    const promise = new Promise<T>((a, b) => {
        resolve = a;
        reject = b;
    });
    return { promise, resolve, reject };
}
function bridge() {
    return {
        updateEndpoint: vi.fn<ControllerNativeBridge["updateEndpoint"]>(),
        beginScope: vi.fn<ControllerNativeBridge["beginScope"]>(async () => "scope"), connect: vi.fn<ControllerNativeBridge["connect"]>(async (hostId: string) => ({ snapshot: snapshot(hostId), grants: { control: true } })), suspend: vi.fn<ControllerNativeBridge["suspend"]>(async () => {
        }), forget: vi.fn<ControllerNativeBridge["forget"]>(async () => {
        }), scan: vi.fn<ControllerNativeBridge["scan"]>(), pair: vi.fn<ControllerNativeBridge["pair"]>(), query: vi.fn<ControllerNativeBridge["query"]>(async () => ({ type: "tracks", page: { items: [], nextCursor: null, revision: 1 } } as QueryResult)), command: vi.fn<ControllerNativeBridge["command"]>(async () => ({ status: "applied" as const, revision: 1 })), commandStatus: vi.fn<ControllerNativeBridge["commandStatus"]>(async () => ({ status: "applied" as const, revision: 1 })), poll: vi.fn<ControllerNativeBridge["poll"]>(() => new Promise<EventBatch>(() => {
        })), media: vi.fn<ControllerNativeBridge["media"]>()
    } satisfies ControllerNativeBridge;
}
afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
});
it("offline_mutations_are_not_queued", async () => {
    const n = bridge(), s = createControllerSession(n);
    await expect(s.port.execute({ type: "pause" }, { hostEpoch: "epoch", outputRevision: 3 })).rejects.toMatchObject({ code: "host_not_ready" });
    await s.connectController("one");
    expect(n.command).not.toHaveBeenCalled();
    s.suspendController();
});
it("snapshot_precedes_enabled_controls", async () => {
    const n = bridge(), d = deferred<AuthenticatedConnection>();
    n.connect.mockReturnValue(d.promise);
    const s = createControllerSession(n), p = s.connectController("one");
    expect(get(s.state).ready).toBe(false);
    await Promise.resolve();
    d.resolve({ snapshot: snapshot(), grants: { control: true } });
    await p;
    expect(get(s.state)).toMatchObject({ ready: true, snapshot: { hostId: "one" } });
    expect(n.poll).toHaveBeenCalledTimes(1);
    s.suspendController();
});
it("timeout_queries_outcome_without_replay", async () => {
    const n = bridge();
    n.command.mockRejectedValue({ code: "outcome_unknown" });
    const s = createControllerSession(n);
    await s.connectController("one");
    expect(await s.port.execute({ type: "pause" }, { hostEpoch: "epoch", outputRevision: 3 })).toMatchObject({ status: "applied" });
    expect(n.command).toHaveBeenCalledOnce();
    expect(n.commandStatus).toHaveBeenCalledExactlyOnceWith(expect.any(Object), n.command.mock.calls[0][1].requestId);
    s.suspendController();
});
it("old_session_results_cannot_replace_new_host", async () => {
    const n = bridge(), q = deferred<QueryResult>(), m = deferred<any>();
    n.query.mockReturnValue(q.promise);
    n.media.mockReturnValue(m.promise);
    const s = createControllerSession(n);
    await s.connectController("one");
    const pending = s.port.query({ type: "tracks" }), art = s.port.resolveArtwork({ resourceId: "art", revision: 1 });
    const qCheck = expect(pending).rejects.toMatchObject({ code: "resync_required" }), aCheck = expect(art).rejects.toMatchObject({ code: "resync_required" });
    await vi.waitFor(() => expect(n.media).toHaveBeenCalledOnce());
    await s.connectController("two");
    const revoke = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {
    });
    vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:old");
    q.resolve({ type: "tracks", page: { items: [], nextCursor: null, revision: 1 } });
    m.resolve({ mime: "image/png", bytes: [137, 80, 78, 71] });
    await Promise.all([qCheck, aCheck]);
    expect(get(s.state).currentHostId).toBe("two");
    expect(revoke).toHaveBeenCalledExactlyOnceWith("blob:old");
    s.suspendController();
});
it("rejects old epoch and noncontiguous batches without enabling stale controls", async () => {
    const n = bridge(), poll = deferred<EventBatch>();
    n.poll.mockReturnValueOnce(poll.promise);
    n.connect.mockImplementation(async (id) => ({ snapshot: snapshot(id, n.connect.mock.calls.length === 1 ? "epoch" : "restart"), grants: { control: true } }));
    const s = createControllerSession(n);
    await s.connectController("one");
    poll.resolve({ hostEpoch: "old", revision: 1, events: [] });
    await vi.waitFor(() => expect(n.connect).toHaveBeenCalledTimes(2));
    expect(get(s.state).snapshot?.hostEpoch).toBe("restart");
    s.suspendController();
});
it("does not rewrite stale caller preconditions into trusted current revisions", async () => {
    const n = bridge(), s = createControllerSession(n);
    await s.connectController("one");
    await expect(s.port.execute({ type: "pause" }, { hostEpoch: "epoch", outputRevision: 2 })).rejects.toMatchObject({ code: "revision_conflict" });
    expect(n.command).not.toHaveBeenCalled();
    s.suspendController();
});
it("suspend cancels reconnect and resume requires another confirmed snapshot", async () => {
    vi.useFakeTimers();
    const n = bridge();
    n.connect.mockRejectedValueOnce({ code: "host_not_ready", retryable: true });
    const s = createControllerSession(n, { random: () => 0.5 });
    await s.connectController("one");
    expect(get(s.state).ready).toBe(false);
    s.suspendController();
    await vi.advanceTimersByTimeAsync(9000);
    expect(n.connect).toHaveBeenCalledOnce();
    await s.connectController("one");
    expect(get(s.state).ready).toBe(true);
    s.suspendController();
});
it("revocation does not reconnect or retain confirmed controls", async () => {
    const n = bridge(), poll = deferred<EventBatch>();
    n.poll.mockReturnValueOnce(poll.promise);
    const s = createControllerSession(n);
    await s.connectController("one");
    poll.reject({ code: "unauthorized", retryable: false });
    await vi.waitFor(() => expect(get(s.state).status).toBe("pairing_required"));
    expect(get(s.state).ready).toBe(false);
    expect(n.connect).toHaveBeenCalledOnce();
    s.suspendController();
});
it("expired command outcomes stay unknown and never replay", async () => {
    const n = bridge();
    n.command.mockRejectedValue({ code: "outcome_unknown" });
    n.commandStatus.mockRejectedValue({ code: "outcome_unknown", message: "Expired result", retryable: false });
    const s = createControllerSession(n);
    await s.connectController("one");
    await expect(s.port.execute({ type: "pause" }, { hostEpoch: "epoch", outputRevision: 3 })).rejects.toMatchObject({ code: "outcome_unknown" });
    expect(n.command).toHaveBeenCalledOnce();
    s.suspendController();
});
it("fresh renderer bootstrap gets a new native scope before any connect", async () => {
    const n = bridge();
    n.beginScope.mockResolvedValueOnce("old").mockResolvedValueOnce("new");
    const old = createControllerSession(n);
    await old.initialize();
    await old.connectController("one");
    const fresh = createControllerSession(n);
    await fresh.initialize();
    await fresh.connectController("two");
    old.suspendController();
    await Promise.resolve();
    expect(n.connect.mock.calls[1][1]).toEqual({ scopeId: "new", generation: 1 });
    expect(get(fresh.state).currentHostId).toBe("two");
    fresh.suspendController();
});
it("ordered batches update only confirmed playback and retain one poll", async () => {
    const n = bridge(), d = deferred<EventBatch>();
    n.poll.mockReturnValueOnce(d.promise);
    const s = createControllerSession(n);
    await s.connectController("one");
    const seen = vi.fn();
    s.port.subscribe(seen);
    d.resolve({ hostEpoch: "epoch", revision: 1, events: [{ type: "playback", revision: 1, playback: { ...snapshot().playback, position: 42 } }] });
    await vi.waitFor(() => expect(get(s.state).snapshot?.playback.position).toBe(42));
    expect(n.poll).toHaveBeenCalledTimes(2);
    expect(n.poll.mock.calls[1][1]).toEqual({ hostEpoch: "epoch", revision: 1 });
    s.suspendController();
});
it("forget promptly disposes all acquired image handles", async () => {
    const n = bridge();
    n.media.mockResolvedValue({ mime: "image/png", bytes: [137, 80, 78, 71] });
    vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:owned");
    const revoke = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {
    });
    const s = createControllerSession(n);
    await s.connectController("one");
    const handle = await s.port.resolveArtwork({ resourceId: "art", revision: 1 });
    await s.forgetController("one");
    handle.dispose();
    expect(revoke).toHaveBeenCalledExactlyOnceWith("blob:owned");
    expect(get(s.state)).toMatchObject({ ready: false, currentHostId: null });
});
it("library events dispose late media from the previous revision", async () => {
    const n = bridge(), poll = deferred<EventBatch>(), image = deferred<{
        mime: string;
        bytes: number[];
    }>();
    n.poll.mockReturnValueOnce(poll.promise);
    n.media.mockReturnValueOnce(image.promise);
    vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:old-revision");
    const revoke = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {
    });
    const s = createControllerSession(n);
    await s.connectController("one");
    const pending = s.port.resolveArtwork({ resourceId: "art", revision: 1 });
    const rejected = expect(pending).rejects.toMatchObject({ code: "resync_required" });
    await vi.waitFor(() => expect(n.media).toHaveBeenCalledOnce());
    poll.resolve({ hostEpoch: "epoch", revision: 1, events: [{ type: "library", revision: 1, libraryRevision: 2 }] });
    await vi.waitFor(() => expect(get(s.state).snapshot?.revisions.libraryRevision).toBe(2));
    image.resolve({ mime: "image/png", bytes: [137, 80, 78, 71] });
    await rejected;
    expect(revoke).toHaveBeenCalledExactlyOnceWith("blob:old-revision");
    s.suspendController();
});

it.each(["switch", "suspend", "forget"] as const)("held post-pair connect cannot own selection after %s", async (intent) => {
    const n = bridge(), held = deferred<AuthenticatedConnection>();
    n.scan.mockResolvedValue({ status: "invitation_ready", fingerprint: "ab".repeat(32) });
    n.pair.mockResolvedValue({ hostId: "one" });
    n.connect.mockImplementation(async host => host === "one" ? held.promise : { snapshot: snapshot(host), grants: { control: true } });
    const s = createControllerSession(n);
    const pending = s.pairController();
    await vi.waitFor(() => expect(n.connect).toHaveBeenCalledOnce());
    if (intent === "switch") await s.connectController("two");
    else if (intent === "suspend") s.suspendController();
    else await s.forgetController("one");
    const before = get(s.state);
    held.resolve({ snapshot: snapshot("one"), grants: { control: true } });
    expect(await pending).toBeUndefined();
    expect(get(s.state)).toEqual(before);
    s.suspendController();
});

it("pairing receipt retains connection ownership until the consumer commits selection", async () => {
    const n = bridge();
    n.scan.mockResolvedValue({ status: "invitation_ready", fingerprint: "ab".repeat(32) });
    n.pair.mockResolvedValue({ hostId: "one" });
    const catalog = vi.fn(() => expect(n.connect).not.toHaveBeenCalled());
    const s = createControllerSession(n);
    const receipt = await s.pairController(catalog);
    expect(catalog).toHaveBeenCalledExactlyOnceWith("one");
    expect(receipt?.hostId).toBe("one");
    expect(receipt?.isCurrent()).toBe(true);
    s.suspendController();
    expect(receipt?.isCurrent()).toBe(false);
});

it("requires explicit authenticated grants and clears them on reconnect", async () => {
    const n = bridge();
    n.connect.mockResolvedValueOnce({ snapshot: snapshot(), grants: { control: false } } as any);
    const s = createControllerSession(n);
    await s.connectController("one");
    expect(get(s.state)).toMatchObject({ ready: true, grants: { control: false } });
    await expect(s.port.execute({ type: "pause" }, { hostEpoch: "epoch", outputRevision: 3 })).rejects.toMatchObject({ code: "permission_required" });
    expect(n.command).not.toHaveBeenCalled();
    n.connect.mockReturnValueOnce(new Promise(() => {}));
    void s.connectController("one");
    expect(get(s.state)).toMatchObject({ ready: false, grants: null });
    s.suspendController();
});
it("rejects missing grant receipts instead of treating readiness as permission", async () => {
    const n = bridge();
    n.connect.mockResolvedValueOnce({ snapshot: snapshot() } as any);
    const s = createControllerSession(n);
    await s.connectController("one");
    expect(get(s.state)).toMatchObject({ ready: false, grants: null, status: "protocol_error" });
    s.suspendController();
});

it.each(["unauthorized", "permission_required"] as const)("clears permission on rejected or failed status %s with poll held", async code => {
 for (const rejection of [true, false]) {
  const n=bridge(), s=createControllerSession(n);
  n.command.mockRejectedValue({code:"outcome_unknown"});
  const error={code,message:"Permission changed",retryable:false};
  if(rejection)n.commandStatus.mockRejectedValue(error);
  else n.commandStatus.mockResolvedValue({status:"failed",error,revision:1,partialEffects:[]});
  await s.connectController("one");
  await s.port.execute({type:"pause"},{hostEpoch:"epoch",outputRevision:3}).catch(()=>{});
  expect(get(s.state)).toMatchObject({ready:false,grants:null,snapshot:null});
  expect(n.command).toHaveBeenCalledOnce();s.suspendController();
 }
});
it.each([true,false])("late status receipt rejection=%s cannot invalidate replacement connection", async rejection=>{
 const n=bridge(),s=createControllerSession(n),status=deferred<any>();
 n.command.mockRejectedValue({code:"outcome_unknown"});n.commandStatus.mockReturnValue(status.promise);
 await s.connectController("one");
 const pending=s.port.execute({type:"pause"},{hostEpoch:"epoch",outputRevision:3});
 const checked=expect(pending).rejects.toMatchObject({code:"resync_required"});
 await vi.waitFor(()=>expect(n.commandStatus).toHaveBeenCalledOnce());
 await s.connectController("two");
 if(rejection)status.reject({code:"unauthorized"});
 else status.resolve({status:"failed",revision:1,partialEffects:[],error:{code:"permission_required",message:"Permission changed",retryable:false}});
 await checked;
 expect(get(s.state)).toMatchObject({ready:true,currentHostId:"two",grants:{control:true}});s.suspendController();
});
it("endpoint repair replaces ownership and only adopts the confirmed candidate", async () => {
    const n = bridge(), candidate = deferred<AuthenticatedConnection>();
    const updateEndpoint = vi.fn(() => candidate.promise);
    const s = createControllerSession({ ...n, updateEndpoint } as any);
    await s.connectController("one");
    const updating = s.updateEndpoint("one", "192.168.1.9:1234");
    expect(get(s.state).ready).toBe(false);
    await vi.waitFor(() => expect(updateEndpoint).toHaveBeenCalledOnce());
    expect(updateEndpoint).toHaveBeenCalledWith("one", "192.168.1.9:1234", { scopeId: "scope", generation: 2 });
    s.suspendController();
    candidate.resolve({ snapshot: snapshot("one"), grants: { control: true } });
    await updating;
    expect(get(s.state)).toMatchObject({ ready: false, status: "disconnected" });
});
it("shows native-validated fingerprint while waiting for explicit PC approval", async () => {
    const n = bridge(), approval = deferred<{ hostId: string }>();
    n.scan.mockResolvedValue({ status: "invitation_ready", hostId: "one", fingerprint: "ab".repeat(32) });
    n.pair.mockReturnValue(approval.promise);
    const s = createControllerSession(n), pending = s.pairController();
    await vi.waitFor(() => expect(n.pair).toHaveBeenCalledOnce());
    expect(get(s.state).pairingFingerprint).toBe("ab".repeat(32));
    s.suspendController();
    expect(get(s.state).pairingFingerprint).toBeUndefined();
    approval.resolve({ hostId: "one" }); await pending;
});
