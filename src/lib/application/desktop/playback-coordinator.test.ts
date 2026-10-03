import { describe, expect, it, vi } from "vitest";
import { createPlaybackCoordinator, type HostStateAccess, type HostState, type DesktopPlaybackRuntime, type RuntimeResult } from "./playback-coordinator";

const ok: RuntimeResult = { status: "applied" };
const fail: RuntimeResult = { status: "failed", error: { code: "execution_failed", message: "offline", retryable: true }, partialEffects: [] };
function fixture() {
  let value: HostState = { hostEpoch: "host", revision: 0, revisions: { queueRevision: 0, outputRevision: 0, libraryRevision: 0, settingsRevision: 0 }, selectedOutput: { kind: "pc" }, ownershipGeneration: 1, transitionGeneration: 1, queue: [] };
  const calls: string[] = [];
  const state: HostStateAccess = {
    read: () => value,
    commit: vi.fn(delta => { value = { ...value, ...delta, revision: value.revision + 1, revisions: { ...value.revisions, ...delta.revisions } }; }),
  };
  const runtime: DesktopPlaybackRuntime = {
    validateOutput: vi.fn(async () => ok),
    resolvePlayback: vi.fn(async () => ({ tracks: [], startIndex: 0, context: null })),
    stopOwnedOutput: vi.fn(async output => { calls.push(`stop:${output.kind === "pc" ? "pc" : `squeeze:${output.playerId}`}`); return ok; }),
    selectOutput: vi.fn(async output => { calls.push(`select:${output.kind === "pc" ? "pc" : `squeeze:${output.playerId}`}`); return ok; }),
    apply: vi.fn(async () => ok),
    applySignal: vi.fn(async () => ok),
  };
  return { runtime, state, calls, coordinator: createPlaybackCoordinator(runtime, state) };
}
describe("confirmed playback lane", () => {
  it("awaits old-output stop before selecting and publishing the new target", async () => {
    const f = fixture();
    let stop!: (result: RuntimeResult) => void;
    f.runtime.stopOwnedOutput = vi.fn(() => new Promise<RuntimeResult>(resolve => { stop = resolve; }));
    const pending = f.coordinator.execute({ type: "select_output", output: { kind: "squeeze", playerId: "A" } }, { hostEpoch: "host", outputRevision: 0 });
    await vi.waitFor(() => expect(f.runtime.stopOwnedOutput).toHaveBeenCalledOnce());
    expect(f.runtime.selectOutput).not.toHaveBeenCalled();
    expect(f.state.read().selectedOutput).toEqual({ kind: "pc" });
    stop(ok);
    expect(await pending).toEqual({ status: "applied", revision: 2 });
    expect(f.state.read().selectedOutput).toEqual({ kind: "squeeze", playerId: "A" });
  });
  it("orders stop and select, including Squeeze A to B", async () => {
    const f = fixture();
    await f.coordinator.execute({ type: "select_output", output: { kind: "squeeze", playerId: "A" } }, { hostEpoch: "host", outputRevision: 0 });
    expect(f.calls).toEqual(["stop:pc", "select:squeeze:A"]);
    await f.coordinator.execute({ type: "select_output", output: { kind: "squeeze", playerId: "B" } }, { hostEpoch: "host", outputRevision: 1 });
    expect(f.calls).toEqual(["stop:pc", "select:squeeze:A", "stop:squeeze:A", "select:squeeze:B"]);
  });
  it.each(["failed", "rejected", "unknown"])("never selects after %s stop", async mode => {
    const f = fixture();
    f.runtime.stopOwnedOutput = vi.fn(async (): Promise<RuntimeResult> => {
      if (mode === "rejected") throw new Error("IPC lost");
      return mode === "unknown" ? { ...fail, error: { code: "outcome_unknown", message: "unknown", retryable: false } } : fail;
    });
    const result = await f.coordinator.execute({ type: "select_output", output: { kind: "squeeze", playerId: "A" } }, { hostEpoch: "host", outputRevision: 0 });
    expect(result.status).toBe("failed");
    expect(f.runtime.selectOutput).not.toHaveBeenCalled();
    expect(f.state.read().selectedOutput).toEqual({ kind: "pc" });
  });
  it("rejects stale or unavailable targets before stopping anything", async () => {
    const f = fixture();
    expect(await f.coordinator.execute({ type: "select_output", output: { kind: "squeeze", playerId: "A" } }, { hostEpoch: "host", outputRevision: 2 })).toMatchObject({ status: "superseded", error: { code: "revision_conflict" } });
    f.runtime.validateOutput = vi.fn(async () => fail);
    await f.coordinator.execute({ type: "select_output", output: { kind: "squeeze", playerId: "A" } }, { hostEpoch: "host", outputRevision: 0 });
    expect(f.runtime.stopOwnedOutput).not.toHaveBeenCalled();
  });
  it("does not stop or reselect the same output", async () => {
    const f = fixture();
    expect(await f.coordinator.execute({ type: "select_output", output: { kind: "pc" } }, { hostEpoch: "host", outputRevision: 0 })).toEqual({ status: "applied", revision: 0 });
    expect(f.runtime.stopOwnedOutput).not.toHaveBeenCalled();
    expect(f.runtime.selectOutput).not.toHaveBeenCalled();
  });
  it("discloses a confirmed stop even when selection fails", async () => {
    const f = fixture();
    f.runtime.selectOutput = vi.fn(async () => fail);
    expect(await f.coordinator.execute({ type: "select_output", output: { kind: "squeeze", playerId: "A" } }, { hostEpoch: "host", outputRevision: 0 })).toMatchObject({ status: "failed", revision: 1, partialEffects: ["Previous output stopped"] });
    expect(f.state.read().selectedOutput).toEqual({ kind: "pc" });
  });
});

describe("signal and command ownership", () => {
  it("deduplicates completion/gapless but preserves a time expiry across natural advance", async () => {
    const f = fixture();
    const signal = { output: { kind: "pc" as const }, ownershipGeneration: 1, transitionGeneration: 1 };
    await Promise.all([
      f.coordinator.enqueueSignal({ ...signal, kind: "gapless" }),
      f.coordinator.enqueueSignal({ ...signal, kind: "completion" }),
      f.coordinator.enqueueSignal({ ...signal, kind: "timer" }),
    ]);
    expect(f.runtime.applySignal).toHaveBeenCalledOnce();
    expect(f.runtime.apply).toHaveBeenCalledExactlyOnceWith({ type: "pause" });
  });
  it("discards an expiry captured before a manual replacement", async () => {
    const f = fixture();
    const signal = { kind: "timer" as const, output: { kind: "pc" as const }, ownershipGeneration: 1, transitionGeneration: 1 };
    await f.coordinator.execute({ type: "next" }, { hostEpoch: "host" });
    await f.coordinator.enqueueSignal(signal);
    expect(f.runtime.apply).toHaveBeenCalledExactlyOnceWith({ type: "next" }, undefined);
  });
  it("validates repeated tracks by entry identity, not track ID", async () => {
    const f = fixture();
    f.state.commit({ queue: [{ entryId: "first", track: { id: 7 } as any }, { entryId: "second", track: { id: 7 } as any }] });
    expect((await f.coordinator.execute({ type: "queue_remove", entryId: "7" }, { hostEpoch: "host" })).status).toBe("failed");
    expect((await f.coordinator.execute({ type: "queue_remove", entryId: "second" }, { hostEpoch: "host" })).status).toBe("applied");
    expect(f.runtime.apply).toHaveBeenCalledExactlyOnceWith({ type: "queue_remove", entryId: "second" }, undefined);
  });
  it("recovers the lane after a caught runtime error", async () => {
    const f = fixture();
    f.runtime.apply = vi.fn().mockRejectedValueOnce(new Error("native refused")).mockResolvedValue(ok);
    expect(await f.coordinator.execute({ type: "pause" }, { hostEpoch: "host" })).toMatchObject({ status: "failed", revision: 0 });
    expect(await f.coordinator.execute({ type: "resume" }, { hostEpoch: "host" })).toEqual({ status: "applied", revision: 1 });
  });
});

it("keeps the origin generation until an in-flight natural transition finishes", async () => {
  const f = fixture();
  let finish!: (result: RuntimeResult) => void;
  f.runtime.applySignal = vi.fn().mockImplementationOnce(() => new Promise<RuntimeResult>(resolve => { finish = resolve; })).mockResolvedValue(ok);
  const capture = () => ({ kind: "completion" as const, output: f.state.read().selectedOutput, ownershipGeneration: f.state.read().ownershipGeneration, transitionGeneration: f.state.read().transitionGeneration });
  const first = f.coordinator.enqueueSignal(capture());
  await vi.waitFor(() => expect(f.runtime.applySignal).toHaveBeenCalledOnce());
  const duplicate = f.coordinator.enqueueSignal(capture());
  finish(ok);
  await first;
  await duplicate;
  expect(f.runtime.applySignal).toHaveBeenCalledOnce();
});

it("invalidates an old timer after a failed manual command with confirmed playback effects", async () => {
  const f = fixture();
  const timer = { kind: "timer" as const, output: { kind: "pc" as const }, ownershipGeneration: 1, transitionGeneration: 1 };
  f.runtime.apply = vi.fn().mockResolvedValue({ ...fail, partialEffects: ["Playback started"] });
  expect(await f.coordinator.execute({ type: "next" }, { hostEpoch: "host" })).toMatchObject({ status: "failed", partialEffects: ["Playback started"] });
  await f.coordinator.enqueueSignal(timer);
  expect(f.runtime.apply).toHaveBeenCalledOnce();
});

it("disposal cancels queued remote, local and signal work but drains the entered effect", async () => {
  const f = fixture();
  let finish!: (value: RuntimeResult) => void;
  f.runtime.apply = vi.fn(() => new Promise<RuntimeResult>(resolve => { finish = resolve; }));
  const entered = f.coordinator.execute({ type: "pause" }, { hostEpoch: "host" });
  await Promise.resolve();
  const remote = f.coordinator.execute({ type: "resume" }, { hostEpoch: "host" });
  let localRan = false;
  const local = f.coordinator.executeLocal(async () => { localRan = true; return ok; });
  const signal = f.coordinator.enqueueSignal({ kind: "completion", output: { kind: "pc" }, ownershipGeneration: 1, transitionGeneration: 1 });
  let drained = false;
  const disposing = f.coordinator.dispose().then(() => { drained = true; });
  expect(drained).toBe(false);
  finish(ok);
  expect(await entered).toMatchObject({ status: "applied" });
  // A queued resume would hang on the deliberately unresolved second runtime call.
  f.runtime.apply = vi.fn(async () => ok);
  expect(await remote).toMatchObject({ status: "failed", error: { code: "host_not_ready" } });
  expect(await local).toMatchObject({ status: "failed", error: { code: "host_not_ready" } });
  await signal; await disposing;
  expect(localRan).toBe(false);
  expect(f.runtime.applySignal).not.toHaveBeenCalled();
  expect(drained).toBe(true);
});
