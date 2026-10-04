import { afterEach, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import type { Track } from "$lib/api/tauri";
import type { PersistedState } from "$lib/stores/persist";

// Only the OS boundary is replaced; application, persistence, SDK and cover helpers stay real.
vi.mock("@tauri-apps/plugin-os", () => ({ platform: () => "windows", hostname: async () => "Synthetic PC" }));
afterEach(() => { vi.clearAllTimers(); vi.useRealTimers(); vi.unstubAllGlobals(); });

it("boots the real desktop with a restored file-cover track before any wrapper IPC preload", async () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-10-03T12:00:00Z"));
  const track: Track = {
    id: 1, path: "C:\\synthetic-audion-fixture\\track.flac", title: "Fixture",
    artist: "Artist", album: "Album", track_number: 1, duration: 180,
    album_id: 1, format: "flac", bitrate: 900, cover_url: null,
    track_cover: null, track_cover_path: "C:\\synthetic-audion-fixture\\cover.jpg",
    source_type: "local", external_id: null, local_src: null, disc_number: 1,
    metadata_json: null, date_added: null, play_count: 0,
  };
  const persisted: PersistedState = {
    volume: 0.7, lyricsVisible: false, queue: [track], queueIndex: 0,
    userQueueCount: 0, shuffle: false, repeat: "none", shuffledIndices: [],
    shuffledIndex: 0, playbackContext: null, currentTime: 12, duration: 180,
    lastTrack: { id: track.id, path: track.path, title: track.title, artist: track.artist, album: track.album },
  };
  const values = new Map([
    ["rlist_player_state", JSON.stringify(persisted)],
    ["covers_migrated", "true"],
    ["audion_settings", JSON.stringify({ autoScanLibrary: false, showDiscord: false })],
  ]);
  const storage = {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value); },
    removeItem: (key: string) => { values.delete(key); }, clear: () => values.clear(),
    key: (index: number) => [...values.keys()][index] ?? null,
    get length() { return values.size; },
  };
  const sequence: string[] = [];
  const conversions: string[] = [];
  const unexpected: string[] = [];
  let callbackId = 0;
  const callbacks = new Map<number, unknown>();
  vi.stubGlobal("window", {
    __TAURI_INTERNALS__: {
      convertFileSrc(filePath: string, protocol: string) {
        sequence.push("convert");
        if (protocol !== "asset") throw new Error("Unexpected protocol");
        const url = `http://${protocol}.localhost/${encodeURIComponent(filePath)}`;
        conversions.push(url);
        return url;
      },
      transformCallback(callback: unknown) { callbacks.set(++callbackId, callback); return callbackId; },
      unregisterCallback(id: number) { callbacks.delete(id); },
      async invoke(command: string) {
        sequence.push(command);
        switch (command) {
          case "get_application_mode": return "desktop";
          case "get_window_start_mode": return "normal";
          case "get_listenbrainz_token_set": return false;
          case "native_audio_available": return true;
          case "windows_init_thumbar": return true;
          case "plugin:event|listen": return callbackId;
          case "plugin:deep-link|get_current": return null;
          case "get_liked_track_ids": case "get_liked_album_ids": case "get_listen_later_album_ids": case "get_playlists": return [];
          case "get_library": return { tracks: [], albums: [], artists: [] };
          case "sync_get_auth_state": return { is_logged_in: false, user_id: null, email: null, name: null, avatar_url: null, is_supporter: false, supporter_until: null };
          case "sync_get_status": return { is_syncing: false, last_sync_at: null, pending_changes: 0, last_error: null };
          case "squeeze_is_running": return false;
          case "control_host_enable": return { enabled: false };
          case "windows_update_thumbar_state": case "plugin:event|unlisten": case "audio_stop": case "squeeze_start_server": case "discord_clear_presence": case "discord_disconnect": return undefined;
          default: unexpected.push(command); throw new Error(`Unexpected synthetic native command: ${command}`);
        }
      },
    },
    __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener() {} },
    localStorage: storage, addEventListener() {}, removeEventListener() {},
    matchMedia: () => ({ matches: false, addEventListener() {}, removeEventListener() {} }),
  });
  vi.stubGlobal("localStorage", storage);
  vi.stubGlobal("navigator", { userAgent: "Synthetic Windows", platform: "Win32", onLine: false });
  vi.stubGlobal("requestIdleCallback", () => 1);
  vi.stubGlobal("cancelIdleCallback", () => {});
  vi.stubGlobal("fetch", () => { throw new Error("Network forbidden"); });
  vi.stubGlobal("WebSocket", class { constructor() { throw new Error("Network forbidden"); } });
  vi.stubGlobal("Audio", class { constructor() { throw new Error("Media forbidden"); } });

  const api = await import("$lib/api/tauri");
  const core = await import("@tauri-apps/api/core");
  const app = await import("$lib/application/bootstrap");
  await api.initPlatformDetection();
  const mode = await core.invoke<"desktop">("get_application_mode");
  const handle = await app.bootstrapApplication(mode, app.defaultBootstrapLoaders);
  try {
    const player = await import("./player-runtime");
    expect(get(player.currentTrack)).toEqual(track);
    expect(get(player.isPlaying)).toBe(false);
    expect(conversions).toEqual([
      "http://asset.localhost/C%3A%5Csynthetic-audion-fixture%5Ccover.jpg",
      "http://asset.localhost/C%3A%5Csynthetic-audion-fixture%5Ccover.jpg",
    ]);
    expect(sequence.indexOf("convert")).toBeLessThan(sequence.indexOf("get_library"));
    expect(unexpected).toEqual([]);
  } finally { await handle.dispose(); }
  expect(unexpected).toEqual([]);
});
