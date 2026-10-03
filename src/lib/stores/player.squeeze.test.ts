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

    await playTrack({ id: 1, title: "Failed request", duration: 100, cover_url: "fixture" } as any);

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
      await playing;

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
