import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

const {
  squeezeGetPlayerState,
  squeezePlay,
  getTrackById,
  recordTrackPlay,
  squeezeGetPlayers,
  squeezeIsRunning,
  squeezeStartServer,
  squeezeDisconnectPlayer,
} = vi.hoisted(() => ({
  squeezeGetPlayerState: vi.fn(),
  squeezePlay: vi.fn(),
  getTrackById: vi.fn(),
  recordTrackPlay: vi.fn(),
  squeezeGetPlayers: vi.fn(),
  squeezeIsRunning: vi.fn(),
  squeezeStartServer: vi.fn(),
  squeezeDisconnectPlayer: vi.fn(),
}));

vi.mock("$lib/api/tauri", () => ({
  squeezeGetPlayerState,
  squeezeGetPlayers,
  squeezeStop: vi.fn().mockResolvedValue(undefined),
  squeezeStartServer,
  squeezeIsRunning,
  squeezeDisconnectPlayer,
  squeezePlay,
  getTrackCoverSrc: (track: { cover_url?: string }) => track.cover_url ?? "",
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
  disconnectSqueezePlayer,
  discoveredSqueezePlayers,
  pollSqueezePlayersOnce,
  resetSqueezePollingForTests,
  squeezePlayerState,
  startGlobalSqueezeDiscovery,
  stopGlobalSqueezeDiscovery,
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
    squeezeGetPlayers.mockReset();
    squeezeIsRunning.mockReset();
    squeezeStartServer.mockReset();
    squeezeDisconnectPlayer.mockReset();
    squeezeGetPlayers.mockResolvedValue([]);
    squeezeIsRunning.mockResolvedValue(true);
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

});

describe("Squeeze connection lifecycle", () => {
  beforeEach(() => {
    vi.useRealTimers();
    resetSqueezePollingForTests();
    activeSqueezePlayer.set(null);
    activeBackend.set("none");
    squeezePlayerState.set(null);
    currentTrack.set(null);
    isPlaying.set(false);
    currentTime.set(0);
    duration.set(0);
    squeezeGetPlayerState.mockReset();
    getTrackById.mockReset();
    squeezeGetPlayers.mockReset();
    squeezeIsRunning.mockReset();
    squeezeStartServer.mockReset();
    squeezeDisconnectPlayer.mockReset();
    squeezeGetPlayers.mockResolvedValue([]);
    squeezeIsRunning.mockResolvedValue(true);
    squeezeStartServer.mockResolvedValue(undefined);
    squeezeDisconnectPlayer.mockResolvedValue(undefined);
  });

  afterEach(() => {
    stopGlobalSqueezeDiscovery();
    resetSqueezePollingForTests();
    activeSqueezePlayer.set(null);
    vi.useRealTimers();
  });

  it("does not restart discovery after stop while the server check is pending", async () => {
    vi.useFakeTimers();
    const running = deferred<boolean>();
    squeezeIsRunning.mockReturnValueOnce(running.promise);
    const starting = startGlobalSqueezeDiscovery();

    stopGlobalSqueezeDiscovery();
    running.resolve(true);
    await starting;
    await vi.advanceTimersByTimeAsync(2000);

    expect(get(discoveredSqueezePlayers)).toEqual([]);
    expect(squeezeGetPlayers).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("does not publish or resume discovery after stop during its first player request", async () => {
    vi.useFakeTimers();
    const players = deferred<any[]>();
    squeezeGetPlayers.mockReturnValueOnce(players.promise);
    const starting = startGlobalSqueezeDiscovery();
    await Promise.resolve();
    await Promise.resolve();

    stopGlobalSqueezeDiscovery();
    players.resolve([info("A", 1)]);
    await starting;
    await vi.advanceTimersByTimeAsync(2000);

    expect(get(discoveredSqueezePlayers)).toEqual([]);
    expect(squeezeGetPlayers).toHaveBeenCalledOnce();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("discards a pending discovery tick after stopping", async () => {
    vi.useFakeTimers();
    await startGlobalSqueezeDiscovery();
    const players = deferred<any[]>();
    squeezeGetPlayers.mockReturnValueOnce(players.promise);
    await vi.advanceTimersByTimeAsync(1000);

    stopGlobalSqueezeDiscovery();
    players.resolve([info("A", 1)]);
    await Promise.resolve();
    await Promise.resolve();

    expect(get(discoveredSqueezePlayers)).toEqual([]);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("discards an old discovery tick after stopping and restarting", async () => {
    vi.useFakeTimers();
    await startGlobalSqueezeDiscovery();
    const oldPlayers = deferred<any[]>();
    squeezeGetPlayers.mockReturnValueOnce(oldPlayers.promise);
    await vi.advanceTimersByTimeAsync(1000);
    stopGlobalSqueezeDiscovery();

    squeezeGetPlayers.mockResolvedValue([info("B", 2)]);
    await startGlobalSqueezeDiscovery();
    oldPlayers.resolve([info("A", 1)]);
    await Promise.resolve();
    await Promise.resolve();

    expect(get(discoveredSqueezePlayers).map((player) => player.mac)).toEqual(["B"]);
    expect(vi.getTimerCount()).toBe(1);
  });

  it("allows a fresh discovery start while a cancelled start is still pending", async () => {
    vi.useFakeTimers();
    const oldRunning = deferred<boolean>();
    squeezeIsRunning.mockReturnValueOnce(oldRunning.promise);
    const oldStarting = startGlobalSqueezeDiscovery();
    stopGlobalSqueezeDiscovery();

    squeezeGetPlayers.mockResolvedValue([info("B", 2)]);
    await startGlobalSqueezeDiscovery();
    const freshPlayers = get(discoveredSqueezePlayers).map((player) => player.mac);

    oldRunning.resolve(true);
    await oldStarting;
    expect(freshPlayers).toEqual(["B"]);
    expect(squeezeGetPlayers).toHaveBeenCalledOnce();
    expect(vi.getTimerCount()).toBe(1);
  });

  it("clears playback state when disconnecting the active Squeeze target", () => {
    activeBackend.set("squeeze");
    activeSqueezePlayer.set("A");
    currentTrack.set({ id: 1, title: "Track" });
    isPlaying.set(true);
    currentTime.set(12);
    duration.set(100);

    disconnectSqueezePlayer("A");

    expect(get(isPlaying)).toBe(false);
    expect(get(currentTrack)).toBeNull();
    expect(get(currentTime)).toBe(0);
    expect(get(duration)).toBe(0);
  });

  it("preserves current playback when disconnecting a different Squeeze player", () => {
    activeBackend.set("squeeze");
    activeSqueezePlayer.set("A");
    currentTrack.set({ id: 1, title: "Track" });
    isPlaying.set(true);

    disconnectSqueezePlayer("B");

    expect(get(activeSqueezePlayer)).toBe("A");
    expect(get(activeBackend)).toBe("squeeze");
    expect(get(isPlaying)).toBe(true);
    expect(get(currentTrack)?.id).toBe(1);
    expect(squeezeDisconnectPlayer).toHaveBeenCalledWith("B");
  });

  it("preserves cloud playback when disconnecting the previous Squeeze target", () => {
    activeSqueezePlayer.set("A");
    activeBackend.set("remote");
    currentTrack.set({ id: 2, title: "Cloud track" });
    isPlaying.set(true);

    disconnectSqueezePlayer("A");

    expect(get(activeSqueezePlayer)).toBeNull();
    expect(get(activeBackend)).toBe("remote");
    expect(get(isPlaying)).toBe(true);
    expect(get(currentTrack)?.id).toBe(2);
  });

  it("discovery poll never boots the server — boot and panel own the lifecycle", async () => {
    // The server lifecycle belongs to app boot (+page.svelte) and the
    // Connect panel toggle. The discovery poll must not boot it on its
    // own, otherwise any stray poll trigger silently resurrects the
    // server and the Eversolo auto-reconnects to it.
    squeezeIsRunning.mockResolvedValue(false);
    await startGlobalSqueezeDiscovery();
    expect(squeezeStartServer).not.toHaveBeenCalled();
  });

  it("does not auto-select a discovered player — connection must be explicit", async () => {
    // A device that reconnected to the (user-started) server must not
    // silently become the active control target.
    squeezeGetPlayers.mockResolvedValue([info("AA:BB:CC:DD:EE:FF", 1)]);
    await startGlobalSqueezeDiscovery();
    expect(get(activeSqueezePlayer)).toBeNull();
    expect(get(activeBackend)).not.toBe("squeeze");
  });

  it("disconnect tells the backend to drop the player", async () => {
    const mac = "AA:BB:CC:DD:EE:FF";
    squeezeGetPlayerState.mockResolvedValue(info(mac, 1));
    activeSqueezePlayer.set(mac);
    activeBackend.set("squeeze");

    disconnectSqueezePlayer(mac);

    expect(squeezeDisconnectPlayer).toHaveBeenCalledWith(mac);
    expect(get(activeSqueezePlayer)).toBeNull();
    expect(get(activeBackend)).toBe("none");
  });

  it("does not re-select a player the user disconnected", async () => {
    const mac = "AA:BB:CC:DD:EE:FF";
    squeezeGetPlayers.mockResolvedValue([info(mac, 1)]);
    await startGlobalSqueezeDiscovery();
    disconnectSqueezePlayer(mac);
    // A later discovery tick (device re-announced via the UDP beacon)
    // must not bring the disconnected device back as the active target.
    await pollSqueezePlayersOnce();
    expect(get(activeSqueezePlayer)).toBeNull();
    expect(get(activeBackend)).not.toBe("squeeze");
  });
});
