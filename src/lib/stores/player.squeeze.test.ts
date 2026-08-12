import { beforeEach, describe, expect, it, vi } from "vitest";

const { squeezePlay } = vi.hoisted(() => ({ squeezePlay: vi.fn() }));
vi.mock("$lib/api/tauri", () => ({
  squeezePlay,
  getAudioSrc: vi.fn(),
  getAlbumArtSrc: vi.fn(),
  getTrackCoverSrc: vi.fn(),
  convertFileSrc: vi.fn(),
  listen: vi.fn(),
  initWindowsThumbar: vi.fn(),
  updateWindowsThumbarState: vi.fn(),
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

import { playTrackOnSqueeze } from "./player";

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
