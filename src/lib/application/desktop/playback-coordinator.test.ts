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
    f.runtime.stopOwnedOutput = vi.fn(() => new Promise(resolve => { stop = resolve; }));
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
    f.runtime.stopOwnedOutput = vi.fn(async () => {
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
