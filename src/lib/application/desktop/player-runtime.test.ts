import { beforeEach, afterEach, expect, it, vi } from "vitest";
import { get } from "svelte/store";
vi.mock("$lib/api/tauri", async original => ({ ...await original<object>(), squeezeStop: vi.fn().mockResolvedValue(undefined), squeezePlay: vi.fn().mockResolvedValue(undefined), squeezeDisconnectPlayer: vi.fn().mockResolvedValue(undefined), updateWindowsThumbarState: vi.fn().mockResolvedValue(undefined), initWindowsThumbar: vi.fn().mockResolvedValue(false) }));
vi.mock("$lib/services/native-audio", async original => ({
  ...await original<object>(), shouldUseNativeAudio: async () => true,
  nativeAudioPlay: vi.fn().mockResolvedValue(undefined), nativeAudioStop: vi.fn().mockResolvedValue(undefined),
  nativeAudioSetVolume: vi.fn().mockResolvedValue(undefined), nativeAudioSetRepeatOne: vi.fn().mockResolvedValue(undefined), nativeAudioSetEq: vi.fn().mockResolvedValue(undefined),
}));
import { nativeAudioSetVolume } from "$lib/services/native-audio";
import { initAudioBackend, cleanupPlayer, playTracks, queue, currentTrack, activeBackend } from "./player-runtime";
beforeEach(async () => { vi.stubGlobal("navigator", {}); activeBackend.set("none"); queue.set([]); currentTrack.set(null); await initAudioBackend(); });
afterEach(() => { cleanupPlayer(); vi.unstubAllGlobals(); });
it("discloses playback already started if the following native volume acknowledgement fails", async () => {
  vi.mocked(nativeAudioSetVolume).mockRejectedValueOnce(new Error("volume failed"));
  await expect(playTracks([{ id: 1, path: "fixture.flac", cover_url: "fixture" } as any])).rejects.toMatchObject({ partialEffects: ["Playback started"] });
  expect(get(queue)).toEqual([]);
  expect(get(currentTrack)).toBe(null);
});

it("admits incoming transfer once behind a deferred output stop", async () => {
  const { wsStore } = await import("$lib/stores/websocket");
  const { createDesktopAdapter } = await import("./adapter");
  const { commitSqueezeTarget } = await import("$lib/stores/squeeze");
  const { cacheTrack } = await import("$lib/stores/library");
  const { squeezeStop, squeezePlay } = await import("$lib/api/tauri");
  const { nativeAudioPlay } = await import("$lib/services/native-audio");
  cleanupPlayer();
  let receive!: (type: string, payload: any) => void;
  const subscription = vi.spyOn(wsStore, "onMessage").mockImplementation(handler => { receive = handler; return () => true; });
  await initAudioBackend();
  const track = { id: 41, path: "fixture.flac", title: "Transfer", cover_url: "fixture", duration: 100 } as any;
  cacheTrack(track); commitSqueezeTarget("A");
  let acknowledge!: () => void;
  vi.mocked(squeezeStop).mockReturnValueOnce(new Promise(resolve => { acknowledge = resolve; }));
  vi.mocked(squeezePlay).mockClear(); vi.mocked(nativeAudioPlay).mockClear();
  const adapter = createDesktopAdapter();
  const selecting = adapter.port.execute({ type: "select_output", output: { kind: "pc" } }, { hostEpoch: adapter.state.read().hostEpoch });
  await vi.waitFor(() => expect(squeezeStop).toHaveBeenCalled());
  receive("transfer_playback", { track, isPlaying: true, currentTime: 0 });
  try {
    await new Promise(resolve => setTimeout(resolve, 10));
    expect(squeezePlay).not.toHaveBeenCalled(); expect(nativeAudioPlay).not.toHaveBeenCalled();
    acknowledge(); await selecting;
    await vi.waitFor(() => expect(nativeAudioPlay).toHaveBeenCalledTimes(1));
    await adapter.coordinator.executeLocal(async () => ({ status: "applied" }));
    expect(get(currentTrack)?.id).toBe(41);
  } finally { acknowledge(); await selecting; await adapter.dispose(); subscription.mockRestore(); }
});

it("catches an incoming transfer rejection at its desktop ingress", async () => {
  const { wsStore } = await import("$lib/stores/websocket");
  const { createDesktopAdapter } = await import("./adapter");
  const { cacheTrack } = await import("$lib/stores/library");
  const { nativeAudioPlay } = await import("$lib/services/native-audio");
  cleanupPlayer();
  let receive!: (type: string, payload: any) => void;
  const subscription = vi.spyOn(wsStore, "onMessage").mockImplementation(handler => { receive = handler; return () => true; });
  await initAudioBackend();
  const track = { id: 42, path: "fixture.flac", title: "Rejected transfer", cover_url: "fixture" } as any;
  cacheTrack(track);
  vi.mocked(nativeAudioPlay).mockRejectedValueOnce(new Error("transfer refused"));
  const logged = vi.spyOn(console, "error").mockImplementation(() => {});
  const adapter = createDesktopAdapter();
  receive("transfer_playback", { track, isPlaying: true, currentTime: 0 });
  try {
    await vi.waitFor(() => expect(logged).toHaveBeenCalledWith(expect.objectContaining({ message: expect.stringContaining("transfer refused") })));
    expect(get(currentTrack)).toBe(null);
  } finally { await adapter.dispose(); subscription.mockRestore(); logged.mockRestore(); }
});
