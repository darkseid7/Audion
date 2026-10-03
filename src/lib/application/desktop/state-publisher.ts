import type { PlaybackCoordinator } from "./playback-coordinator";
import type { HostSnapshot } from "../types";
export type PublisherStop = (() => void) & { ready: Promise<void>; flush(): Promise<number> };
const publishers = new WeakMap<PlaybackCoordinator, PublisherStop>();
const structural = (s: HostSnapshot) => JSON.stringify({ ...s, revision: 0, playback: { ...s.playback, position: 0 } });
/** One active writer and one latest pending projection; never an unbounded send queue. */
export function startStatePublisher(coordinator: PlaybackCoordinator, publish: (snapshot: HostSnapshot) => Promise<number>, onFailure: () => void = () => {}): PublisherStop {
  const existing = publishers.get(coordinator);
  if (existing) return existing;
  let failure: Error | undefined;
  let busy = false;
  let pending: { snapshot: HostSnapshot; sequence: number } | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let unsubscribe = () => {};
  let cancelSend: ((error: Error) => void) | undefined;
  let last = coordinator.captureSnapshot();
  let observed = 1;
  let acknowledged = 0;
  let revision = 0;
  const barriers = new Set<{ sequence: number; resolve(revision: number): void; reject(error: Error): void }>();
  const terminate = (error: Error) => {
    if (failure) return;
    failure = error; pending = undefined;
    if (timer !== undefined) clearTimeout(timer);
    unsubscribe(); publishers.delete(coordinator);
    cancelSend?.(error);
    for (const barrier of barriers) barrier.reject(error);
    barriers.clear();
  };
  const stop: PublisherStop = Object.assign(() => terminate(new Error("State publisher stopped")), {
    ready: Promise.resolve(),
    flush(): Promise<number> {
      if (failure) return Promise.reject(failure);
      // Capture a barrier, not a promise tail extended by future telemetry.
      const target = observed;
      if (acknowledged >= target) return Promise.resolve(revision);
      const result = new Promise<number>((resolve, reject) => { barriers.add({ sequence: target, resolve, reject }); });
      if (timer !== undefined) clearTimeout(timer);
      drain();
      return result;
    },
  });
  publishers.set(coordinator, stop);
  const send = async (item: { snapshot: HostSnapshot; sequence: number }): Promise<void> => {
    busy = true;
    let deadline: ReturnType<typeof setTimeout> | undefined;
    try {
      revision = await Promise.race([
        publish(item.snapshot),
        new Promise<never>((_, reject) => {
          cancelSend = reject;
          deadline = setTimeout(() => reject(new Error("State publication timed out")), 5000);
        }),
      ]);
      if (failure) throw failure;
      acknowledged = item.sequence;
      for (const barrier of barriers) if (barrier.sequence <= acknowledged) { barriers.delete(barrier); barrier.resolve(revision); }
    } catch (error) {
      const wasActive = !failure;
      terminate(error instanceof Error ? error : new Error("State publication failed"));
      if (wasActive) onFailure();
      throw error;
    } finally {
      clearTimeout(deadline); cancelSend = undefined; busy = false;
      if (!failure && pending && timer === undefined) drain();
    }
  };
  const drain = () => {
    timer = undefined;
    if (failure || busy || !pending) return;
    const item = pending; pending = undefined;
    void send(item).catch(() => {});
  };
  unsubscribe = coordinator.subscribeSnapshot(snapshot => {
    if (failure || JSON.stringify(last) === JSON.stringify(snapshot)) return;
    const progressOnly = structural(last) === structural(snapshot);
    last = snapshot; pending = { snapshot, sequence: ++observed };
    if (progressOnly) timer ??= setTimeout(drain, 250);
    else { if (timer !== undefined) clearTimeout(timer); drain(); }
  });
  stop.ready = send({ snapshot: last, sequence: observed });
  void stop.ready.catch(() => {});
  return stop;
}
