import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

const { squeezePlay } = vi.hoisted(() => ({ squeezePlay: vi.fn() }));
vi.mock("$lib/api/tauri", () => ({
  isTauri: () => false,
  squeezePlay,
  squeezeGetPlayerState: vi.fn(() => new Promise(() => {})),
  squeezeDisconnectPlayer: vi.fn().mockResolvedValue(undefined),
  getAudioSrc: vi.fn(),
  getAlbumArtSrc: vi.fn(),
  getTrackCoverSrc: vi.fn(),
  convertFileSrc: vi.fn(),
  listen: vi.fn(),
  initWindowsThumbar: vi.fn(),
  updateWindowsThumbarState: vi.fn().mockResolvedValue(undefined),
  submitListenbrainzListen: vi.fn(),
  squeezePause: vi.fn(),
  squeezeResume: vi.fn(),
  squeezeNext: vi.fn(),
  squeezePrevious: vi.fn(),
  squeezeSeek: vi.fn(),
  squeezeSetVolume: vi.fn(),
  squeezeSetShuffle: vi.fn(),
  squeezeSetRepeat: vi.fn(),
  squeezeInsertQueue: vi.fn(),
  squeezeUpdateQueue: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("$lib/stores/toast", () => ({ addToast: vi.fn() }));

import {
  pluginEvents,
  activeBackend,
  currentTrack,
  currentTime,
  duration,
  isPlaying,
  playTrack,
  playTrackOnSqueeze,
  queue,
} from "$lib/application/desktop/player-runtime";
import {
  initializeSqueeze,
  activeSqueezePlayer,
  activateSqueezeTarget,
  disconnectSqueezePlayer,
  resetSqueezePollingForTests,
} from "./squeeze";

initializeSqueeze();

describe("success-gated Squeeze play", () => {
  beforeEach(() => squeezePlay.mockReset());

  it("does not commit optimistic state when squeezePlay rejects", async () => {
    const commit = vi.fn();
    squeezePlay.mockRejectedValueOnce(new Error("offline"));

    await expect(playTrackOnSqueeze("AA", [1], 0, {
      track: { id: 1, title: "Current" } as any,
      startTime: 7,
      sessionId: 1,
      isCurrentSession: () => true,
      commit,
    })).resolves.toBe("failed");

    expect(commit).not.toHaveBeenCalled();
  });

  it("commits only after a successful request and current session check", async () => {
    const commit = vi.fn();
    squeezePlay.mockResolvedValueOnce(undefined);

    await expect(playTrackOnSqueeze("AA", [1], 0, {
      track: { id: 1, title: "Current" } as any,
      startTime: 7,
      sessionId: 1,
      isCurrentSession: () => true,
      commit,
    })).resolves.toBe("played");

    expect(commit).toHaveBeenCalledOnce();
  });
});


describe("Squeeze play request ownership", () => {
  beforeEach(() => {
    vi.stubGlobal("navigator", {});
    resetSqueezePollingForTests();
    activeSqueezePlayer.set(null);
    activeBackend.set("none");
    currentTrack.set(null);
    currentTime.set(0);
    duration.set(0);
    isPlaying.set(false);
    queue.set([]);
    squeezePlay.mockReset();
  });

  afterEach(() => {
    resetSqueezePollingForTests();
    activeSqueezePlayer.set(null);
    activeBackend.set("none");
    vi.unstubAllGlobals();
  });

  it("commits a successful play while the same target still owns playback", async () => {
    squeezePlay.mockResolvedValueOnce(undefined);
    activateSqueezeTarget("A");

    await playTrack({ id: 1, title: "Current target", duration: 100, cover_url: "fixture" } as any);

    expect(get(currentTrack)?.id).toBe(1);
    expect(get(duration)).toBe(100);
    expect(get(isPlaying)).toBe(true);
  });

  it("keeps existing playback state when the actual play request fails", async () => {
    squeezePlay.mockRejectedValueOnce(new Error("offline"));
    activateSqueezeTarget("A");
    currentTrack.set({ id: 2, title: "Existing track" } as any);
    currentTime.set(7);
    duration.set(200);
    isPlaying.set(false);

    await expect(playTrack({ id: 1, title: "Failed request", duration: 100, cover_url: "fixture" } as any)).rejects.toThrow("Squeeze playback failed");

    expect(get(currentTrack)?.id).toBe(2);
    expect(get(currentTime)).toBe(7);
    expect(get(duration)).toBe(200);
    expect(get(isPlaying)).toBe(false);
  });

  it.each(["cloud", "other player", "A -> B -> A", "Squeeze -> cloud -> Squeeze", "disconnect"])(
    "does not publish late play success after %s ownership changes",
    async (transition) => {
      let finishPlay!: () => void;
      squeezePlay.mockReturnValueOnce(new Promise<void>((resolve) => { finishPlay = resolve; }));
      activateSqueezeTarget("A");
      const playing = playTrack({ id: 1, title: "Old Squeeze request", duration: 100, cover_url: "fixture" } as any);
      await vi.waitFor(() => expect(squeezePlay).toHaveBeenCalledOnce());

      if (transition === "disconnect") {
        disconnectSqueezePlayer("A");
      } else {
        if (transition === "cloud" || transition === "Squeeze -> cloud -> Squeeze") {
          activeBackend.set("remote");
        } else {
          activateSqueezeTarget("B");
        }
        if (transition === "A -> B -> A" || transition === "Squeeze -> cloud -> Squeeze") {
          activateSqueezeTarget("A");
        }
        currentTrack.set({ id: 2, title: "New owner track" } as any);
        currentTime.set(7);
        duration.set(200);
        isPlaying.set(false);
      }

      finishPlay();
      await expect(playing).rejects.toThrow("Squeeze playback superseded");

      expect(get(currentTrack)?.id ?? null).toBe(transition === "disconnect" ? null : 2);
      expect(get(currentTime)).toBe(transition === "disconnect" ? 0 : 7);
      expect(get(duration)).toBe(transition === "disconnect" ? 0 : 200);
      expect(get(isPlaying)).toBe(false);
    },
  );
});

it("preserves the plugin event singleton across facade and desktop runtime", async () => {
  const facade = await import("./player");
  expect(facade.pluginEvents).toBe(pluginEvents);
});


it("disposes runtime subscriptions before a subsequent desktop startup", async () => {
  vi.stubGlobal("navigator", {});
  const { initAudioBackend, cleanupPlayer } = await import("$lib/application/desktop/player-runtime");
  const { updateWindowsThumbarState } = await import("$lib/api/tauri");
  await initAudioBackend();
  cleanupPlayer();
  vi.mocked(updateWindowsThumbarState).mockClear();
  activeBackend.set("remote");
  isPlaying.set(true);
  expect(updateWindowsThumbarState).not.toHaveBeenCalled();
  await initAudioBackend();
  vi.mocked(updateWindowsThumbarState).mockClear();
  isPlaying.set(false);
  expect(updateWindowsThumbarState).toHaveBeenCalledTimes(1);
  cleanupPlayer();
  vi.unstubAllGlobals();
});


it("shares pure playback helper identities between facade and runtime", async () => {
  const helper = await import("$lib/application/playback-helpers");
  const facade = await import("./player");
  const runtime = await import("$lib/application/desktop/player-runtime");
  expect(facade.isStreaming).toBe(helper.isStreaming);
  expect(runtime.isStreaming).toBe(helper.isStreaming);
  expect(facade.sliderToAudioVolume).toBe(runtime.sliderToAudioVolume);
  expect(facade.audioVolumeToSlider).toBe(runtime.audioVolumeToSlider);
});

it("does not publish a replacement queue until Squeeze acknowledges it", async () => {
  vi.stubGlobal("navigator", {});
  const { playTracks, queueIndex } = await import("$lib/application/desktop/player-runtime");
  activeBackend.set("squeeze");
  activeSqueezePlayer.set("A");
  queue.set([{ id: 9 } as any]);
  queueIndex.set(0);
  let finish!: () => void;
  squeezePlay.mockReturnValueOnce(new Promise<void>(resolve => { finish = resolve; }));
  const pending = playTracks([{ id: 1, cover_url: "fixture" } as any, { id: 2, cover_url: "fixture" } as any], 1);
  await vi.waitFor(() => expect(squeezePlay).toHaveBeenCalledWith("A", [1, 2], 1));
  expect(get(queue).map(t => t.id)).toEqual([9]);
  finish();
  await pending;
  expect(get(queue).map(t => t.id)).toEqual([1, 2]);
  expect(get(queueIndex)).toBe(1);
  activeBackend.set("none");
  activeSqueezePlayer.set(null);
  vi.unstubAllGlobals();
});

it("keeps the queue and shuffle cursor unchanged when Squeeze queue synchronization fails", async () => {
  const { playNext, queueIndex, shuffle, shuffledIndices } = await import("$lib/application/desktop/player-runtime");
  const { squeezeInsertQueue } = await import("$lib/api/tauri");
  activeBackend.set("squeeze"); activeSqueezePlayer.set("A");
  queue.set([{ id: 1 } as any, { id: 2 } as any]); queueIndex.set(0);
  shuffle.set(true); shuffledIndices.set([0, 1]);
  vi.mocked(squeezeInsertQueue).mockRejectedValueOnce(new Error("queue refused"));
  await expect(playNext([{ id: 3 } as any])).rejects.toThrow("queue refused");
  expect(get(queue).map(t => t.id)).toEqual([1, 2]);
  expect(get(shuffledIndices)).toEqual([0, 1]);
  activeBackend.set("none"); activeSqueezePlayer.set(null); shuffle.set(false);
});

it.each([
  ["pause", "squeezePause"], ["resume", "squeezeResume"],
  ["nextTrack", "squeezeNext"], ["previousTrack", "squeezePrevious"],
  ["seek", "squeezeSeek"], ["setVolume", "squeezeSetVolume"],
] as const)("propagates %s backend rejection without a success-shaped return", async (method, apiMethod) => {
  const runtime = await import("$lib/application/desktop/player-runtime");
  const api = await import("$lib/api/tauri");
  const { squeezePlayerState } = await import("./squeeze");
  activeBackend.set("squeeze"); activeSqueezePlayer.set("A");
  squeezePlayerState.set({ current_track: { id: 1 } } as any);
  vi.mocked(api[apiMethod]).mockRejectedValueOnce(new Error("acknowledgement failed"));
  await expect((runtime[method] as (value?: number) => Promise<void>)(0.5)).rejects.toThrow("acknowledgement failed");
  activeBackend.set("none"); activeSqueezePlayer.set(null);
});

it("does not commit an acknowledged queue edit after its Squeeze owner changed", async () => {
  const { playNext } = await import("$lib/application/desktop/player-runtime");
  const { squeezeInsertQueue } = await import("$lib/api/tauri");
  activeBackend.set("squeeze"); activeSqueezePlayer.set("A");
  queue.set([{ id: 1 } as any]);
  let finish!: () => void;
  vi.mocked(squeezeInsertQueue).mockReturnValueOnce(new Promise<void>(resolve => { finish = resolve; }));
  const editing = playNext([{ id: 2 } as any]);
  activeBackend.set("remote");
  finish();
  await expect(editing).rejects.toMatchObject({ status: "superseded" });
  expect(get(queue).map(t => t.id)).toEqual([1]);
  activeBackend.set("none"); activeSqueezePlayer.set(null);
});

it.each([true, false])("rejects removing the active Squeeze occurrence before effects (playing=%s)", async playing => {
  const { removeFromQueue, queueIndex } = await import("$lib/application/desktop/player-runtime");
  const { squeezeUpdateQueue } = await import("$lib/api/tauri");
  vi.mocked(squeezeUpdateQueue).mockClear();
  const tracks = [{ id: 7 } as any, { id: 7 } as any];
  activeBackend.set("squeeze"); activeSqueezePlayer.set("A");
  queue.set(tracks); queueIndex.set(1); currentTrack.set(tracks[1]); isPlaying.set(playing);
  await expect(removeFromQueue(1)).rejects.toMatchObject({ controlError: { code: "unsupported" } });
  expect(get(queue)).toBe(tracks);
  expect(get(queueIndex)).toBe(1);
  expect(squeezeUpdateQueue).not.toHaveBeenCalled();
  activeBackend.set("none"); activeSqueezePlayer.set(null);
});

it("sends exact source indices for repeated-entry reorder and removal", async () => {
  const { reorderQueue, removeFromQueue, clearUpcoming, queueIndex } = await import("$lib/application/desktop/player-runtime");
  const { squeezeUpdateQueue } = await import("$lib/api/tauri");
  const repeated = { id: 7 } as any;
  activeBackend.set("squeeze"); activeSqueezePlayer.set("A");
  queue.set([repeated, repeated, repeated]); queueIndex.set(2); currentTrack.set(repeated);
  await reorderQueue(2, 0);
  expect(squeezeUpdateQueue).toHaveBeenLastCalledWith("A", [7, 7, 7], 7, 0, [2, 0, 1]);
  await removeFromQueue(2);
  expect(squeezeUpdateQueue).toHaveBeenLastCalledWith("A", [7, 7], 7, 0, [0, 1]);
  await clearUpcoming();
  expect(squeezeUpdateQueue).toHaveBeenLastCalledWith("A", [7], 7, 0, [0]);
  activeBackend.set("none"); activeSqueezePlayer.set(null);
});

it("reports buffered-occurrence removal as retryable unsupported without committing the plan", async () => {
  const { removeFromQueue, queueIndex } = await import("$lib/application/desktop/player-runtime");
  const { squeezeUpdateQueue } = await import("$lib/api/tauri");
  const original = [{ id: 7 } as any, { id: 7 } as any];
  activeBackend.set("squeeze"); activeSqueezePlayer.set("A");
  queue.set(original); queueIndex.set(0); currentTrack.set(original[0]);
  vi.mocked(squeezeUpdateQueue).mockRejectedValueOnce("SQUEEZE_QUEUE_BUSY: buffered occurrence cannot be removed");
  await expect(removeFromQueue(1)).rejects.toMatchObject({ controlError: { code: "unsupported", retryable: true }, partialEffects: [] });
  expect(get(queue)).toBe(original);
  activeBackend.set("none"); activeSqueezePlayer.set(null);
});
