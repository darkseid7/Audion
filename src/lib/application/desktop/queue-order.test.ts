import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { get } from "svelte/store";
vi.mock("$lib/services/native-audio", async original => ({
  ...await original<object>(), shouldUseNativeAudio: async () => true,
  nativeAudioPlay: vi.fn().mockResolvedValue(undefined), nativeAudioStop: vi.fn().mockResolvedValue(undefined),
  nativeAudioPreload: vi.fn().mockResolvedValue(undefined), nativeAudioSetVolume: vi.fn().mockResolvedValue(undefined),
  nativeAudioSetRepeatOne: vi.fn().mockResolvedValue(undefined), nativeAudioSetEq: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("$lib/api/tauri", async original => ({ ...await original<object>(), updateWindowsThumbarState: vi.fn().mockResolvedValue(undefined), initWindowsThumbar: vi.fn().mockResolvedValue(false), squeezeUpdateQueue: vi.fn().mockResolvedValue(undefined) }));
import * as runtime from "./player-runtime";
import { createDesktopAdapter } from "./adapter";
import { appSettings } from "$lib/stores/settings";
import { squeezeUpdateQueue } from "$lib/api/tauri";
import { activeSqueezePlayer, invalidateSqueezePollOwnership } from "$lib/stores/squeeze";
const tracks = [1, 2, 1].map((id, i) => ({ id, title: `Occurrence ${i}`, path: `fixture${i}.flac`, cover_url: "fixture", duration: 100 } as any));
beforeEach(async () => {
  vi.stubGlobal("navigator", {});
  runtime.activeBackend.set("none"); runtime.queue.set([]); runtime.currentTrack.set(null);
  runtime.shuffle.set(false); runtime.repeat.set("none"); runtime.userQueueCount.set(0);
  activeSqueezePlayer.set(null);
  vi.mocked(squeezeUpdateQueue).mockReset().mockResolvedValue(undefined);
  appSettings.setAutoplay(false);
  await runtime.initAudioBackend();
});
afterEach(() => { runtime.cleanupPlayer(); vi.unstubAllGlobals(); });
it.each([true, false].flatMap(playing => [0, 1, 2].map(index => ({ playing, index }))))("rejects active PC occurrence removal atomically ($playing/$index)", async ({ playing, index }) => {
  runtime.queue.set(tracks); runtime.queueIndex.set(index); runtime.currentTrack.set(tracks[index]);
  runtime.activeBackend.set("native"); runtime.isPlaying.set(playing);
  const adapter = createDesktopAdapter();
  try {
    const before = adapter.state.read();
    const result = await adapter.port.execute({ type: "queue_remove", entryId: before.queue[index].entryId }, { hostEpoch: before.hostEpoch });
    expect(result).toMatchObject({ status: "failed", error: { code: "unsupported" } });
    expect(get(runtime.queue)).toEqual(tracks);
    expect(get(runtime.currentTrack)).toBe(tracks[index]);
    expect(adapter.state.read().queue).toEqual(before.queue);
    await runtime.nextTrack();
    expect(get(runtime.queueIndex)).toBe(Math.min(index + 1, 2));
  } finally { await adapter.dispose(); }
});
it("allows removing default index zero when no track is current", async () => {
  runtime.queue.set(tracks); runtime.queueIndex.set(0);
  await runtime.removeFromQueue(0);
  expect(get(runtime.queue)).toEqual(tracks.slice(1));
});

it.each([
  { name: "second same-object occurrence", queue: [tracks[0], tracks[0]], order: [1, 0], cursor: 0, retained: [1], current: 0, history: [0] },
  { name: "non-prefix same-object history", queue: [tracks[0], tracks[1], tracks[0], tracks[2]], order: [3, 2, 0, 1], cursor: 1, retained: [2, 3], current: 0, history: [1, 0] },
  { name: "distinct objects with duplicate IDs", queue: tracks, order: [2, 0, 1], cursor: 1, retained: [0, 2], current: 0, history: [1, 0] },
])("preserves published occurrence identities when clearing $name", async fixture => {
  runtime.queue.set(fixture.queue); runtime.queueIndex.set(fixture.order[fixture.cursor]);
  runtime.currentTrack.set(fixture.queue[fixture.order[fixture.cursor]]);
  runtime.shuffle.set(true); runtime.shuffledIndices.set(fixture.order); runtime.shuffledIndex.set(fixture.cursor);
  runtime.activeBackend.set("native");
  const adapter = createDesktopAdapter();
  try {
    const before = adapter.state.read();
    const retainedIds = fixture.retained.map(index => before.queue[index].entryId);
    const currentId = before.queue[fixture.order[fixture.cursor]].entryId;
    expect(new Set(before.queue.map(entry => entry.entryId)).size).toBe(fixture.queue.length);
    const result = await adapter.port.execute({ type: "queue_clear_upcoming" }, { hostEpoch: before.hostEpoch });
    expect(result.status).toBe("applied");
    expect(get(runtime.queue)).toEqual(fixture.retained.map(index => fixture.queue[index]));
    expect(get(runtime.queueIndex)).toBe(fixture.current);
    expect(get(runtime.shuffledIndices)).toEqual(fixture.history);
    expect(get(runtime.shuffledIndex)).toBe(fixture.cursor);
    expect(adapter.state.read().queue.map(entry => entry.entryId)).toEqual(retainedIds);
    const snapshot = adapter.coordinator.captureSnapshot();
    expect(snapshot.queue.currentEntryId).toBe(currentId);
    expect(snapshot.revisions.queueRevision).toBe(before.revisions.queueRevision + 1);
    expect(adapter.coordinator.capturePresentation?.(snapshot)?.queue.map(entry => entry.entryId)).toEqual(retainedIds);
    expect(await adapter.port.query({ type: "queue" })).toMatchObject({ type: "queue", page: { items: retainedIds.map(entryId => ({ entryId })) } });
  } finally { await adapter.dispose(); }
});

it("preserves queue revision when shuffled clear has no upcoming occurrences", async () => {
  const repeated = tracks[0];
  runtime.queue.set([repeated, repeated]); runtime.queueIndex.set(0); runtime.currentTrack.set(repeated);
  runtime.shuffle.set(true); runtime.shuffledIndices.set([1, 0]); runtime.shuffledIndex.set(1);
  runtime.activeBackend.set("native");
  const adapter = createDesktopAdapter();
  try {
    const before = adapter.state.read();
    expect((await adapter.port.execute({ type: "queue_clear_upcoming" }, { hostEpoch: before.hostEpoch })).status).toBe("applied");
    expect(adapter.state.read().queue).toEqual(before.queue);
    expect(adapter.state.read().revisions.queueRevision).toBe(before.revisions.queueRevision);
    expect(adapter.coordinator.captureSnapshot().queue.currentEntryId).toBe(before.queue[0].entryId);
    expect(get(runtime.queueIndex)).toBe(0);
    expect(get(runtime.shuffledIndices)).toEqual([1, 0]);
    expect(get(runtime.shuffledIndex)).toBe(1);
  } finally { await adapter.dispose(); }
});

it.each(["acknowledged", "rejected", "stale"] as const)("stages retained identities only after an owned Squeeze clear is %s", async outcome => {
  const repeated = tracks[0];
  const original = [repeated, repeated];
  runtime.queue.set(original); runtime.queueIndex.set(1); runtime.currentTrack.set(repeated);
  runtime.shuffle.set(true); runtime.shuffledIndices.set([1, 0]); runtime.shuffledIndex.set(0);
  runtime.userQueueCount.set(1); activeSqueezePlayer.set("A"); runtime.activeBackend.set("squeeze");
  let finish!: () => void;
  let reject!: (error: Error) => void;
  let started!: () => void;
  const called = new Promise<void>(resolve => { started = resolve; });
  vi.mocked(squeezeUpdateQueue).mockImplementationOnce(() => {
    started();
    return new Promise<void>((resolve, fail) => { finish = resolve; reject = fail; });
  });
  const adapter = createDesktopAdapter();
  try {
    const before = adapter.state.read();
    const assertUnchanged = () => {
      expect(get(runtime.queue)).toBe(original);
      expect(get(runtime.queueIndex)).toBe(1);
      expect(get(runtime.shuffledIndices)).toEqual([1, 0]);
      expect(get(runtime.shuffledIndex)).toBe(0);
      expect(get(runtime.userQueueCount)).toBe(1);
      expect(adapter.state.read().queue).toEqual(before.queue);
      expect(adapter.state.read().revisions.queueRevision).toBe(before.revisions.queueRevision);
      expect(adapter.coordinator.captureSnapshot().queue.currentEntryId).toBe(before.queue[1].entryId);
    };
    const clearing = adapter.port.execute({ type: "queue_clear_upcoming" }, { hostEpoch: before.hostEpoch });
    await called;
    expect(squeezeUpdateQueue).toHaveBeenCalledExactlyOnceWith("A", [1], 1, 0, [1]);
    assertUnchanged();
    if (outcome === "rejected") reject(new Error("queue refused"));
    else { if (outcome === "stale") invalidateSqueezePollOwnership(); finish(); }
    const result = await clearing;
    if (outcome === "acknowledged") {
      expect(result.status).toBe("applied");
      expect(adapter.state.read().queue.map(entry => entry.entryId)).toEqual([before.queue[1].entryId]);
      expect(adapter.coordinator.captureSnapshot().queue.currentEntryId).toBe(before.queue[1].entryId);
      expect(adapter.state.read().revisions.queueRevision).toBe(before.revisions.queueRevision + 1);
    } else {
      expect(result).toMatchObject({ status: outcome === "stale" ? "superseded" : "failed", error: { code: outcome === "stale" ? "revision_conflict" : "execution_failed" } });
      assertUnchanged();
      // A later observation commit must not consume leaked pendingEntries.
      adapter.state.commit({});
      assertUnchanged();
    }
  } finally { await adapter.dispose(); }
});

it.each([
  { order: [0, 2, 1], cursor: 1, retained: [0, 2], current: 1 },
  { order: [2, 0, 1], cursor: 1, retained: [0, 2], current: 0 },
])("clears only shuffled upcoming occurrences ($order)", async ({ order, cursor, retained, current }) => {
  runtime.queue.set(tracks); runtime.queueIndex.set(order[cursor]); runtime.currentTrack.set(tracks[order[cursor]]);
  runtime.shuffle.set(true); runtime.shuffledIndices.set(order); runtime.shuffledIndex.set(cursor);
  runtime.activeBackend.set("native");
  await runtime.clearUpcoming();
  expect(get(runtime.queue)).toEqual(retained.map(i => tracks[i]));
  expect(get(runtime.queueIndex)).toBe(current);
  expect(get(runtime.shuffledIndices)).toEqual(order.slice(0, cursor + 1).map(i => retained.indexOf(i)));
  await runtime.nextTrack();
  expect(get(runtime.queueIndex)).toBe(current);
  expect(get(runtime.isPlaying)).toBe(false);
});
it.each([false, true])("appends at shuffled tail without replay, with immediate insertion=%s", async immediate => {
  await runtime.playTracks(tracks);
  runtime.shuffle.set(true); runtime.shuffledIndices.set([0, 2, 1]); runtime.shuffledIndex.set(0);
  const added = (title: string) => ({ ...tracks[0], title });
  if (immediate) {
    await runtime.addToQueue([added("queued")]);
    await runtime.playNext([added("next")]);
    await runtime.addToQueue([added("queued second")]);
  }
  await runtime.appendToQueueEnd([added("tail")]);
  const heard: string[] = [];
  for (let i = 0; i < (immediate ? 6 : 3); i++) {
    await runtime.nextTrack(); heard.push(get(runtime.currentTrack)!.title!);
  }
  expect(heard).toEqual(immediate
    ? ["next", "queued", "queued second", "Occurrence 2", "Occurrence 1", "tail"]
    : ["Occurrence 2", "Occurrence 1", "tail"]);
  await runtime.nextTrack();
  expect(get(runtime.isPlaying)).toBe(false);
  expect(get(runtime.userQueueCount)).toBe(0);
});
