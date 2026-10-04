vi.mock("@tauri-apps/plugin-os", () => ({ hostname: vi.fn(async () => "Studio PC") }));
import type { HostSnapshot } from "../types";

vi.mock("./adapter", () => ({ createDesktopAdapter: () => ({
  port: { execute: async () => ({ status: "applied", revision: 1 }) },
  coordinator: {
    captureSnapshot: () => ({ hostId: "host", hostEpoch: "epoch", revision: 0, revisions: { queueRevision: 0, outputRevision: 0, libraryRevision: 0, settingsRevision: 0 }, playback: { status: "stopped", track: null, context: null, position: 0, duration: null, volume: 0.5, shuffle: false, repeat: "none" }, queue: { count: 0, currentEntryId: null }, output: { kind: "pc" }, outputs: [], capabilities: { queries: ["snapshot"], intents: [] }, settings: {}, jobs: [] }),
    subscribeSnapshot: (listener: (snapshot: HostSnapshot) => void) => { state.observe = listener; state.bridgeSteps.push("subscribe"); return () => { state.bridgeSteps.push("unsubscribe"); }; },
  },
  attachAuthority: async () => {},
  pauseForTimer: async () => {}, dispose: async () => { state.bridgeSteps.push("adapter-dispose"); },
}) }));
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const state = vi.hoisted(() => ({
  starts: 0,
  cleanups: 0,
  resourcesAlive: false,
  stopPlugins: async () => {},
  hostEnabled: false,
  bridgeSteps: [] as string[],
  register: undefined as undefined | ((phase: string) => Promise<unknown>),
  observe: (_snapshot: HostSnapshot) => {},
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
vi.mock("@tauri-apps/api/core", () => ({ invoke: async (command: string, args: { request: { phase?: string } }) => {
  if (command === "control_host_enable") return { enabled: state.hostEnabled };
  state.bridgeSteps.push(args.request.phase!);
  if (state.register) return state.register(args.request.phase!);
  return { hostId: "host", lease: { hostEpoch: "epoch", leaseId: "lease" }, ...(args.request.phase === "publish" ? { revision: 0 } : {}) };
} }));
vi.mock("@tauri-apps/api/webviewWindow", () => ({ getCurrentWebviewWindow: () => ({ listen: async () => () => { state.bridgeSteps.push("unlisten"); } }) }));

beforeEach(() => {
  vi.resetModules();
  state.starts = 0;
  state.cleanups = 0;
  state.resourcesAlive = false;
  state.stopPlugins = async () => {};
  state.hostEnabled = false;
  state.bridgeSteps = [];
  state.register = undefined;
  vi.stubGlobal("localStorage", { getItem: () => "true" });
  vi.stubGlobal("requestIdleCallback", () => 1);
  vi.stubGlobal("cancelIdleCallback", () => {});
});
afterEach(() => { vi.unstubAllGlobals(); vi.useRealTimers(); vi.restoreAllMocks(); });

describe("real desktop resource ownership", () => {
  it("shares one reloaded host bridge and releases authority before the final adapter teardown", async () => {
    state.hostEnabled = true;
    const { bootstrapDesktop } = await import("./bootstrap");
    const first = await bootstrapDesktop();
    const second = await bootstrapDesktop();
    await first.dispose();
    expect(state.bridgeSteps).toEqual(["prepare", "subscribe", "publish", "ready"]);
    await second.dispose();
    expect(state.bridgeSteps).toEqual(["prepare", "subscribe", "publish", "ready", "unsubscribe", "unlisten", "release", "adapter-dispose"]);
  });
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

it("returns the initialized desktop adapter rather than an unavailable port", async () => {
  const { bootstrapDesktop } = await import("./bootstrap");
  const handle = await bootstrapDesktop();
  await expect(handle.port.execute({ type: "pause" }, { hostEpoch: "test" })).resolves.toMatchObject({ status: "applied" });
  await handle.dispose();
});

it.each(["initial failure", "steady timeout", "dispose"])("finishes desktop cleanup after %s even when native release never settles", async reason => {
  vi.useFakeTimers(); state.hostEnabled = true;
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  let finishRelease!: (value: unknown) => void; let finishPublication!: (value: unknown) => void;
  const release = new Promise(resolve => { finishRelease = resolve; });
  const publication = new Promise(resolve => { finishPublication = resolve; });
  const ack = { hostId: "host", lease: { hostEpoch: "epoch", leaseId: "lease" }, revision: 0 };
  let publications = 0;
  state.register = phase => {
    if (phase === "release") return release;
    if (phase === "publish" && (reason === "initial failure" || publications++ > 0)) return publication;
    return Promise.resolve(ack);
  };
  const { bootstrapDesktop } = await import("./bootstrap");
  const starting = bootstrapDesktop();
  await vi.advanceTimersByTimeAsync(0);
  if (reason === "initial failure") {
    await vi.advanceTimersByTimeAsync(5000);
    expect(state.bridgeSteps).toContain("unlisten");
    await vi.advanceTimersByTimeAsync(1000);
  }
  const handle = await starting;
  if (reason === "steady timeout") {
    // Real publisher observes a structural update through the adapter's projection subscription.
    const { createDesktopAdapter } = await import("./adapter");
    state.observe({ ...createDesktopAdapter().coordinator.captureSnapshot(), queue: { count: 1, currentEntryId: null } });
    await vi.advanceTimersByTimeAsync(5000);
  }
  let disposed = false;
  const stopping = handle.dispose().then(() => { disposed = true; });
  await vi.advanceTimersByTimeAsync(0);
  expect(state.bridgeSteps).toContain("unlisten");
  await vi.advanceTimersByTimeAsync(1000);
  expect(disposed).toBe(true); await stopping;
  expect(state.resourcesAlive).toBe(false); expect(state.cleanups).toBe(1);
  expect(state.bridgeSteps.filter(step => step === "adapter-dispose")).toHaveLength(1);
  expect(state.bridgeSteps.filter(step => step === "release")).toHaveLength(1);
  expect(warn).toHaveBeenCalledWith(reason === "initial failure" ? "Controller hosting unavailable" : "Controller bridge release unavailable", expect.objectContaining({ message: expect.stringMatching(/unavailable.*unknown/i) }));
  finishPublication(ack); finishRelease(ack); await vi.advanceTimersByTimeAsync(0);
  expect(state.bridgeSteps.filter(step => step === "unlisten")).toHaveLength(1);
  expect(state.bridgeSteps.filter(step => step === "ready")).toHaveLength(reason === "initial failure" ? 0 : 1);
  expect(vi.getTimerCount()).toBe(0);
});

it("desktop startup continues when hostname permission/API is unavailable",async()=>{
 const {hostname}=await import("@tauri-apps/plugin-os");vi.mocked(hostname).mockRejectedValueOnce(new Error("unavailable"));
 const {bootstrapDesktop}=await import("./bootstrap");const handle=await bootstrapDesktop();expect(state.starts).toBe(1);await handle.dispose();
});
