import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

const { squeezeGetPlayerState, squeezePlay, getTrackById, recordTrackPlay } = vi.hoisted(() => ({
  squeezeGetPlayerState: vi.fn(),
  squeezePlay: vi.fn(),
  getTrackById: vi.fn(),
  recordTrackPlay: vi.fn(),
}));

vi.mock("$lib/api/tauri", () => ({
  squeezeGetPlayerState,
  squeezeGetPlayers: vi.fn().mockResolvedValue([]),
  squeezeStop: vi.fn().mockResolvedValue(undefined),
  squeezeStartServer: vi.fn().mockResolvedValue(undefined),
  squeezeIsRunning: vi.fn().mockResolvedValue(true),
  squeezePlay,
  getTrackCoverSrc: (track: any) => track.cover_url ?? "",
  getTrackById,
}));

const {
  currentTrack,
  isPlaying,
  currentTime,
  duration,
  volume,
  activeBackend,
  shuffle,
  repeat,
  queue,
  queueIndex,
  activeRemoteDevice,
} = vi.hoisted(() => {
  const store = <T>(initial: T) => {
    let value = initial;
    const subscribers = new Set<(value: T) => void>();
    return {
      subscribe(run: (value: T) => void) {
        subscribers.add(run);
        run(value);
        return () => subscribers.delete(run);
      },
      set(next: T) {
        value = next;
        subscribers.forEach((run) => run(value));
      },
      update(fn: (value: T) => T) {
        value = fn(value);
        subscribers.forEach((run) => run(value));
      },
    };
  };
  return {
    currentTrack: store<any>(null),
    isPlaying: store(false),
    currentTime: store(0),
    duration: store(0),
    volume: store(0),
    activeBackend: store("squeeze"),
    shuffle: store(false),
    repeat: store<"none" | "one" | "all">("none"),
    queue: store<any[]>([]),
    queueIndex: store(0),
    activeRemoteDevice: store<string | null>(null),
  };
});

vi.mock("$lib/stores/player", () => ({
  currentTrack,
  isPlaying,
  currentTime,
  duration,
  volume,
  activeBackend,
  shuffle,
  repeat,
  queue,
  queueIndex,
}));
vi.mock("$lib/stores/websocket", () => ({ activeRemoteDevice }));
vi.mock("$lib/stores/library", () => ({
  getTrackByIdSync: vi.fn(() => null),
  incrementPlayCount: vi.fn(),
  cacheTrack: vi.fn(),
}));
vi.mock("$lib/stores/activity", () => ({ recordTrackPlay }));

import {
  activeSqueezePlayer,
  activateSqueezeTarget,
  playHereOnSqueeze,
  resetSqueezePollingForTests,
  squeezePlayerState,
} from "./squeeze";

const info = (mac: string, id: number) => ({
  mac,
  name: mac,
  state: "Playing",
  capabilities: "",
  current_track: { id, title: `Track ${id}`, artist: "Artist", album: "Album", path: `/track-${id}.mp3`, duration: 100 },
  elapsed_ms: 10,
  volume: 50,
  repeat: "Off",
  shuffle: false,
  queue_length: 1,
  queue_position: 0,
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

describe("Squeeze poll ownership", () => {
  beforeEach(() => {
    vi.useRealTimers();
    resetSqueezePollingForTests();
    activeSqueezePlayer.set(null);
    activeBackend.set("squeeze");
    squeezePlayerState.set(null);
    currentTrack.set(null);
    isPlaying.set(false);
    currentTime.set(0);
    duration.set(0);
    squeezeGetPlayerState.mockReset();
    getTrackById.mockReset();
  });

  it("discards a poll after switching players, including A -> B -> A reuse", async () => {
    const requests = [deferred<any>(), deferred<any>(), deferred<any>()];
    const pending = [...requests];
    squeezeGetPlayerState.mockImplementation(() => pending.shift()!.promise);

    activeSqueezePlayer.set("A");
    activeSqueezePlayer.set("B");
    activeSqueezePlayer.set("A");

    requests[1].resolve(info("B", 2));
    requests[2].resolve(info("A", 3));
    await Promise.resolve();
    await Promise.resolve();
    requests[0].resolve(info("A", 1));
    await Promise.resolve();
    await Promise.resolve();

    expect(get(activeSqueezePlayer)).toBe("A");
    expect(get(squeezePlayerState)?.current_track?.id).toBe(3);
  });

  it("does not publish metadata fetched after backend ownership changes", async () => {
    const state = deferred<any>();
    const metadata = deferred<any>();
    squeezeGetPlayerState.mockReturnValueOnce(state.promise);
    getTrackById.mockReturnValueOnce(metadata.promise);

    activeSqueezePlayer.set("A");
    state.resolve(info("A", 42));
    await Promise.resolve();
    await Promise.resolve();
    activeBackend.set("none");
    metadata.resolve({ id: 42, title: "stale metadata", path: "/stale.mp3" });
    await Promise.resolve();
    await Promise.resolve();

    expect(get(currentTrack)).toBeNull();
  });

  it("activates Play Here only after squeezePlay succeeds", async () => {
    activeSqueezePlayer.set("old");
    activeBackend.set("remote");
    activeRemoteDevice.set("phone");
    squeezePlay.mockResolvedValueOnce(undefined);

    await playHereOnSqueeze("new", [1], 0);

    expect(get(activeSqueezePlayer)).toBe("new");
    expect(get(activeBackend)).toBe("squeeze");
    expect(get(activeRemoteDevice)).toBeNull();
  });

  it("preserves activation when Play Here fails", async () => {
    activeSqueezePlayer.set("old");
    activeBackend.set("remote");
    activeRemoteDevice.set("phone");
    squeezePlay.mockRejectedValueOnce(new Error("offline"));

    await expect(playHereOnSqueeze("new", [1], 0)).rejects.toThrow("offline");

    expect(get(activeSqueezePlayer)).toBe("old");
    expect(get(activeBackend)).toBe("remote");
    expect(get(activeRemoteDevice)).toBe("phone");
  });
});
