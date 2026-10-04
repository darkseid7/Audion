import type { Track } from "$lib/api/tauri";
import type { PlaybackContext } from "$lib/stores/playback-state";
import type { HostSnapshot, HostPresentation, ApplicationIntent, CommandPreconditions, ControlError, DomainRevisions, ExecutionResult, OutputRef, SnapshotOutputRef } from "../types";

export type PlaybackIntent = ApplicationIntent;
export interface HostQueueEntry { entryId: string; track: Track }
export interface HostState {
  hostEpoch: string;
  /** Desktop-local commit counter. Native publication owns the LAN replay revision. */
  revision: number;
  revisions: DomainRevisions;
  selectedOutput: SnapshotOutputRef;
  ownershipGeneration: number;
  transitionGeneration: number;
  queue: HostQueueEntry[];
}
export type HostDelta = Partial<Omit<HostState, "revisions">> & { revisions?: Partial<DomainRevisions> };
export interface HostStateAccess { read(): HostState; commit(delta: HostDelta): void }
export interface ResolvedPlayback { tracks: Track[]; startIndex: number; context: PlaybackContext | null }
export type RuntimeResult = { status: "applied" } | { status: "failed" | "superseded"; error: ControlError; partialEffects: string[] };
export interface PlaybackSignal {
  kind: "completion" | "gapless" | "timer";
  output: SnapshotOutputRef;
  ownershipGeneration: number;
  transitionGeneration: number;
}
export interface DesktopPlaybackRuntime {
  refreshLibrary?(): Promise<void>;
  validateOutput(output: OutputRef): Promise<RuntimeResult>;
  resolvePlayback(intent: PlaybackIntent): Promise<ResolvedPlayback>;
  stopOwnedOutput(output: OutputRef): Promise<RuntimeResult>;
  selectOutput(output: OutputRef): Promise<RuntimeResult>;
  apply(intent: PlaybackIntent, resolved?: ResolvedPlayback): Promise<RuntimeResult>;
  applySignal(signal: PlaybackSignal): Promise<RuntimeResult>;
}
export interface HostProjectionAccess { presentation?(snapshot: HostSnapshot): HostPresentation; read(): HostSnapshot; subscribe(listener: (snapshot: HostSnapshot) => void): () => void }
export interface PlaybackCoordinator {
  capturePresentation?(snapshot: HostSnapshot): HostPresentation | undefined;
  captureSnapshot(): HostSnapshot;
  subscribeSnapshot(listener: (snapshot: HostSnapshot) => void): () => void;
  execute(intent: PlaybackIntent, context: CommandPreconditions): Promise<ExecutionResult>;
  /** Native desktop binding only; captures revisions after the in-lane refresh. Not a wire trust flag. */
  executeDesktopIntent?(intent: PlaybackIntent, capturedEpoch: string): Promise<ExecutionResult>;
  enqueueSignal(signal: PlaybackSignal): Promise<void>;
  /** Desktop-only commands use the same lane, never the network protocol. */
  executeLocal(operation: () => Promise<RuntimeResult>, replacesPlayback?: boolean): Promise<ExecutionResult>;
  dispose(): Promise<void>;
}
export const sameOutput = (a: SnapshotOutputRef, b: SnapshotOutputRef): boolean => a.kind === b.kind && (a.kind === "pc" || a.kind === "desktop_only" && b.kind === "desktop_only" && a.reason === b.reason || a.kind === "squeeze" && b.kind === "squeeze" && a.playerId === b.playerId);
const replacement = (intent: PlaybackIntent) => intent.type.startsWith("play_") || ["next", "previous", "queue_play"].includes(intent.type);
const resolvesTracks = (intent: PlaybackIntent) => intent.type.startsWith("play_") || intent.type === "queue_insert" || intent.type === "queue_append" || intent.type === "queue_entity";
export function runtimeFailure(error: unknown): RuntimeResult {
  if (error instanceof PlaybackFailure) return { status: error.status, error: error.controlError, partialEffects: error.partialEffects };
  return { status: "failed", error: { code: "execution_failed", message: error instanceof Error ? error.message : String(error), retryable: false }, partialEffects: [] };
}
export class PlaybackFailure extends Error {
  constructor(public controlError: ControlError, public status: "failed" | "superseded" = "failed", public partialEffects: string[] = []) { super(controlError.message); }
}
/** All preconditions are checked in the lane, not at request arrival. */
export function createPlaybackCoordinator(runtime: DesktopPlaybackRuntime, state: HostStateAccess, projection?: HostProjectionAccess): PlaybackCoordinator {
  let tail: Promise<unknown> = Promise.resolve();
  let disposed = false;
  const enqueue = <T>(operation: () => Promise<T>): Promise<T> => {
    const result = tail.then(operation);
    tail = result.catch(() => {});
    return result;
  };
  const failed = (code: ControlError["code"], message: string, status: "failed" | "superseded" = "failed"): ExecutionResult => ({ status, error: { code, message, retryable: false }, revision: state.read().revision, partialEffects: [] });
  const result = (value: RuntimeResult, partialEffects: string[] = []): ExecutionResult => value.status === "applied"
    ? { status: "applied", revision: state.read().revision }
    : { ...value, revision: state.read().revision, partialEffects: [...partialEffects, ...value.partialEffects] };
  const attempt = async (operation: () => Promise<RuntimeResult>): Promise<RuntimeResult> => { try { return await operation(); } catch (error) { return runtimeFailure(error); } };
  const advanceGeneration = (manual: boolean) => { const s = state.read(); state.commit({ transitionGeneration: s.transitionGeneration + 1, ...(manual ? { ownershipGeneration: s.ownershipGeneration + 1 } : {}) }); };
  const executeIntent = (intent: PlaybackIntent, context: CommandPreconditions | (() => CommandPreconditions)): Promise<ExecutionResult> => {
    if (disposed) return Promise.resolve(failed("host_not_ready", "Playback coordinator disposed"));
    return enqueue(async () => {
      if (disposed) return failed("host_not_ready", "Playback coordinator disposed");
      if (runtime.refreshLibrary && resolvesTracks(intent)) {
        const refreshed = await attempt(async () => { await runtime.refreshLibrary!(); return { status: "applied" }; });
        if (refreshed.status !== "applied") return result(refreshed);
        if (disposed) return failed("host_not_ready", "Playback coordinator disposed");
      }
      const expected = typeof context === "function" ? context() : context;
      const before = state.read();
      if (expected.hostEpoch !== before.hostEpoch) return failed("resync_required", "Host epoch changed", "superseded");
      for (const key of ["queueRevision", "outputRevision", "libraryRevision"] as const) {
        if (expected[key] !== undefined && expected[key] !== before.revisions[key]) return failed("revision_conflict", `${key} changed`, "superseded");
      }
      if (before.selectedOutput.kind === "desktop_only") return failed("unsupported", "Legacy cloud output is desktop-only");
      const previousOutput = before.selectedOutput;
      if (intent.type === "select_output") {
        if (sameOutput(before.selectedOutput, intent.output)) return result({ status: "applied" });
        const validated = await attempt(() => runtime.validateOutput(intent.output));
        if (validated.status !== "applied") return result(validated);
        const stopped = await attempt(() => runtime.stopOwnedOutput(previousOutput));
        if (stopped.status !== "applied") return result(stopped);
        // The stop is already a real effect even if selecting the new target fails.
        advanceGeneration(true);
        const selected = await attempt(() => runtime.selectOutput(intent.output));
        if (selected.status !== "applied") return result(selected, ["Previous output stopped"]);
        state.commit({ selectedOutput: intent.output, revisions: { outputRevision: before.revisions.outputRevision + 1 } });
        return result(selected);
      }
      if ("entryId" in intent) {
        if (!before.queue.some(entry => entry.entryId === intent.entryId) || intent.type === "queue_reorder" && intent.beforeEntryId !== null && !before.queue.some(entry => entry.entryId === intent.beforeEntryId)) return failed("not_found", "Queue entry no longer exists");
      }
      const applied = await attempt(async () => {
        const resolved = resolvesTracks(intent) ? await runtime.resolvePlayback(intent) : undefined;
        if (resolved) {
          await runtime.refreshLibrary?.();
          if (state.read().revisions.libraryRevision !== before.revisions.libraryRevision) throw new PlaybackFailure({ code: "revision_conflict", message: "Library changed while resolving playback", retryable: false }, "superseded");
        }
        return runtime.apply(intent, resolved);
      });
      if (applied.status !== "applied" && applied.partialEffects.length && replacement(intent)) advanceGeneration(true);
      if (applied.status === "applied") {
        if (replacement(intent)) advanceGeneration(true);
        else state.commit({});
      }
      return result(applied);
    });
  };
  return {
    capturePresentation(snapshot) { return projection?.presentation?.(snapshot); },
    captureSnapshot() {
      if (disposed || !projection) throw new Error("Host projection unavailable");
      return projection.read();
    },
    subscribeSnapshot(listener) {
      if (disposed || !projection) throw new Error("Host projection unavailable");
      return projection.subscribe(listener);
    },
    execute: (intent, context) => executeIntent(intent, context),
    executeDesktopIntent: (intent, capturedEpoch) => executeIntent(intent, () => ({ hostEpoch: capturedEpoch, ...state.read().revisions })),
    enqueueSignal(signal) {
      if (disposed) return Promise.resolve();
      return enqueue(async () => {
        if (disposed) return;
        const current = state.read();
        if (!sameOutput(signal.output, current.selectedOutput) || signal.ownershipGeneration !== current.ownershipGeneration || signal.kind !== "timer" && signal.transitionGeneration !== current.transitionGeneration) return;
        const applied = await attempt(() => signal.kind === "timer" ? runtime.apply({ type: "pause" }) : runtime.applySignal(signal));
        // Keep the origin while awaiting so duplicates arriving in-flight retain
        // the old generation. Consume it even on failure (scrobble may have run).
        if (signal.kind !== "timer") advanceGeneration(false);
        if (applied.status !== "applied") throw new PlaybackFailure(applied.error, applied.status, applied.partialEffects);
        if (signal.kind === "timer") state.commit({});
      });
    },
    executeLocal(operation, replacesPlayback = false) {
      if (disposed) return Promise.resolve(failed("host_not_ready", "Playback coordinator disposed"));
      return enqueue(async () => {
        if (disposed) return failed("host_not_ready", "Playback coordinator disposed");
        const applied = await attempt(operation);
        if (applied.status !== "applied" && applied.partialEffects.length && replacesPlayback) advanceGeneration(true);
        if (applied.status === "applied") { if (replacesPlayback) advanceGeneration(true); else state.commit({}); }
        return result(applied);
      });
    },
    async dispose() { disposed = true; await tail; },
  };
}
