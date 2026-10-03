import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const state = vi.hoisted(() => ({
  starts: 0,
  cleanups: 0,
  resourcesAlive: false,
  stopPlugins: async () => {},
}));
vi.mock("./player-runtime", () => ({
  initAudioBackend: async () => { state.starts++; state.resourcesAlive = true; },
  cleanupPlayer: () => { state.cleanups++; state.resourcesAlive = false; },
  pause: async () => {},
}));
vi.mock("$lib/stores/settings", () => ({ appSettings: {
  initialize: async () => {},
  subscribe: (run: (value: { autoScanLibrary: boolean }) => void) => { run({ autoScanLibrary: false }); return () => {}; },
} }));
vi.mock("$lib/stores/equalizer", () => ({ equalizer: { initialize() {} } }));
vi.mock("$lib/stores/sleepTimer", () => ({ initializeSleepTimer: () => () => {} }));
vi.mock("$lib/stores/squeeze", () => ({ initializeSqueeze: () => () => {}, startGlobalSqueezeDiscovery: async () => {} }));
vi.mock("$lib/stores/websocket", () => ({ wsStore: { initialize: () => () => {} } }));
vi.mock("$lib/stores/persist", () => ({ initializeFromPersistedState() {}, setupAutoSave: () => () => {} }));
vi.mock("$lib/stores/pinned", () => ({ initializePinned: () => () => {} }));
vi.mock("$lib/stores/customArtwork", () => ({ initializeCustomArtwork: () => () => {} }));
vi.mock("$lib/stores/playlistCovers", () => ({ initializePlaylistCovers: () => () => {} }));
vi.mock("$lib/stores/lyrics", () => ({ initializeLyricsPreferences: () => () => {}, destroyLyricsSync() {} }));
vi.mock("$lib/stores/discordPresence", () => ({ initDiscordPresence() {}, disposeDiscordPresence() {} }));
vi.mock("$lib/stores/sync", () => ({ initSync: async () => {}, destroySync() {} }));
vi.mock("$lib/stores/liked", () => ({ loadLikedTracks: async () => {} }));
vi.mock("$lib/stores/liked-albums", () => ({ loadLikedAlbums: async () => {} }));
vi.mock("$lib/stores/listen-later", () => ({ loadListenLaterAlbums: async () => {} }));
vi.mock("$lib/stores/library", () => ({ loadLibrary: async () => {}, loadPlaylists: async () => {}, refreshLibrarySilently: async () => {} }));
vi.mock("$lib/stores/plugin-store", () => ({ pluginStore: { init: async () => {}, dispose: () => state.stopPlugins() } }));
vi.mock("$lib/api/tauri", () => ({ migrateCoversToFiles: vi.fn(), startWatcher: vi.fn(), squeezeStartServer: async () => {} }));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));

beforeEach(() => {
  vi.resetModules();
  state.starts = 0;
  state.cleanups = 0;
  state.resourcesAlive = false;
  state.stopPlugins = async () => {};
  vi.stubGlobal("localStorage", { getItem: () => "true" });
  vi.stubGlobal("requestIdleCallback", () => 1);
  vi.stubGlobal("cancelIdleCallback", () => {});
});
afterEach(() => vi.unstubAllGlobals());

describe("real desktop resource ownership", () => {
  it("reserves the pending owner before the previous owner can tear down resources", async () => {
    const { bootstrapDesktop } = await import("./bootstrap");
    const first = await bootstrapDesktop();
    const pending = bootstrapDesktop();
    await first.dispose();
    const second = await pending;
    expect(state.resourcesAlive).toBe(true);
    expect(state.starts).toBe(1);
    expect(state.cleanups).toBe(0);
    await second.dispose();
    expect(state.cleanups).toBe(1);
    expect(state.resourcesAlive).toBe(false);
  });

  it("keeps resources until both ordinary owners are released exactly once", async () => {
    const { bootstrapDesktop } = await import("./bootstrap");
    const first = await bootstrapDesktop();
    const second = await bootstrapDesktop();
    await first.dispose();
    await first.dispose();
    expect(state.resourcesAlive).toBe(true);
    expect(state.cleanups).toBe(0);
    await second.dispose();
    await second.dispose();
    expect(state.starts).toBe(1);
    expect(state.cleanups).toBe(1);
  });

  it("waits for an already-started teardown before starting fresh resources", async () => {
    const { bootstrapDesktop } = await import("./bootstrap");
    let finish!: () => void;
    state.stopPlugins = () => new Promise<void>(resolve => { finish = resolve; });
    const first = await bootstrapDesktop();
    const disposing = first.dispose();
    await vi.waitFor(() => expect(state.cleanups).toBe(1));
    let acquired = false;
    const pending = bootstrapDesktop().then(handle => { acquired = true; return handle; });
    await Promise.resolve();
    expect(acquired).toBe(false);
    expect(state.starts).toBe(1);
    finish();
    await disposing;
    const second = await pending;
    expect(state.starts).toBe(2);
    expect(state.resourcesAlive).toBe(true);
    state.stopPlugins = async () => {};
    await second.dispose();
  });
});
