import { afterEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

const { forbiddenDomainCalls, controllerCalls } = vi.hoisted(() => ({ forbiddenDomainCalls: [] as string[], controllerCalls: [] as {command:string;args:unknown}[] }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string,args:unknown) => {
    if(command==="controller_connection" && JSON.stringify(args)===JSON.stringify({request:{type:"begin_scope"}})){controllerCalls.push({command,args});return {type:"scope",scopeId:"native-renderer-scope"};}
    if(command==="controller_suspend" && JSON.stringify(args)===JSON.stringify({fence:{scopeId:"native-renderer-scope",generation:1}})){controllerCalls.push({command,args});return;}
    forbiddenDomainCalls.push(command);
  }),
  convertFileSrc: (path: string) => path,
  isTauri: () => true,
}));
vi.mock("$lib/plugins/runtime", () => {
  forbiddenDomainCalls.push("plugin-module");
  return {
    PluginRuntime: class { constructor() { forbiddenDomainCalls.push("plugin-runtime"); } },
    setGlobalPermissionManager: vi.fn(),
  };
});

afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); vi.resetModules(); });

describe("role bootstrap isolation", () => {
  it("controller_import_and_bootstrap_ignore_poisoned_legacy_storage", async () => {
    forbiddenDomainCalls.length = 0;controllerCalls.length=0;
    const records = new Map([
      ["rlist_player_state", JSON.stringify({ queue: [{ id: 7, path: "C:/private/song.flac" }], currentTime: 88 })],
      ["audion_sleep_timer", JSON.stringify({ endsAt: Date.now() + 60000, lastDurationMinutes: 30 })],
      ["audion_settings", JSON.stringify({ autoplay: true, remoteControlEnabled: true, audioBackend: "native" })],
      ["audion_auth", JSON.stringify({ is_logged_in: true, access_token: "old-cloud-token" })],
    ]);
    vi.stubGlobal("window", { addEventListener() {}, removeEventListener() {} });
    vi.stubGlobal("navigator", { onLine: true, userAgent: "Android" });
    vi.stubGlobal("localStorage", {
      getItem: vi.fn((key: string) => records.get(key) ?? null),
      setItem: vi.fn(), removeItem: vi.fn(),
    });
    for (const name of ["Audio", "AudioContext", "WebSocket"]) {
      const constructor = class { constructor() { forbiddenDomainCalls.push(name); } };
      vi.stubGlobal(name, constructor);
      (window as unknown as Record<string, unknown>)[name] = constructor;
    }
    vi.spyOn(globalThis, "setInterval").mockImplementation((() => {
      forbiddenDomainCalls.push("domain-interval"); return 1;
    }) as unknown as typeof setInterval);

    window.setInterval = globalThis.setInterval;
    vi.stubGlobal("fetch", vi.fn(async () => { forbiddenDomainCalls.push("fetch"); throw new Error("Network forbidden"); }));
    const player = await import("$lib/stores/player");
    await import("$lib/stores/sleepTimer");
    const { authState } = await import("$lib/stores/sync");
    authState.update(state => ({ ...state, is_logged_in: true }));
    await import("$lib/stores/websocket");
    await import("$lib/stores/squeeze");
    await import("$lib/stores/equalizer");
    await import("$lib/stores/lyrics");
    await import("$lib/stores/settings");
    await import("$lib/stores/persist");
    await import("$lib/stores/pinned");
    await import("$lib/stores/customArtwork");
    await import("$lib/stores/playlistCovers");
    await import("$lib/stores/plugin-store");
    expect(forbiddenDomainCalls).toEqual([]);
    expect(get(player.queue)).toEqual([]);
    expect("set" in player.currentTrack).toBe(false);
    const { bootstrapApplication, defaultBootstrapLoaders } = await import("./bootstrap");
    const handle = await bootstrapApplication("controller", defaultBootstrapLoaders);
    expect(forbiddenDomainCalls).toEqual([]);
    expect(get(player.queue)).toEqual([]);
    expect(localStorage.getItem).not.toHaveBeenCalledWith("rlist_player_state");
    expect(localStorage.getItem).not.toHaveBeenCalledWith("audion_sleep_timer");
    await handle.dispose();
    await Promise.resolve();await Promise.resolve();
    expect(controllerCalls).toEqual([{command:"controller_connection",args:{request:{type:"begin_scope"}}},{command:"controller_suspend",args:{fence:{scopeId:"native-renderer-scope",generation:1}}}]);
    expect(forbiddenDomainCalls).toEqual([]);
  });

  it("old_dispose_cannot_uninstall_new_owner", async () => {
    const { bootstrapApplication } = await import("./bootstrap");
    const { createUnavailablePort, getApplicationPort } = await import("./port");
    const first = { port: createUnavailablePort(), dispose: vi.fn(async () => {}) };
    const second = { port: createUnavailablePort(), dispose: vi.fn(async () => {}) };
    const old = await bootstrapApplication("controller", { controller: async () => first, desktop: async () => first });
    const current = await bootstrapApplication("controller", { controller: async () => second, desktop: async () => second });
    await old.dispose();
    expect(getApplicationPort()).toBe(second.port);
    expect(second.dispose).not.toHaveBeenCalled();
    await old.dispose();
    expect(first.dispose).toHaveBeenCalledTimes(1);
    await current.dispose();
    expect(() => getApplicationPort()).toThrow("Application not ready");
  });
});


describe("shared typed facade", () => {
  it("converts legacy fractional seek to protocol seconds without writing playback state", async () => {
    const facade = await import("$lib/stores/player");
    const { playbackStateWriter } = await import("$lib/stores/playback-state");
    const { installApplicationPort, createUnavailablePort } = await import("./port");
    playbackStateWriter.duration.set(200);
    playbackStateWriter.currentTime.set(12);
    const execute = vi.fn(async () => ({ status: "applied" as const, revision: 1 }));
    const uninstall = installApplicationPort({ ...createUnavailablePort(), execute });
    facade.setPlayerPreconditions({ hostEpoch: "epoch" });
    await facade.seek(0.25);
    expect(execute).toHaveBeenCalledWith({ type: "seek", seconds: 50 }, { hostEpoch: "epoch" });
    expect(get(facade.currentTime)).toBe(12);
    uninstall();
  });
});


describe("bootstrap lifecycle", () => {
  it("clears role capability state when the current loader fails", async () => {
    const { bootstrapApplication, desktopEffectsEnabled } = await import("./bootstrap");
    const failure = async () => { throw new Error("startup failed"); };
    await expect(bootstrapApplication("desktop", { desktop: failure, controller: failure })).rejects.toThrow("startup failed");
    expect(get(desktopEffectsEnabled)).toBe(false);
  });
  it("does not switch the immutable role after startup", async () => {
    const { bootstrapApplication } = await import("./bootstrap");
    const { createUnavailablePort } = await import("./port");
    const loader = vi.fn(async () => ({ port: createUnavailablePort(), async dispose() {} }));
    const current = await bootstrapApplication("controller", { controller: loader, desktop: loader });
    await expect(bootstrapApplication("desktop", { controller: loader, desktop: loader })).rejects.toThrow("Application role is immutable");
    expect(loader).toHaveBeenCalledTimes(1);
    await current.dispose();
  });
});


it("a slow obsolete loader cannot replace the newer installed port", async () => {
  const { bootstrapApplication } = await import("./bootstrap");
  const { getApplicationPort, createUnavailablePort } = await import("./port");
  const obsolete = { port: createUnavailablePort(), dispose: vi.fn(async () => {}) };
  let release!: (value: typeof obsolete) => void;
  const pending = new Promise<typeof obsolete>(resolve => { release = resolve; });
  const stale = bootstrapApplication("controller", { controller: () => pending, desktop: () => pending });
  const latest = { port: createUnavailablePort(), async dispose() {} };
  const current = await bootstrapApplication("controller", { controller: async () => latest, desktop: async () => latest });
  release(obsolete);
  await expect(stale).rejects.toThrow("Application bootstrap superseded");
  expect(getApplicationPort()).toBe(latest.port);
  expect(obsolete.dispose).toHaveBeenCalledTimes(1);
  await current.dispose();
});


it("rejects legacy playTrack options instead of silently dropping them from a typed intent", async () => {
  const facade = await import("$lib/stores/player");
  const { installApplicationPort, createUnavailablePort } = await import("./port");
  const execute = vi.fn(async () => ({ status: "applied" as const, revision: 1 }));
  const uninstall = installApplicationPort({ ...createUnavailablePort(), execute });
  facade.setPlayerPreconditions({ hostEpoch: "epoch" });
  await expect(facade.playTrack({ id: 1 } as import("$lib/api/tauri").Track, true, 15)).rejects.toThrow("Application capability unavailable");
  expect(execute).not.toHaveBeenCalled();
  uninstall();
});


it("keeps the shared streaming classifier passive and preserves local/provider precedence", async () => {
  const { isStreaming, sliderToAudioVolume, audioVolumeToSlider } = await import("./playback-helpers");
  const cases = [
    [{ path: "https://host/song", source_type: "local" }, false],
    [{ path: "https://host/song", local_src: "C:/song.flac", source_type: "provider" }, false],
    [{ path: "file://song", source_type: "provider" }, false],
    [{ path: "asset://song", source_type: "provider" }, false],
    [{ path: "tauri://song", source_type: "provider" }, false],
    [{ path: "https://host/song" }, true],
    [{ path: "http://host/song" }, true],
    [{ path: "provider://song", source_type: "provider" }, true],
    [{ path: "C:/song.flac" }, false],
    [{ path: "blob:track" }, false],
  ] as const;
  for (const [track, expected] of cases) {
    expect(isStreaming(track as import("$lib/api/tauri").Track)).toBe(expected);
  }
  expect(sliderToAudioVolume(0.5)).toBe(0.25);
  expect(audioVolumeToSlider(0.25)).toBe(0.5);
});
