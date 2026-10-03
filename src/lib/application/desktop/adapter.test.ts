vi.mock("$lib/stores/activity", () => ({ recordTrackPlay: vi.fn() }));
import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
const api = vi.hoisted(() => ({ play: vi.fn(), stop: vi.fn(), tracks: vi.fn(), players: vi.fn() }));
vi.mock("$lib/api/tauri", async importOriginal => ({
  ...await importOriginal<object>(),
  squeezeNext: vi.fn().mockResolvedValue(undefined), squeezePrevious: vi.fn().mockResolvedValue(undefined), squeezeSeek: vi.fn().mockResolvedValue(undefined), getLikedTrackIds: vi.fn().mockResolvedValue([7]), squeezeGetPlayerState: vi.fn(), squeezeUpdateQueue: vi.fn().mockResolvedValue(undefined), squeezePause: vi.fn().mockResolvedValue(undefined), squeezePlay: api.play, squeezeStop: api.stop, squeezeDisconnectPlayer: vi.fn().mockResolvedValue(undefined), getTracksByAlbum: api.tracks,
  squeezeGetPlayers: api.players, getTrackById: async (id: number) => ({ id, title: `Track ${id}`, duration: 100, cover_url: "fixture" }),
}));
import { createDesktopAdapter } from "./adapter";
import { playbackStateWriter as state } from "$lib/stores/playback-state";
import { activeSqueezePlayer, commitSqueezeTarget } from "$lib/stores/squeeze";

beforeEach(() => {
  vi.stubGlobal("navigator", {});
  state.activeBackend.set("none"); activeSqueezePlayer.set(null);
  state.queue.set([]); state.queueIndex.set(0); state.currentTrack.set(null);
  state.isPlaying.set(false); state.shuffle.set(false);
  api.play.mockReset().mockResolvedValue(undefined); api.stop.mockReset().mockResolvedValue(undefined);
  api.tracks.mockReset().mockResolvedValue([{ id: 7, duration: 100, cover_url: "fixture" }, { id: 7, duration: 100, cover_url: "fixture" }]);
  api.players.mockReset().mockResolvedValue([{ mac: "A", name: "Living room" }]);
});
describe("desktop adapter execution", () => {
  it("resolves entity playback in the host and exposes distinct repeated queue entries", async () => {
    const adapter = createDesktopAdapter();
    const context = () => ({ hostEpoch: adapter.state.read().hostEpoch });
    expect((await adapter.port.execute({ type: "select_output", output: { kind: "squeeze", playerId: "A" } }, context())).status).toBe("applied");
    expect((await adapter.port.execute({ type: "play_album", albumId: 5, playMode: "all" }, context())).status).toBe("applied");
    expect(api.tracks).toHaveBeenCalledWith(5);
    expect(api.play).toHaveBeenCalledWith("A", [7, 7], 0);
    const entries = adapter.state.read().queue;
    expect(entries).toHaveLength(2);
    expect(entries[0].entryId).not.toBe(entries[1].entryId);
    await adapter.dispose();
  });
  it("does not acknowledge a failed play or publish its queue", async () => {
    commitSqueezeTarget("A");
    const adapter = createDesktopAdapter();
    api.play.mockRejectedValueOnce(new Error("device refused"));
    const result = await adapter.port.execute({ type: "play_album", albumId: 5, playMode: "all" }, { hostEpoch: adapter.state.read().hostEpoch });
    expect(result).toMatchObject({ status: "failed", error: { code: "execution_failed" } });
    expect(get(state.queue)).toEqual([]);
    expect(adapter.state.read().queue).toEqual([]);
    await adapter.dispose();
  });
});

it("stops then disconnects Squeeze before confirming PC selection", async () => {
  commitSqueezeTarget("A");
  const adapter = createDesktopAdapter();
  const calls: string[] = [];
  api.stop.mockImplementationOnce(async () => { calls.push("stop"); });
  const { squeezeDisconnectPlayer } = await import("$lib/api/tauri");
  vi.mocked(squeezeDisconnectPlayer).mockImplementationOnce(async () => { calls.push("disconnect"); });
  const result = await adapter.port.execute({ type: "select_output", output: { kind: "pc" } }, { hostEpoch: adapter.state.read().hostEpoch });
  expect(result.status).toBe("applied");
  expect(calls).toEqual(["stop", "disconnect"]);
  expect(get(activeSqueezePlayer)).toBe(null);
  await adapter.dispose();
});

it("advances and records once for duplicate gapless/completion, while timer only pauses", async () => {
  const { recordTrackPlay } = await import("$lib/stores/activity");
  const { squeezePause } = await import("$lib/api/tauri");
  vi.spyOn(Date, "now").mockReturnValue(1000);
  api.tracks.mockResolvedValueOnce([{ id: 1, duration: 100, cover_url: "fixture" }, { id: 2, duration: 100, cover_url: "fixture" }]);
  commitSqueezeTarget("A");
  const adapter = createDesktopAdapter();
  await adapter.port.execute({ type: "play_album", albumId: 1, playMode: "all" }, { hostEpoch: adapter.state.read().hostEpoch });
  vi.spyOn(Date, "now").mockReturnValue(11000);
  const current = adapter.state.read();
  const origin = { output: current.selectedOutput, ownershipGeneration: current.ownershipGeneration, transitionGeneration: current.transitionGeneration };
  await Promise.all([
    adapter.coordinator.enqueueSignal({ ...origin, kind: "gapless" }),
    adapter.coordinator.enqueueSignal({ ...origin, kind: "completion" }),
    adapter.coordinator.enqueueSignal({ ...origin, kind: "timer" }),
  ]);
  expect(get(state.queueIndex)).toBe(1);
  expect(get(state.currentTrack)?.id).toBe(2);
  expect(recordTrackPlay).toHaveBeenCalledExactlyOnceWith(1, null, 10);
  expect(squeezePause).toHaveBeenCalledExactlyOnceWith("A");
  expect(get(state.isPlaying)).toBe(false);
  await adapter.dispose();
  vi.restoreAllMocks();
});

it("never reports a controller command applied while legacy cloud owns output", async () => {
  state.activeBackend.set("remote");
  const adapter = createDesktopAdapter();
  const result = await adapter.port.execute({ type: "select_output", output: { kind: "pc" } }, { hostEpoch: adapter.state.read().hostEpoch });
  expect(result).toMatchObject({ status: "failed", error: { code: "unsupported" } });
  expect(get(state.activeBackend)).toBe("remote");
  await adapter.dispose();
});

it("preserves the playing occurrence when an earlier duplicate is removed", async () => {
  const { squeezeUpdateQueue } = await import("$lib/api/tauri");
  const first = { id: 7, cover_url: "fixture" } as any;
  const second = { id: 7, cover_url: "fixture" } as any;
  state.queue.set([first, second]); state.queueIndex.set(1); state.currentTrack.set(second);
  commitSqueezeTarget("A");
  const adapter = createDesktopAdapter();
  const [firstEntry, secondEntry] = adapter.state.read().queue;
  expect((await adapter.port.execute({ type: "queue_remove", entryId: firstEntry.entryId }, { hostEpoch: adapter.state.read().hostEpoch })).status).toBe("applied");
  expect(squeezeUpdateQueue).toHaveBeenCalledWith("A", [7], 7, 0, [1]);
  expect(adapter.state.read().queue.map(item => item.entryId)).toEqual([secondEntry.entryId]);
  expect(get(state.queueIndex)).toBe(0);
  await adapter.dispose();
});

it("reflects a server-owned Squeeze disconnect without leaving a selected phantom output", async () => {
  const { clearSqueezeTarget } = await import("$lib/stores/squeeze");
  commitSqueezeTarget("A");
  const adapter = createDesktopAdapter();
  clearSqueezeTarget();
  await vi.waitFor(() => expect(adapter.state.read().selectedOutput).toEqual({ kind: "pc" }));
  await adapter.dispose();
});

it("does not let an old Squeeze sample overwrite a confirmed pause", async () => {
  const { initializeSqueeze } = await import("$lib/stores/squeeze");
  const { squeezeGetPlayerState } = await import("$lib/api/tauri");
  let finish!: (state: any) => void;
  vi.mocked(squeezeGetPlayerState).mockReturnValueOnce(new Promise(resolve => { finish = resolve; }));
  commitSqueezeTarget("A"); state.isPlaying.set(true);
  const adapter = createDesktopAdapter();
  const stopPolling = initializeSqueeze();
  await adapter.port.execute({ type: "pause" }, { hostEpoch: adapter.state.read().hostEpoch });
  finish({ mac: "A", name: "A", state: "Playing", capabilities: "", current_track: null, elapsed_ms: 0, volume: 70, repeat: "Off", shuffle: false, queue_length: 0, queue_position: null, current_queue_index: null });
  await new Promise(resolve => setTimeout(resolve, 10));
  expect(get(state.isPlaying)).toBe(false);
  stopPolling(); await adapter.dispose();
});

it("preserves host-only plugin tracks through the registered desktop list facade", async () => {
  const { appendToQueueEnd } = await import("$lib/stores/player");
  const adapter = createDesktopAdapter();
  const custom = { id: -8, title: "Plugin stream", path: "plugin://live", source_type: "plugin" } as any;
  await appendToQueueEnd([custom]);
  expect(get(state.queue)[0]).toBe(custom);
  await adapter.dispose();
});

it("preserves liked-only album context in the confirmed host snapshot", async () => {
  commitSqueezeTarget("A");
  const adapter = createDesktopAdapter();
  await adapter.port.execute({ type: "play_album", albumId: 5, playMode: "liked_only" }, { hostEpoch: adapter.state.read().hostEpoch });
  const result = await adapter.port.query({ type: "snapshot" });
  expect(result.type === "snapshot" && result.snapshot.playback.context).toEqual({ type: "album", albumId: 5, playMode: "liked_only" });
  await adapter.dispose();
});

it("keeps legacy cloud ownership explicit in the internal host state", async () => {
  state.activeBackend.set("remote");
  const adapter = createDesktopAdapter();
  expect(adapter.state.read().selectedOutput).toMatchObject({ kind: "desktop_only" });
  await adapter.dispose();
});

it.each(["track_end", "album_end"] as const)("awaits %s completion pause before confirming, without advancing", async mode => {
  const { armTrackEndTimer, armAlbumEndTimer, stopSleepTimer } = await import("$lib/stores/sleepTimer");
  const { squeezePause } = await import("$lib/api/tauri");
  const track = { id: 7, album_id: 1, duration: 100, cover_url: "fixture" } as any;
  state.queue.set([track, { ...track, id: 8, album_id: 2 }]); state.currentTrack.set(track);
  state.isPlaying.set(true); commitSqueezeTarget("A");
  const adapter = createDesktopAdapter();
  let acknowledge!: () => void;
  vi.mocked(squeezePause).mockClear().mockReturnValueOnce(new Promise(resolve => { acknowledge = resolve; }));
  mode === "track_end" ? armTrackEndTimer() : armAlbumEndTimer(1);
  const owner = adapter.state.read();
  let settled = false;
  const pending = adapter.coordinator.enqueueSignal({ kind: "completion", output: owner.selectedOutput, ownershipGeneration: owner.ownershipGeneration, transitionGeneration: owner.transitionGeneration }).finally(() => { settled = true; });
  try {
    await new Promise(resolve => setTimeout(resolve, 0));
    expect(squeezePause).toHaveBeenCalledExactlyOnceWith("A");
    expect(settled).toBe(false); expect(get(state.isPlaying)).toBe(true);
    expect(get(state.queueIndex)).toBe(0);
    acknowledge(); await pending;
    expect(get(state.isPlaying)).toBe(false); expect(api.play).not.toHaveBeenCalled();
  } finally { acknowledge?.(); await pending; stopSleepTimer(false); await adapter.dispose(); }
});

it.each(["track_end", "album_end"] as const)("surfaces %s completion pause rejection without false confirmation", async mode => {
  const { armTrackEndTimer, armAlbumEndTimer, stopSleepTimer } = await import("$lib/stores/sleepTimer");
  const { squeezePause } = await import("$lib/api/tauri");
  const track = { id: 7, album_id: 1, duration: 100, cover_url: "fixture" } as any;
  state.queue.set([track]); state.currentTrack.set(track); state.isPlaying.set(true); commitSqueezeTarget("A");
  const adapter = createDesktopAdapter();
  vi.mocked(squeezePause).mockRejectedValueOnce(new Error("pause refused"));
  mode === "track_end" ? armTrackEndTimer() : armAlbumEndTimer(1);
  const owner = adapter.state.read();
  try {
    await expect(adapter.coordinator.enqueueSignal({ kind: "completion", output: owner.selectedOutput, ownershipGeneration: owner.ownershipGeneration, transitionGeneration: owner.transitionGeneration })).rejects.toThrow("pause refused");
    expect(get(state.isPlaying)).toBe(true); expect(get(state.queueIndex)).toBe(0); expect(api.play).not.toHaveBeenCalled();
  } finally { stopSleepTimer(false); await adapter.dispose(); }
});

it.each([false, true])("keeps a watchdog alive through its own sample commit, replaced=%s", async replaced => {
  vi.useFakeTimers();
  const { initializeSqueeze, resetSqueezePollingForTests } = await import("$lib/stores/squeeze");
  const { squeezeGetPlayerState } = await import("$lib/api/tauri");
  const track = { id: 7, duration: 100, cover_url: "fixture" } as any;
  state.queue.set([track]); state.currentTrack.set(null); state.duration.set(100);
  commitSqueezeTarget("A");
  const adapter = createDesktopAdapter();
  vi.mocked(squeezeGetPlayerState).mockResolvedValue({ mac: "A", name: "A", state: "Playing", capabilities: "", current_track: track, elapsed_ms: 100000, volume: 70, repeat: "Off", shuffle: false, queue_length: 1, queue_position: 0, current_queue_index: 0 });
  const cleanup = initializeSqueeze();
  try {
    await vi.advanceTimersByTimeAsync(0);
    expect(get(state.currentTrack)?.id).toBe(7);
    if (replaced) {
      await adapter.port.execute({ type: "play_album", albumId: 1, playMode: "all" }, { hostEpoch: adapter.state.read().hostEpoch });
      // Withhold subsequent samples: only the original watchdog may run.
      vi.mocked(squeezeGetPlayerState).mockReturnValue(new Promise(() => {}));
    }
    await vi.advanceTimersByTimeAsync(3500);
    expect(api.stop).toHaveBeenCalledTimes(replaced ? 0 : 1);
  } finally { cleanup(); resetSqueezePollingForTests(); await adapter.dispose(); vi.useRealTimers(); }
});

it.each(["next", "previous", "seek"] as const)("reconciles acknowledged Squeeze %s before the next queue edit without polling", async type => {
  const native = await import("$lib/api/tauri");
  const track = { id: 7, duration: 100, cover_url: "fixture" } as any;
  const next = { ...track, id: 8 };
  state.queue.set([track, next]); state.queueIndex.set(0); state.currentTrack.set(track); state.currentTime.set(20); state.duration.set(100);
  commitSqueezeTarget("A");
  vi.mocked(native.squeezeGetPlayerState).mockResolvedValue({ mac: "A", name: "A", state: "Playing", capabilities: "", current_track: next, elapsed_ms: 30000, volume: 70, repeat: "Off", shuffle: false, queue_length: 2, queue_position: 1, current_queue_index: 1 });
  const adapter = createDesktopAdapter();
  const context = () => ({ hostEpoch: adapter.state.read().hostEpoch });
  const result = await adapter.port.execute(type === "seek" ? { type, seconds: 30 } : { type }, context());
  expect(result.status).toBe("applied");
  const removed = await adapter.port.execute({ type: "queue_remove", entryId: adapter.state.read().queue[1].entryId }, context());
  expect(removed).toMatchObject({ status: "failed", error: { code: "unsupported" } });
  expect(get(state.currentTrack)?.id).toBe(8); expect(get(state.queueIndex)).toBe(1); expect(get(state.currentTime)).toBe(30);
  await adapter.dispose();
});

it("discloses acknowledged navigation when its authoritative state read fails", async () => {
  const { squeezeGetPlayerState } = await import("$lib/api/tauri");
  commitSqueezeTarget("A"); const adapter = createDesktopAdapter();
  vi.mocked(squeezeGetPlayerState).mockRejectedValueOnce(new Error("state unavailable"));
  const result = await adapter.port.execute({ type: "next" }, { hostEpoch: adapter.state.read().hostEpoch });
  expect(result).toMatchObject({ status: "failed", partialEffects: ["Squeeze navigation acknowledged"] });
  await adapter.dispose();
});

it.each([
  { type: "next", duplicate: false, elapsed: 20, records: 1 },
  { type: "previous", duplicate: false, elapsed: 20, records: 1 },
  { type: "next", duplicate: true, elapsed: 20, records: 1 },
  { type: "previous", duplicate: true, elapsed: 20, records: 1 },
  { type: "seek", duplicate: true, elapsed: 20, records: 0 },
  { type: "next", duplicate: false, elapsed: 5.9, records: 0 },
  { type: "next", duplicate: false, elapsed: 6, records: 1 },
] as const)("preserves navigation accounting once: $type duplicate=$duplicate elapsed=$elapsed", async ({ type, duplicate, elapsed, records }) => {
  vi.useFakeTimers();
  const { recordTrackPlay } = await import("$lib/stores/activity");
  const library = await import("$lib/stores/library");
  const increment = vi.spyOn(library, "incrementPlayCount").mockImplementation(() => {});
  const { squeezeGetPlayerState, squeezePause } = await import("$lib/api/tauri");
  const { initializeSqueeze, resetSqueezePollingForTests } = await import("$lib/stores/squeeze");
  const { armTrackEndTimer, isTimerModeTrackOrAlbumEnd, stopSleepTimer } = await import("$lib/stores/sleepTimer");
  const tracks = [{ id: 71, album_id: 44, duration: 100, cover_url: "fixture" }, { id: duplicate ? 71 : 72, album_id: 44, duration: 100, cover_url: "fixture" }] as any;
  const from = type === "previous" ? 1 : 0;
  const to = type === "seek" ? from : 1 - from;
  state.queue.set(tracks); state.queueIndex.set(from); state.currentTrack.set(tracks[from]);
  state.currentTime.set(elapsed); state.duration.set(100); state.isPlaying.set(true);
  commitSqueezeTarget("A");
  vi.mocked(recordTrackPlay).mockClear(); vi.mocked(squeezePause).mockClear();
  vi.mocked(squeezeGetPlayerState).mockResolvedValue({ mac: "A", name: "A", state: "Playing", capabilities: "", current_track: tracks[to], elapsed_ms: type === "seek" ? 30000 : 1000, volume: 70, repeat: "Off", shuffle: false, queue_length: 2, queue_position: to, current_queue_index: to });
  const adapter = createDesktopAdapter();
  armTrackEndTimer();
  let cleanup: (() => void) | undefined;
  const assertAccounting = () => {
    expect(recordTrackPlay).toHaveBeenCalledTimes(records);
    expect(increment).toHaveBeenCalledTimes(records);
    if (records) {
      expect(recordTrackPlay).toHaveBeenCalledWith(tracks[from].id, 44, Math.floor(elapsed));
      expect(increment).toHaveBeenCalledWith(tracks[from].id);
    }
  };
  try {
    const result = await adapter.port.execute(type === "seek" ? { type, seconds: 30 } : { type }, { hostEpoch: adapter.state.read().hostEpoch });
    expect(result.status).toBe("applied");
    expect(get(state.queueIndex)).toBe(to);
    assertAccounting();
    // Identical later samples cannot lose or duplicate the departed occurrence's account.
    cleanup = initializeSqueeze();
    await vi.advanceTimersByTimeAsync(500);
    assertAccounting();
    expect(isTimerModeTrackOrAlbumEnd()).toBe(true);
    expect(squeezePause).not.toHaveBeenCalled();
    expect(api.play).not.toHaveBeenCalled();
  } finally {
    cleanup?.(); resetSqueezePollingForTests(); stopSleepTimer(false);
    await adapter.dispose(); increment.mockRestore(); vi.useRealTimers();
  }
});
