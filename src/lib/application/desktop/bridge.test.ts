import { afterEach, describe, expect, it, vi } from "vitest";
import { connectHostBridge, type HostDispatch, type HostTransport } from "./bridge";
import type { ApplicationPort, ExecutionResult } from "../types";
import type { PlaybackCoordinator } from "./playback-coordinator";
const coordinator: PlaybackCoordinator = {
  captureSnapshot: () => ({ hostId: "host", hostEpoch: "epoch", revision: 0, revisions: { queueRevision: 0, outputRevision: 0, libraryRevision: 0, settingsRevision: 0 }, playback: { status: "stopped", track: null, context: null, position: 0, duration: null, volume: 0.5, shuffle: false, repeat: "none" }, queue: { count: 0, currentEntryId: null }, output: { kind: "pc" }, outputs: [], capabilities: { queries: ["snapshot"], intents: [] }, settings: {}, jobs: [] }),
  subscribeSnapshot: () => () => {}, execute: async () => ({ status: "applied", revision: 0 }), enqueueSignal: async () => {}, executeLocal: async () => ({ status: "applied", revision: 0 }), dispose: async () => {},
};
const portWith = (execute: ApplicationPort["execute"]): ApplicationPort => ({
  execute,
  async query() { throw new Error("Unexpected query"); },
  subscribe() { return () => {}; },
  async resolveArtwork() { throw new Error("Unexpected artwork resolution"); },
});

describe("native host bridge", () => {
  it("listens before preparation and attaches authority before ready, then completes exact tickets through the port", async () => {
    const steps: string[] = [];
    let receive!: (dispatch: HostDispatch) => void;
    const completed: unknown[] = [];
    const lease = { leaseId: "lease", hostEpoch: "epoch" };
    const transport: HostTransport = {
      async listen(listener) { steps.push("listen"); receive = listener; return () => { steps.push("unlisten"); }; },
      async register(request) { steps.push(request.phase); return { hostId: "host", lease, ...(request.phase === "publish" ? { revision: 0 } : {}) }; },
      async authorize() { return envelope; },
      async complete(actualLease, ticket, result) { completed.push({ lease: actualLease, ticket, result }); },
    };
    const envelope = { protocolVersion: 1 as const, requestId: "request", preconditions: { hostEpoch: "epoch", outputRevision: 7 }, intent: { type: "pause" as const } };
    const port = portWith(async (intent, preconditions) => {
      expect(intent).toEqual(envelope.intent); expect(preconditions).toEqual(envelope.preconditions);
      steps.push("execute"); return { status: "applied", revision: 8 } as ExecutionResult;
    });
    const bridge = await connectHostBridge(port, async authority => {
      expect(authority).toEqual({ hostId: "host", hostEpoch: "epoch" }); steps.push("attach");
    }, coordinator, transport);
    expect(steps).toEqual(["listen", "prepare", "attach", "publish", "ready"]);
    receive({ ticket: "native-ticket", envelope });
    await new Promise(resolve => setTimeout(resolve, 0));
    expect(completed).toEqual([{ lease, ticket: "native-ticket", result: { status: "applied", revision: 8 } }]);
    await bridge.dispose();
    expect(steps.slice(-2)).toEqual(["unlisten", "release"]);
    receive({ ticket: "late", envelope });
    expect(steps.filter(step => step === "execute")).toHaveLength(1);
  });
  it("releases a prepared lease when attachment fails without advertising ready", async () => {
    const steps: string[] = [];
    const transport: HostTransport = {
      async listen() { return () => { steps.push("unlisten"); }; },
      async register(request) { steps.push(request.phase); return { hostId: "host", lease: { leaseId: "lease", hostEpoch: "epoch" } }; },
      async authorize() { throw new Error("unexpected authorization"); },
      async complete() { throw new Error("unexpected completion"); },
    };
    await expect(connectHostBridge(portWith(async () => { throw new Error("unexpected execution"); }), async () => { throw new Error("attachment failed"); }, coordinator, transport)).rejects.toThrow("attachment failed");
    expect(steps).toEqual(["prepare", "unlisten", "release"]);
  });
  it("rejects forged and duplicate events and executes only the native claimed envelope", async () => {
    let receive!: (dispatch: HostDispatch) => void;
    const intents: unknown[] = [];
    const original = { protocolVersion: 1 as const, requestId: "original", preconditions: { hostEpoch: "epoch" }, intent: { type: "pause" as const } };
    const tickets = new Set(["native"]);
    const bridge = await connectHostBridge(portWith(async intent => { intents.push(intent); return { status: "applied", revision: 1 }; }), async () => {}, coordinator, {
      async listen(listener) { receive = listener; return () => {}; },
      async register(request) { return { hostId: "host", lease: { leaseId: "lease", hostEpoch: "epoch" }, ...(request.phase === "publish" ? { revision: 0 } : {}) }; },
      async authorize(_lease, ticket) { if (!tickets.delete(ticket)) throw new Error("Unauthorized ticket"); return original; },
      async complete() {},
    });
    receive({ ticket: "forged", envelope: { ...original, intent: { type: "resume" } } });
    await new Promise(resolve => setTimeout(resolve, 0));
    expect(intents).toEqual([]);
    receive({ ticket: "native", envelope: { ...original, intent: { type: "resume" } } });
    receive({ ticket: "native", envelope: original });
    await new Promise(resolve => setTimeout(resolve, 0));
    expect(intents).toEqual([{ type: "pause" }]);
    await bridge.dispose();
  });
});

it("holds native completion behind command-effect publication and releases on failed flush", async () => {
  let receive!: (dispatch: HostDispatch) => void;
  let observe!: (snapshot: ReturnType<PlaybackCoordinator["captureSnapshot"]>) => void;
  let deliver!: (value: { hostId: string; lease: { leaseId: string; hostEpoch: string }; revision: number }) => void;
  let rejectDelivery!: (error: Error) => void;
  const lease = { leaseId: "lease", hostEpoch: "epoch" };
  const completed: ExecutionResult[] = []; const phases: string[] = [];
  let snapshot = coordinator.captureSnapshot();
  const source: PlaybackCoordinator = { ...coordinator, captureSnapshot: () => snapshot, subscribeSnapshot: listener => { observe = listener; return () => {}; } };
  let publications = 0;
  const transport: HostTransport = {
    async listen(listener) { receive = listener; return () => {}; },
    register(request) { phases.push(request.phase); if (request.phase === "publish" && publications++ > 0) return new Promise((resolve, reject) => { deliver = resolve; rejectDelivery = reject; }); return Promise.resolve({ hostId: "host", lease, revision: 0 }); },
    async authorize() { return { protocolVersion: 1, requestId: "one", preconditions: { hostEpoch: "epoch" }, intent: { type: "pause" } }; },
    async complete(_lease, _ticket, result) { completed.push(result); },
  };
  const bridge = await connectHostBridge(portWith(async () => {
    snapshot = { ...snapshot, playback: { ...snapshot.playback, volume: snapshot.playback.volume + 0.1 } }; observe(snapshot);
    return { status: "applied", revision: 999 };
  }), async () => {}, source, transport);
  const event = { ticket: "native", envelope: await transport.authorize(lease, "native") };
  receive(event); await new Promise(resolve => setTimeout(resolve, 0));
  expect(completed).toHaveLength(0);
  deliver({ hostId: "host", lease, revision: 4 }); await new Promise(resolve => setTimeout(resolve, 0));
  expect(completed).toHaveLength(1);
  receive(event); await new Promise(resolve => setTimeout(resolve, 0)); rejectDelivery(new Error("revoked"));
  await new Promise(resolve => setTimeout(resolve, 0)); expect(completed).toHaveLength(1); expect(phases.at(-1)).toBe("release");
  await bridge.dispose();
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
async function settle() { for (let i = 0; i < 20; i++) await Promise.resolve(); }
function stalledBridgeFixture(initialPublication = false, waitForReady = false) {
  const ack = { hostId: "host", lease: { leaseId: "lease", hostEpoch: "epoch" }, revision: 1 };
  const publication = deferred<typeof ack>(); const release = deferred<typeof ack>(); const nativeReady = deferred<typeof ack>();
  const phases: string[] = []; const unlisten = vi.fn(); const unsubscribe = vi.fn();
  const complete = vi.fn(async () => {});
  let receive!: (dispatch: HostDispatch) => void;
  let observe!: (snapshot: ReturnType<PlaybackCoordinator["captureSnapshot"]>) => void;
  let snapshot = coordinator.captureSnapshot(); let publications = 0;
  const source: PlaybackCoordinator = { ...coordinator, captureSnapshot: () => snapshot, subscribeSnapshot: listener => { observe = listener; return unsubscribe; } };
  const envelope = { protocolVersion: 1 as const, requestId: "one", preconditions: { hostEpoch: "epoch" }, intent: { type: "pause" as const } };
  const execute = vi.fn(async (): Promise<ExecutionResult> => {
    snapshot = { ...snapshot, queue: { count: 1, currentEntryId: null } }; observe(snapshot);
    return { status: "applied", revision: 1 };
  });
  const transport: HostTransport = {
    async listen(listener) { receive = listener; return unlisten; },
    register(request) {
      phases.push(request.phase);
      if (request.phase === "release") return release.promise;
      if (request.phase === "ready" && waitForReady) return nativeReady.promise;
      if (request.phase === "publish" && (initialPublication || publications++ > 0)) return publication.promise;
      return Promise.resolve(ack);
    },
    async authorize() { return envelope; }, complete,
  };
  return { connect: () => connectHostBridge(portWith(execute), async () => {}, source, transport),
    send: () => receive({ ticket: "ticket", envelope }), ack, publication, release, nativeReady, phases, unlisten, unsubscribe, execute, complete };
}
afterEach(() => vi.useRealTimers());
it.each(["rejection", "timeout"])("detaches initial publication %s immediately and reports unknown when release stalls", async failure => {
  vi.useFakeTimers(); const f = stalledBridgeFixture(true);
  let outcome: unknown;
  const connecting = f.connect().catch(error => { outcome = error; });
  await settle();
  if (failure === "rejection") { f.publication.reject(new Error("publication failed")); await settle(); }
  else await vi.advanceTimersByTimeAsync(5000);
  expect(f.unsubscribe).toHaveBeenCalledTimes(1); expect(f.unlisten).toHaveBeenCalledTimes(1);
  expect(f.phases).not.toContain("ready"); expect(outcome).toBeUndefined();
  await vi.advanceTimersByTimeAsync(999); expect(outcome).toBeUndefined();
  await vi.advanceTimersByTimeAsync(1);
  expect(outcome).toBeInstanceOf(Error); expect((outcome as Error).message).toMatch(/unavailable.*unknown/i);
  await connecting;
  f.release.resolve(f.ack); f.publication.resolve(f.ack); await settle(); f.send(); await settle();
  expect(f.execute).not.toHaveBeenCalled(); expect(f.complete).not.toHaveBeenCalled();
  expect(f.phases).not.toContain("ready"); expect(f.unlisten).toHaveBeenCalledTimes(1);
  expect(vi.getTimerCount()).toBe(0);
});
it.each([
  ["timeout", "resolve"], ["timeout", "reject"], ["dispose", "resolve"], ["dispose", "reject"],
])("detaches on steady publication %s and bounds one shared release despite late %s without acknowledging entered effects", async (reason, late) => {
  vi.useFakeTimers(); const f = stalledBridgeFixture(); const bridge = await f.connect();
  f.send(); await settle(); expect(f.execute).toHaveBeenCalledTimes(1); expect(f.complete).not.toHaveBeenCalled();
  if (reason === "timeout") await vi.advanceTimersByTimeAsync(5000);
  const first = bridge.dispose(); const second = bridge.dispose();
  let firstError: unknown; let secondError: unknown;
  const firstDone = first.catch(error => { firstError = error; }); const secondDone = second.catch(error => { secondError = error; });
  expect(f.unlisten).toHaveBeenCalledTimes(1); expect(f.unsubscribe).toHaveBeenCalledTimes(1);
  await settle(); expect(f.phases.filter(phase => phase === "release")).toHaveLength(1);
  await vi.advanceTimersByTimeAsync(1000);
  expect(firstError).toBeInstanceOf(Error); expect((firstError as Error).message).toMatch(/unavailable.*unknown/i);
  expect(secondError).toBe(firstError); await Promise.all([firstDone, secondDone]);
  // Neither a late acknowledgement nor a release receipt revives the stopped lease locally.
  if (late === "resolve") { f.publication.resolve(f.ack); f.release.resolve(f.ack); }
  else { f.publication.reject(new Error("late publication failure")); f.release.reject(new Error("late release failure")); }
  await settle(); f.send(); await settle();
  expect(f.complete).not.toHaveBeenCalled(); expect(f.execute).toHaveBeenCalledTimes(1);
  await expect(bridge.dispose()).rejects.toBe(firstError);
  expect(f.unlisten).toHaveBeenCalledTimes(1); expect(f.phases.filter(phase => phase === "release")).toHaveLength(1);
  expect(vi.getTimerCount()).toBe(0);
});

it("rejects a late readiness receipt after publication failure already closed the bridge", async () => {
  vi.useFakeTimers(); const f = stalledBridgeFixture(false, true);
  let outcome: unknown; let connected = false;
  const connecting = f.connect().then(() => { connected = true; }, error => { outcome = error; });
  await settle(); expect(f.phases).toContain("ready");
  f.send(); await settle(); await vi.advanceTimersByTimeAsync(5000);
  expect(f.unlisten).toHaveBeenCalledTimes(1); f.release.resolve(f.ack); await settle();
  f.nativeReady.resolve(f.ack); await settle(); await connecting;
  expect(connected).toBe(false); expect(outcome).toBeInstanceOf(Error);
  expect((outcome as Error).message).toMatch(/unavailable/);
  f.publication.resolve(f.ack); await settle(); expect(f.complete).not.toHaveBeenCalled();
  expect(vi.getTimerCount()).toBe(0);
});
