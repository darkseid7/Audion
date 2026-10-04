import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { get } from "svelte/store";
vi.mock("$lib/services/native-audio", async original => ({
  ...await original<object>(), shouldUseNativeAudio: async () => true,
  nativeAudioPlay: vi.fn().mockResolvedValue(undefined), nativeAudioStop: vi.fn().mockResolvedValue(undefined),
  nativeAudioPreload: vi.fn().mockResolvedValue(undefined), nativeAudioSetVolume: vi.fn().mockResolvedValue(undefined),
  nativeAudioSetRepeatOne: vi.fn().mockResolvedValue(undefined), nativeAudioSetEq: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("$lib/api/tauri", async original => ({ ...await original<object>(), updateWindowsThumbarState: vi.fn().mockResolvedValue(undefined), initWindowsThumbar: vi.fn().mockResolvedValue(false) }));
import * as runtime from "./player-runtime";
import { createDesktopAdapter } from "./adapter";
import { appSettings } from "$lib/stores/settings";
const tracks = [1, 2, 1].map((id, i) => ({ id, title: `Occurrence ${i}`, path: `fixture${i}.flac`, cover_url: "fixture", duration: 100 } as any));
beforeEach(async () => {
  vi.stubGlobal("navigator", {});
  runtime.activeBackend.set("none"); runtime.queue.set([]); runtime.currentTrack.set(null);
  runtime.shuffle.set(false); runtime.repeat.set("none"); runtime.userQueueCount.set(0);
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
