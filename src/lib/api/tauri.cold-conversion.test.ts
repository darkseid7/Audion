import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Album, Track } from "./tauri";

const path = "C:\\synthetic-audion-fixture\\cover space.jpg";
const url = "http://asset.localhost/C%3A%5Csynthetic-audion-fixture%5Ccover%20space.jpg";
const track: Track = {
  id: 1, path: "C:\\synthetic-audion-fixture\\track.flac", title: "Fixture",
  artist: "Artist", album: "Album", track_number: 1, duration: 180,
  album_id: 1, format: "flac", bitrate: 900, cover_url: null,
  track_cover: null, track_cover_path: path, source_type: "local",
  external_id: null, local_src: null, disc_number: 1, metadata_json: null,
  date_added: null, play_count: 0,
};
const album: Album = { id: 1, name: "Album", artist: "Artist", art_data: null, art_path: path, year: null, original_year: null };

beforeEach(() => vi.resetModules());
afterEach(() => vi.unstubAllGlobals());

function nativeWindow() {
  vi.stubGlobal("window", {
    __TAURI_INTERNALS__: {
      convertFileSrc(filePath: string, protocol: string) {
        if (protocol !== "asset") throw new Error("Unexpected protocol");
        return `http://${protocol}.localhost/${encodeURIComponent(filePath)}`;
      },
      invoke() { throw new Error("Cold conversion must not invoke IPC"); },
    },
  });
}

it("converts a cold desktop file without prior asynchronous IPC", async () => {
  nativeWindow();
  const api = await import("./tauri");
  expect(api.convertFileSrc(path)).toBe(url);
});

it("renders file-based track and album covers from a cold desktop binding", async () => {
  nativeWindow();
  const api = await import("./tauri");
  expect(api.getTrackCoverSrc(track)).toBe(url);
  expect(api.getAlbumCoverSrc(album)).toBe(url);
  expect(api.getAlbumArtSrc(path, true)).toBe(url);
});

it("imports safely without window and fails explicitly on non-native conversion", async () => {
  vi.stubGlobal("window", undefined);
  const api = await import("./tauri");
  expect(api.isTauri()).toBe(false);
  expect(() => api.convertFileSrc(path)).toThrow();
  vi.stubGlobal("window", {});
  expect(() => api.convertFileSrc(path)).toThrow();
});

it("preserves native converter failures instead of hiding them", async () => {
  const error = new Error("Native asset conversion failed");
  vi.stubGlobal("window", { __TAURI_INTERNALS__: { convertFileSrc() { throw error; } } });
  const api = await import("./tauri");
  expect(() => api.getTrackCoverSrc(track)).toThrow(error);
});

it("retains URL, base64 and absent-cover behavior without native conversion", async () => {
  vi.stubGlobal("window", undefined);
  const api = await import("./tauri");
  expect(api.getTrackCoverSrc({ ...track, track_cover_path: null, cover_url: "https://example.test/cover.jpg" })).toBe("https://example.test/cover.jpg");
  expect(api.getTrackCoverSrc({ ...track, track_cover_path: null, track_cover: "/9j/fixture" })).toBe("data:image/jpeg;base64,/9j/fixture");
  expect(api.getAlbumCoverSrc({ ...album, art_path: null, art_data: "iVBORfixture" })).toBe("data:image/png;base64,iVBORfixture");
  expect(api.getAlbumArtSrc("blob:fixture", true)).toBe("blob:fixture");
  expect(api.getAlbumArtSrc("data:image/png;base64,fixture", true)).toBe("data:image/png;base64,fixture");
  expect(api.getTrackCoverSrc({ ...track, track_cover_path: null })).toBeNull();
  expect(api.getAlbumCoverSrc({ ...album, art_path: null })).toBeNull();
});

it("retains non-Linux local audio conversion through the shared native binding", async () => {
  nativeWindow();
  vi.doMock("@tauri-apps/plugin-os", () => ({ platform: () => "windows" }));
  try {
    const api = await import("./tauri");
    expect(await api.getAudioSrc(path)).toBe(url);
  } finally { vi.doUnmock("@tauri-apps/plugin-os"); }
});

it("preserves Linux local audio file URLs instead of using the asset protocol", async () => {
  vi.stubGlobal("window", { __TAURI_INTERNALS__: {
    convertFileSrc() { throw new Error("Linux audio must not use native asset conversion"); },
    invoke() { throw new Error("Unexpected IPC"); },
  } });
  vi.doMock("@tauri-apps/plugin-os", () => ({ platform: () => "linux" }));
  try {
    const api = await import("./tauri");
    expect(await api.getAudioSrc("/synthetic/track space.flac")).toBe("file:///synthetic/track space.flac");
  } finally { vi.doUnmock("@tauri-apps/plugin-os"); }
});
