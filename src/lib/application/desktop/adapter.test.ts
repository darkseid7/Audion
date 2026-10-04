vi.mock("$lib/stores/activity", () => ({ recordTrackPlay: vi.fn() }));
import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
const api = vi.hoisted(() => ({ play: vi.fn(), stop: vi.fn(), tracks: vi.fn(), players: vi.fn() }));
vi.mock("$lib/api/tauri", async importOriginal => ({
  ...await importOriginal<object>(),
  getPlaylistTracks: vi.fn(), getTracksByArtist: vi.fn(), getLikedTracks: vi.fn(), squeezeNext: vi.fn().mockResolvedValue(undefined), squeezePrevious: vi.fn().mockResolvedValue(undefined), squeezeSeek: vi.fn().mockResolvedValue(undefined), getLikedTrackIds: vi.fn().mockResolvedValue([7]), squeezeGetPlayerState: vi.fn(), squeezeUpdateQueue: vi.fn().mockResolvedValue(undefined), squeezePause: vi.fn().mockResolvedValue(undefined), squeezePlay: api.play, squeezeStop: api.stop, squeezeDisconnectPlayer: vi.fn().mockResolvedValue(undefined), getTracksByAlbum: api.tracks,
  squeezeGetPlayers: api.players, getTrackById: async (id: number) => ({ id, title: `Track ${id}`, duration: 100, cover_url: "fixture" }),
}));
import { createDesktopAdapter } from "./adapter";
import { playbackStateWriter as state } from "$lib/stores/playback-state";
import { activeSqueezePlayer, commitSqueezeTarget } from "$lib/stores/squeeze";

beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal("navigator", {});
  state.activeBackend.set("none"); activeSqueezePlayer.set(null);
  state.queue.set([]); state.queueIndex.set(0); state.currentTrack.set(null);
  state.isPlaying.set(false); state.shuffle.set(false);
  api.play.mockReset().mockResolvedValue(undefined); api.stop.mockReset().mockResolvedValue(undefined);
  api.tracks.mockReset().mockResolvedValue([{ id: 7, duration: 100, cover_url: "fixture" }, { id: 7, duration: 100, cover_url: "fixture" }]);
  api.players.mockReset().mockResolvedValue([{ mac: "A", name: "Living room" }]);
});
describe("desktop adapter execution", () => {
  it("attaches native identity throughout snapshots, preconditions and regenerated queue entry IDs", async () => {
    commitSqueezeTarget("A");
    const adapter = createDesktopAdapter();
    const oldEpoch = adapter.state.read().hostEpoch;
    await adapter.port.execute({ type: "play_album", albumId: 1, playMode: "all" }, { hostEpoch: oldEpoch });
    const previousId = adapter.state.read().queue[0].entryId;
    await adapter.attachAuthority({ hostId: "native-host", hostEpoch: "native-epoch" });
    const snapshot = await adapter.port.query({ type: "snapshot" });
    expect(snapshot).toMatchObject({ snapshot: { hostId: "native-host", hostEpoch: "native-epoch" } });
    expect(adapter.state.read().hostEpoch).toBe("native-epoch");
    expect(adapter.state.read().queue[0].entryId).not.toBe(previousId);
    expect(adapter.state.read().queue[0].entryId).toMatch(/^native-epoch:/);
    expect(await adapter.port.execute({ type: "pause" }, { hostEpoch: oldEpoch })).toMatchObject({ status: "superseded", error: { code: "resync_required" } });
    expect(await adapter.port.execute({ type: "pause" }, { hostEpoch: "native-epoch" })).toMatchObject({ status: "applied" });
    await adapter.dispose();
  });
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

it("observes PC progress, output discovery and library invalidation through the real coordinator projection", async () => {
  const adapter = createDesktopAdapter();
  const seen: import("../types").HostSnapshot[] = [];
  const unsubscribe = adapter.coordinator.subscribeSnapshot(s => seen.push(s));
  const queueRevision = adapter.state.read().revisions.queueRevision;
  state.currentTime.set(42);
  await new Promise(resolve => setTimeout(resolve, 0));
  expect(seen.at(-1)?.playback.position).toBe(42);
  expect(seen.at(-1)?.revisions.queueRevision).toBe(queueRevision);
  const { discoveredSqueezePlayers } = await import("$lib/stores/squeeze");
  const outputRevision = adapter.state.read().revisions.outputRevision;
  discoveredSqueezePlayers.set([{ mac: "A", name: "Living room", state: "Stopped", capabilities: "", current_track: null, elapsed_ms: 0, volume: 50, repeat: "Off", shuffle: false, queue_length: 0, queue_position: null }]);
  await new Promise(resolve => setTimeout(resolve, 0));
  expect(seen.at(-1)?.outputs.some(item => item.output.kind === "squeeze" && item.output.playerId === "A")).toBe(true);
  expect(seen.at(-1)?.revisions.outputRevision).toBe(outputRevision + 1);
  discoveredSqueezePlayers.update(devices => devices.map(device => ({ ...device, elapsed_ms: 5000 })));
  await new Promise(resolve => setTimeout(resolve, 0));
  expect(adapter.state.read().revisions.outputRevision).toBe(outputRevision + 1);
  const before = adapter.state.read().revisions.libraryRevision;
  const { tracks } = await import("$lib/stores/library");
  tracks.set([]);
  await new Promise(resolve => setTimeout(resolve, 0));
  expect(seen.at(-1)?.revisions.libraryRevision).toBe(before);
  expect(await adapter.port.query({ type: "outputs" })).toMatchObject({ revision: outputRevision + 1 });
  const projection = adapter.coordinator.captureSnapshot();
  expect(projection.capabilities.queries).toEqual(["snapshot", "outputs"]);
  unsubscribe(); await adapter.dispose();
  const count = seen.length; state.currentTime.set(43);
  await new Promise(resolve => setTimeout(resolve, 0)); expect(seen).toHaveLength(count);
});

it("adopts the native library clock and rejects writes during entity resolution before playback", async () => {
  commitSqueezeTarget("A");
  const adapter = createDesktopAdapter();
  let revision = 7;
  const library = { revision: vi.fn(async () => revision), query: vi.fn(), artwork: vi.fn() };
  await adapter.attachAuthority({ hostId: "host", hostEpoch: "epoch", library });
  api.tracks.mockImplementationOnce(async () => { revision = 8; return [{ id: 7, duration: 100, cover_url: "fixture" }]; });
  const result = await adapter.port.execute({ type: "play_album", albumId: 1, playMode: "all" }, { hostEpoch: "epoch", libraryRevision: 7 });
  expect(result).toMatchObject({ status: "superseded", error: { code: "revision_conflict" } });
  expect(api.play).not.toHaveBeenCalled();
  expect(adapter.state.read().revisions.libraryRevision).toBe(8);
  await adapter.dispose();
});

it("detached hosting never prevents ordinary desktop playback", async () => {
  commitSqueezeTarget("A");
  const adapter = createDesktopAdapter(); let active = true;
  const revision = vi.fn(async () => { if (!active) throw new Error("Released lease"); return 1; });
  await adapter.attachAuthority({ hostId: "host", hostEpoch: "epoch", library: { active: () => active, revision, query: vi.fn(), artwork: vi.fn() } });
  active = false;
  const result = await adapter.port.execute({ type: "pause" }, { hostEpoch: "epoch" });
  expect(result.status).toBe("applied");
  expect((await adapter.port.execute({ type: "play_album", albumId: 1, playMode: "all" }, { hostEpoch: "epoch" })).status).toBe("applied");
  expect(revision).toHaveBeenCalledTimes(1);
  await adapter.dispose();
});


it("keeps local native library and artwork available before hosting and after detach", async () => {
  const local = { revision: vi.fn().mockResolvedValue(4), query: vi.fn().mockResolvedValue({ type: "tracks", page: { items: [], revision: 4, nextCursor: null } }), artwork: vi.fn().mockResolvedValue({ src: "blob:local", dispose: vi.fn() }) };
  const adapter = createDesktopAdapter(local);
  expect(await adapter.port.query({ type: "tracks" })).toMatchObject({ type: "tracks", page: { revision: 4 } });
  expect(adapter.state.read().revisions.libraryRevision).toBe(4);
  expect(await adapter.port.resolveArtwork({ resourceId: "local", revision: 4 })).toMatchObject({ src: "blob:local" });
  commitSqueezeTarget("A");
  expect(await adapter.port.execute({ type: "play_album", albumId: 1, playMode: "all" }, { hostEpoch: adapter.state.read().hostEpoch })).toMatchObject({ status: "applied" });
  let active = true;
  await adapter.attachAuthority({ hostId: "native", hostEpoch: "lan", library: { ...local, active: () => active } });
  active = false;
  expect(await adapter.port.query({ type: "tracks" })).toMatchObject({ type: "tracks" });
  expect(await adapter.port.resolveArtwork({ resourceId: "local", revision: 4 })).toMatchObject({ src: "blob:local" });
  expect(await adapter.port.execute({ type: "pause" }, { hostEpoch: adapter.state.read().hostEpoch })).toMatchObject({ status: "applied" });
  expect(await adapter.port.execute({ type: "play_album", albumId: 1, playMode: "all" }, { hostEpoch: adapter.state.read().hostEpoch })).toMatchObject({ status: "applied" });
  expect(local.revision).toHaveBeenCalled();
  await adapter.dispose();
});

it("rejects late local library and artwork results after LAN attach", async () => {
  let finish!: (result: unknown) => void;
  let finishArt!: (result: unknown) => void;
  const local = { revision: async () => 2, query: () => new Promise<any>(resolve => finish = resolve), artwork: () => new Promise<any>(resolve => finishArt = resolve) };
  const adapter = createDesktopAdapter(local);
  const pending = adapter.port.query({ type: "tracks" });
  const pendingArt = adapter.port.resolveArtwork({ resourceId: "old", revision: 2 });
  await adapter.attachAuthority({ hostId: "native", hostEpoch: "new", library: { revision: async () => 7, query: local.query, artwork: local.artwork } });
  finish({ type: "tracks", page: { items: [], revision: 2, nextCursor: null } });
  const dispose = vi.fn();finishArt({ src: "blob:old", dispose });
  await expect(pending).rejects.toMatchObject({ controlError: { code: "resync_required" } });
  await expect(pendingArt).rejects.toMatchObject({ controlError: { code: "resync_required" } });
  expect(dispose).toHaveBeenCalledOnce();expect(adapter.state.read().revisions.libraryRevision).toBe(7);
  await adapter.dispose();
});

it("pages local queue occurrences and rejects a cursor after queue mutation", async () => {
  state.queue.set(Array.from({ length: 205 }, (_, id) => ({ id: id + 1, title: "Track", path: `synthetic-${id}`, artist: null, album: null, track_number: null, duration: null, album_id: null, format: null, bitrate: null })));
  const adapter = createDesktopAdapter();
  const first = await adapter.port.query({ type: "queue", limit: 200 });
  expect(first).toMatchObject({ type: "queue", page: { items: expect.any(Array) } });
  if (first.type !== "queue") throw new Error("Wrong queue result");expect(first.page.items).toHaveLength(200);
  const second = await adapter.port.query({ type: "queue", limit: 200, cursor: first.page.nextCursor! });
  if (second.type !== "queue") throw new Error("Wrong queue result");expect(second.page.items).toHaveLength(5);
  state.queue.set([]);adapter.state.commit({});
  await expect(adapter.port.query({ type: "queue", limit: 200, cursor: first.page.nextCursor! })).rejects.toMatchObject({ controlError: { code: "revision_conflict" } });
  await adapter.dispose();
});


it("invalidates queued old commands before awaiting attachment revision", async () => {
  const adapter = createDesktopAdapter();const oldEpoch=adapter.state.read().hostEpoch;
  let release!:()=>void;const held=adapter.coordinator.executeLocal(()=>new Promise(resolve=>{release=()=>resolve({status:"applied"});}));
  await Promise.resolve();const queued=adapter.port.execute({type:"pause"},{hostEpoch:oldEpoch});
  let finish!:(revision:number)=>void;
  const attachment=adapter.attachAuthority({hostId:"native",hostEpoch:"new-epoch",library:{revision:()=>new Promise(resolve=>finish=resolve),query:async()=>{throw new Error("unused")},artwork:async()=>{throw new Error("unused")}}});
  const synchronousEpoch=adapter.state.read().hostEpoch;release();await held;
  const result=await queued;finish(0);await attachment;
  expect(synchronousEpoch).toBe("new-epoch");expect(result).toMatchObject({status:"superseded",error:{code:"resync_required"}});await adapter.dispose();
});

it("keeps bound local playback coherent after attachment revision rejection", async () => {
  const adapter=createDesktopAdapter();
  await expect(adapter.attachAuthority({hostId:"native",hostEpoch:"failed-epoch",library:{revision:async()=>{throw new Error("Busy")},query:async()=>{throw new Error("unused")},artwork:async()=>{throw new Error("unused")}}})).rejects.toThrow("Busy");
  const player=await import("$lib/stores/player");await expect(player.pause()).resolves.toBeUndefined();
  expect((await adapter.port.query({type:"snapshot"}))).toMatchObject({snapshot:{hostEpoch:adapter.state.read().hostEpoch}});await adapter.dispose();
});

it("refreshes actual registered local entity commands without weakening remote assertions", async () => {
  const player=await import("$lib/stores/player");const registered=vi.spyOn(player,"registerDesktopPlayer");let revision=4;
  const access={revision:async()=>revision,query:async()=>({type:"tracks" as const,page:{items:[],revision,nextCursor:null}}),artwork:async()=>{throw new Error("unused")}};
  commitSqueezeTarget("A");const adapter=createDesktopAdapter(access);const local=registered.mock.calls[0][0];
  await adapter.port.query({type:"tracks"});revision=5;
  await expect(local.execute({type:"play_album",albumId:1,playMode:"all"})).resolves.toBeUndefined();expect(api.play).toHaveBeenCalledOnce();
  api.play.mockClear();revision=6;
  expect(await adapter.port.execute({type:"play_album",albumId:1,playMode:"all"},{hostEpoch:adapter.state.read().hostEpoch,libraryRevision:5})).toMatchObject({status:"superseded",error:{code:"revision_conflict"}});
  expect(api.play).not.toHaveBeenCalled();await adapter.dispose();registered.mockRestore();
});


it("keeps the real publisher and bridge lease during busy revision observation and catches up", async () => {
  vi.useFakeTimers();const { connectHostBridge }=await import("./bridge");
  let busy=false;let revision=0;let replay=0;const phases:string[]=[];const snapshots:import("../types").HostSnapshot[]=[];
  const adapter=createDesktopAdapter();
  const lease={leaseId:"same-lease",hostEpoch:"same-epoch"};
  const bridge=await connectHostBridge(adapter.port,adapter.attachAuthority,adapter.coordinator,{
    library:()=>({revision:async()=>{if(busy)throw {code:"busy"};return revision;},query:async()=>{throw new Error("unused")},artwork:async()=>{throw new Error("unused")}}),
    async listen(){return()=>{};},
    async register(request){phases.push(request.phase);if(request.phase==="publish"){snapshots.push(request.update.snapshot);replay++;}return {hostId:"host",lease,revision:replay};},
    async authorize(){throw new Error("unused")},async complete(){},
  });
  try {
    busy=true;adapter.state.commit({});state.currentTime.set(42);
    await vi.advanceTimersByTimeAsync(300);expect(phases).not.toContain("release");expect(snapshots.at(-1)?.hostEpoch).toBe(lease.hostEpoch);
    busy=false;revision=1;await vi.advanceTimersByTimeAsync(300);
    expect(adapter.state.read().revisions.libraryRevision).toBe(1);expect(snapshots.at(-1)?.revisions.libraryRevision).toBe(1);expect(phases.filter(p=>p==="ready")).toHaveLength(1);
  } finally {await bridge.dispose();await adapter.dispose();vi.useRealTimers();}
});


it("does not regress a revision advanced by earlier lane work during attachment", async () => {
  const adapter=createDesktopAdapter();let finish!:()=>void;
  const earlier=adapter.coordinator.executeLocal(async()=>{await new Promise<void>(resolve=>finish=resolve);adapter.state.commit({revisions:{libraryRevision:7}});return {status:"applied"};});
  await Promise.resolve();
  const attaching=adapter.attachAuthority({hostId:"native",hostEpoch:"pending",library:{revision:async()=>5,query:async()=>{throw new Error("unused")},artwork:async()=>{throw new Error("unused")}}});
  await Promise.resolve();await Promise.resolve();finish();await earlier;
  await expect(attaching).rejects.toThrow("Invalid native library revision");expect(adapter.state.read().revisions.libraryRevision).toBe(7);await adapter.dispose();
});


it.each(["queue_insert", "queue_append"] as const)("refreshes bound local %s on the first action while remote stale assertions fail", async type => {
  const player=await import("$lib/stores/player");const registered=vi.spyOn(player,"registerDesktopPlayer");let revision=1;
  const adapter=createDesktopAdapter({revision:async()=>revision,query:async()=>({type:"tracks",page:{items:[],revision,nextCursor:null}}),artwork:async()=>{throw new Error("unused")}});
  const local=registered.mock.calls[0][0];await adapter.port.query({type:"tracks"});revision=2;
  const intent=type==="queue_insert"?{type,trackIds:[7],placement:"next" as const}:{type,trackIds:[7]};
  await expect(local.execute(intent)).resolves.toBeUndefined();expect(adapter.state.read().queue.map(e=>e.track.id)).toEqual([7]);
  revision=3;expect(await adapter.port.execute(intent,{hostEpoch:adapter.state.read().hostEpoch,libraryRevision:2})).toMatchObject({status:"superseded",error:{code:"revision_conflict"}});expect(adapter.state.read().queue).toHaveLength(1);await adapter.dispose();registered.mockRestore();
});

it("projects a safe PC-provided name and explicit identity fallback", async () => {
 const named = createDesktopAdapter(undefined, "  Studio PC  ");
 expect(await named.port.query({ type: "outputs" })).toMatchObject({ outputs: expect.arrayContaining([expect.objectContaining({ name: "Studio PC", output: { kind: "pc" } })]) }); await named.dispose();
 const fallback = createDesktopAdapter(undefined, "\u0000\n"); await fallback.attachAuthority({ hostId: "native-host", hostEpoch: "epoch" });
 expect(await fallback.port.query({ type: "outputs" })).toMatchObject({ outputs: expect.arrayContaining([expect.objectContaining({ name: "PC · native-host" })]) }); await fallback.dispose();
});

it("resolves an over-200-track album once on the PC for one queue operation", async () => {
 const tracks=Array.from({length:201},(_,i)=>({id:i+1,title:`Track ${i+1}`,duration:100,cover_url:"fixture"})); api.tracks.mockResolvedValue(tracks);
 const adapter=createDesktopAdapter(); const before=adapter.state.read();
 const result=await adapter.port.execute({type:"queue_entity",entity:{type:"album",albumId:42,playMode:"all"},placement:"end"} as any,{hostEpoch:before.hostEpoch,...before.revisions});
 expect(result.status).toBe("applied"); expect(api.tracks).toHaveBeenCalledExactlyOnceWith(42); expect(get(state.queue).map(t=>t.id)).toEqual(Array.from({length:201},(_,i)=>i+1)); await adapter.dispose();
});

it.each(["playlist","artist","liked"] as const)("queues a complete >200 %s entity with duplicate occurrences on PC",async type=>{
 const apiModule=await import("$lib/api/tauri");const tracks=Array.from({length:201},()=>({id:7,title:"Duplicate",path:"fixture",duration:100}));
 const resolver=type==="playlist"?apiModule.getPlaylistTracks:type==="artist"?apiModule.getTracksByArtist:apiModule.getLikedTracks;vi.mocked(resolver).mockResolvedValue(tracks as any);
 const entity=type==="playlist"?{type,playlistId:9}:type==="artist"?{type,artistName:"Ada"}:{type};const adapter=createDesktopAdapter();
 expect(await adapter.port.execute({type:"queue_entity",entity,placement:"end"},{hostEpoch:adapter.state.read().hostEpoch,...adapter.state.read().revisions})).toMatchObject({status:"applied"});
 expect(resolver).toHaveBeenCalledOnce();expect(get(state.queue)).toHaveLength(201);expect(adapter.state.read().queue.map(e=>e.entryId).filter((id,i,all)=>all.indexOf(id)===i)).toHaveLength(201);await adapter.dispose();
});
it("filters liked-only album enqueue on PC without losing duplicate occurrences",async()=>{
 api.tracks.mockResolvedValue([{id:7,duration:100},{id:8,duration:100},{id:7,duration:100}]);const adapter=createDesktopAdapter();
 await adapter.port.execute({type:"queue_entity",entity:{type:"album",albumId:42,playMode:"liked_only"},placement:"end"},{hostEpoch:adapter.state.read().hostEpoch,...adapter.state.read().revisions});
 expect(get(state.queue).map(t=>t.id)).toEqual([7,7]);await adapter.dispose();
});

it("reports a confirmed Squeeze seek stop without pretending restart succeeded",async()=>{
 const native=await import("$lib/api/tauri");vi.mocked(native.squeezeSeek).mockRejectedValueOnce("SQUEEZE_SEEK_PARTIAL: output stopped; restart failed");
 commitSqueezeTarget("A");state.duration.set(100);const adapter=createDesktopAdapter();
 expect(await adapter.port.execute({type:"seek",seconds:25},{hostEpoch:adapter.state.read().hostEpoch,...adapter.state.read().revisions})).toMatchObject({status:"failed",partialEffects:["Previous output stopped"]});await adapter.dispose();
});

it("host shuffle chooses a nonzero initial member while explicit start wins",async()=>{
 const random=vi.spyOn(Math,"random").mockReturnValue(.9);
 api.tracks.mockResolvedValue([7,8,9].map(id=>({id,duration:100,cover_url:"fixture"})));
 commitSqueezeTarget("A");state.shuffle.set(true);const adapter=createDesktopAdapter();
 try {
  await adapter.port.execute({type:"play_album",albumId:42,playMode:"all"},{hostEpoch:adapter.state.read().hostEpoch});
  expect(api.play).toHaveBeenLastCalledWith("A",[7,8,9],2);
  await adapter.port.execute({type:"play_album",albumId:42,playMode:"all",startTrackId:8},{hostEpoch:adapter.state.read().hostEpoch});
  expect(api.play).toHaveBeenLastCalledWith("A",[7,8,9],1);
 } finally { await adapter.dispose();random.mockRestore(); }
});
