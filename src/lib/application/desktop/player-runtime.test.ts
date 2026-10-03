import { beforeEach, afterEach, expect, it, vi } from "vitest";
import { get } from "svelte/store";
vi.mock("$lib/api/tauri", async original => ({ ...await original<object>(), updateWindowsThumbarState: vi.fn().mockResolvedValue(undefined), initWindowsThumbar: vi.fn().mockResolvedValue(false) }));
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
