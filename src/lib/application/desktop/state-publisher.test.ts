import { afterEach, expect, it, vi } from "vitest";
import { startStatePublisher } from "./state-publisher";
import { createPlaybackCoordinator, type DesktopPlaybackRuntime, type HostState } from "./playback-coordinator";
import type { HostSnapshot } from "../types";
function fixture() {
  let snapshot: HostSnapshot = { hostId: "host", hostEpoch: "epoch", revision: 0, revisions: { queueRevision: 0, libraryRevision: 0, outputRevision: 0, settingsRevision: 0 }, playback: { status: "stopped", track: null, context: null, position: 0, duration: null, volume: 0.5, shuffle: false, repeat: "none" }, queue: { count: 0, currentEntryId: null }, output: { kind: "pc" }, outputs: [], capabilities: { queries: ["snapshot"], intents: [] }, settings: {}, jobs: [] };
  const listeners = new Set<(snapshot: HostSnapshot) => void>();
  const state: HostState = { hostEpoch: "epoch", revision: 0, revisions: snapshot.revisions, selectedOutput: { kind: "pc" }, ownershipGeneration: 0, transitionGeneration: 0, queue: [] };
  const runtime: DesktopPlaybackRuntime = { validateOutput: async () => ({ status: "applied" }), resolvePlayback: async () => ({ tracks: [], startIndex: 0, context: null }), stopOwnedOutput: async () => ({ status: "applied" }), selectOutput: async () => ({ status: "applied" }), apply: async () => ({ status: "applied" }), applySignal: async () => ({ status: "applied" }) };
  const coordinator = createPlaybackCoordinator(runtime, { read: () => state, commit: () => {} }, { read: () => snapshot, subscribe: listener => { listeners.add(listener); return () => { listeners.delete(listener); }; } });
  return { coordinator, listeners, change(change: Partial<HostSnapshot>) { snapshot = { ...snapshot, ...change }; listeners.forEach(listener => listener(snapshot)); }, get snapshot() { return snapshot; } };
}
afterEach(() => vi.useRealTimers());
it("publishes once after repeated startup and coalesces progress without domain revision changes", async () => {
  vi.useFakeTimers(); const f = fixture(); const seen: HostSnapshot[] = [];
  const stop = startStatePublisher(f.coordinator, async s => { seen.push(s); return seen.length; });
  const duplicate = startStatePublisher(f.coordinator, async () => { throw new Error("duplicate publisher"); });
  await stop.ready;
  expect(seen).toHaveLength(1); expect(f.listeners.size).toBe(1);
  f.change({ playback: { ...f.snapshot.playback, position: 1 } });
  f.change({ playback: { ...f.snapshot.playback, position: 2 } });
  await vi.advanceTimersByTimeAsync(249); expect(seen).toHaveLength(1);
  await vi.advanceTimersByTimeAsync(1); expect(seen).toHaveLength(2);
  expect(seen[1].playback.position).toBe(2); expect(seen[1].revisions.queueRevision).toBe(0);
  f.change({ playback: { ...f.snapshot.playback, status: "playing" } });
  await Promise.resolve(); expect(seen.at(-1)?.playback.status).toBe("playing");
  stop(); duplicate(); expect(f.listeners.size).toBe(0);
  f.change({ playback: { ...f.snapshot.playback, position: 3 } }); await vi.runAllTimersAsync(); expect(seen).toHaveLength(3);
});
it("bounds pending delivery, keeps latest projection, and stops after publication rejection", async () => {
  const f = fixture(); const seen: HostSnapshot[] = []; let finish!: (revision: number) => void;
  const stop = startStatePublisher(f.coordinator, s => { seen.push(s); return new Promise(resolve => { finish = resolve; }); });
  for (let count = 1; count <= 100; count++) f.change({ queue: { count, currentEntryId: null }, revisions: { ...f.snapshot.revisions, queueRevision: count } });
  expect(seen).toHaveLength(1); finish(0); await stop.ready; await Promise.resolve();
  expect(seen).toHaveLength(2); expect(seen[1].queue.count).toBe(100);
  stop(); finish(1); await Promise.resolve(); expect(f.listeners.size).toBe(0);
  const broken = startStatePublisher(f.coordinator, async () => { throw new Error("revoked"); });
  await expect(broken.ready).rejects.toThrow("revoked"); expect(f.listeners.size).toBe(0);
});

it("flush is a bounded command-effect barrier and disposal rejects blocked publication", async () => {
  vi.useFakeTimers(); const f = fixture(); let deliver!: (revision: number) => void;
  const stop = startStatePublisher(f.coordinator, async () => 0); await stop.ready; stop();
  const active = startStatePublisher(f.coordinator, () => new Promise(resolve => { deliver = resolve; }));
  let flushed = false; const flush = active.flush().then(revision => { flushed = true; return revision; });
  await Promise.resolve(); expect(flushed).toBe(false);
  deliver(12); await active.ready; expect(await flush).toBe(12);
  f.change({ playback: { ...f.snapshot.playback, position: 44 } });
  const progressFlush = active.flush(); await Promise.resolve(); deliver(13); expect(await progressFlush).toBe(13);
  f.change({ queue: { count: 1, currentEntryId: null } });
  const abandoned = active.flush(); const rejection = expect(abandoned).rejects.toThrow("stopped"); active(); await rejection;
  const hung = startStatePublisher(f.coordinator, () => new Promise(() => {}));
  const timedOut = expect(hung.ready).rejects.toThrow("timed out");
  await vi.advanceTimersByTimeAsync(5000); await timedOut;
  expect(f.listeners.size).toBe(0);
});

it.each(["initial timeout", "steady timeout", "dispose"])("ignores late publication settlement after %s with immediate idempotent local cleanup", async reason => {
  vi.useFakeTimers(); const f = fixture(); const failed = vi.fn();
  let deliver!: (revision: number) => void; let publications = 0;
  const stop = startStatePublisher(f.coordinator, () => {
    if (reason !== "initial timeout" && publications++ === 0) return Promise.resolve(1);
    return new Promise(resolve => { deliver = resolve; });
  }, failed);
  if (reason !== "initial timeout") { await stop.ready; f.change({ queue: { count: 1, currentEntryId: null } }); }
  const barrier = stop.flush(); const rejected = expect(barrier).rejects.toThrow(reason === "dispose" ? "stopped" : "timed out");
  if (reason === "dispose") { stop(); stop(); expect(f.listeners.size).toBe(0); }
  else await vi.advanceTimersByTimeAsync(5000);
  await rejected; expect(f.listeners.size).toBe(0); expect(failed).toHaveBeenCalledTimes(reason === "dispose" ? 0 : 1);
  deliver(999); await vi.advanceTimersByTimeAsync(0);
  await expect(stop.flush()).rejects.toThrow(reason === "dispose" ? "stopped" : "timed out");
  expect(vi.getTimerCount()).toBe(0); expect(f.listeners.size).toBe(0);
});
