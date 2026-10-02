import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

const { squeezeDisconnectPlayer } = vi.hoisted(() => ({
  squeezeDisconnectPlayer: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("$lib/api/tauri", () => ({
  squeezeGetPlayerState: vi.fn(() => new Promise(() => {})),
  squeezeDisconnectPlayer,
  squeezeGetPlayers: vi.fn().mockResolvedValue([]),
  squeezeIsRunning: vi.fn().mockResolvedValue(true),
  squeezeStop: vi.fn().mockResolvedValue(undefined),
  getTrackCoverSrc: vi.fn(),
  getTrackById: vi.fn(),
}));
vi.mock("$lib/stores/player", async () => {
  const { writable } = await import("svelte/store");
  return {
    activeBackend: writable("none"),
    currentTrack: writable(null),
    isPlaying: writable(false),
    currentTime: writable(0),
    duration: writable(0),
    volume: writable(0),
    shuffle: writable(false),
    repeat: writable("none"),
  };
});
vi.mock("$lib/stores/websocket", async () => {
  const { writable } = await import("svelte/store");
  return { activeRemoteDevice: writable(null) };
});
vi.mock("$lib/stores/library", () => ({
  getTrackByIdSync: () => null,
  incrementPlayCount: vi.fn(),
  cacheTrack: vi.fn(),
}));
vi.mock("$lib/stores/activity", () => ({ recordTrackPlay: vi.fn() }));

import * as squeeze from "$lib/stores/squeeze";
import { activeBackend, currentTrack, currentTime, duration, isPlaying } from "$lib/stores/player";

// Execute the production event handler without introducing a DOM dependency
// or exporting a component-internal function solely for tests.
function loadConnectPanelHandler(name: string, context: Record<string, unknown>) {
  const component = readFileSync(new URL("./ConnectPanel.svelte", import.meta.url), "utf8");
  const script = component.match(/<script[^>]*>([\s\S]*?)<\/script>/)?.[1];
  if (!script) throw new Error("ConnectPanel script not found");
  const source = ts.createSourceFile("ConnectPanel.ts", script, ts.ScriptTarget.Latest, true);
  const handler = source.statements.find(
    (node) => ts.isFunctionDeclaration(node) && node.name?.text === name,
  );
  if (!handler) throw new Error(`ConnectPanel handler ${name} not found`);
  const { outputText } = ts.transpileModule(handler.getText(source), {
    compilerOptions: { target: ts.ScriptTarget.ES2022 },
  });
  return runInNewContext(`${outputText}; ${name};`, context) as (...args: unknown[]) => unknown;
}

function loadSqueezeSelection(context: Record<string, unknown>) {
  return loadConnectPanelHandler("selectSqueezePlayer", context) as (player: { mac: string }) => void;
}

describe("ConnectPanel Squeeze output selection", () => {
  it("reselects the previous Squeeze target after switching to cloud control", () => {
    const state = {
      $activeSqueezePlayer: "A",
      $activeBackend: "remote",
      activateSqueezeTarget(mac: string) {
        state.$activeSqueezePlayer = mac;
        state.$activeBackend = "squeeze";
      },
    };
    loadSqueezeSelection(state)({ mac: "A" });
    expect(state.$activeBackend).toBe("squeeze");
    expect(state.$activeSqueezePlayer).toBe("A");
  });

  it("does not restart control when the same Squeeze target is already active", () => {
    let activations = 0;
    loadSqueezeSelection({
      $activeSqueezePlayer: "A",
      $activeBackend: "squeeze",
      activateSqueezeTarget() { activations += 1; },
    })({ mac: "A" });
    expect(activations).toBe(0);
  });
});


const discoveredPlayer = {
  mac: "A", name: "Player", state: "Playing", capabilities: "",
  current_track: null, elapsed_ms: 12000, volume: 50,
  repeat: "Off", shuffle: false, queue_length: 0, queue_position: 0,
} as const;

function loadServerStop(stop: () => Promise<void>) {
  const context = {
    ...squeeze,
    activeBackend,
    squeezeRunning: true,
    squeezeStarting: false,
    get $activeSqueezePlayer() { return get(squeeze.activeSqueezePlayer); },
    squeezeStopServer: stop,
    console: { error: vi.fn() },
  };
  const toggle = loadConnectPanelHandler("toggleSqueezeServer", context) as () => Promise<void>;
  return { context, toggle };
}

describe("ConnectPanel Squeeze server stop", () => {
  beforeEach(() => {
    squeeze.resetSqueezePollingForTests();
    squeeze.activeSqueezePlayer.set(null);
    activeBackend.set("squeeze");
    squeeze.activeSqueezePlayer.set("A");
    currentTrack.set({ id: 1, title: "Playing track" } as any);
    currentTime.set(12);
    duration.set(100);
    isPlaying.set(true);
    squeeze.discoveredSqueezePlayers.set([discoveredPlayer]);
    squeezeDisconnectPlayer.mockClear();
  });

  afterEach(() => {
    squeeze.stopGlobalSqueezeDiscovery();
    squeeze.resetSqueezePollingForTests();
    squeeze.activeSqueezePlayer.set(null);
    activeBackend.set("none");
  });

  it("clears the active Squeeze playback after successful stop without disconnect IPC", async () => {
    const ownsTarget = squeeze.captureSqueezeTargetOwnership("A");
    const { context, toggle } = loadServerStop(async () => {});
    await toggle();

    expect(get(squeeze.activeSqueezePlayer)).toBeNull();
    expect(get(activeBackend)).toBe("none");
    expect(get(currentTrack)).toBeNull();
    expect(get(currentTime)).toBe(0);
    expect(get(duration)).toBe(0);
    expect(get(isPlaying)).toBe(false);
    expect(get(squeeze.discoveredSqueezePlayers)).toEqual([]);
    expect(ownsTarget()).toBe(false);
    expect(context.squeezeRunning).toBe(false);
    expect(context.squeezeStarting).toBe(false);
    expect(squeezeDisconnectPlayer).not.toHaveBeenCalled();
  });

  it.each(["remote", "native", "html5"] as const)(
    "preserves %s playback when stopping with a stale Squeeze target",
    async (backend) => {
      activeBackend.set(backend);
      const { toggle } = loadServerStop(async () => {});
      await toggle();

      expect(get(squeeze.activeSqueezePlayer)).toBeNull();
      expect(get(activeBackend)).toBe(backend);
      expect(get(currentTrack)?.id).toBe(1);
      expect(get(currentTime)).toBe(12);
      expect(get(duration)).toBe(100);
      expect(get(isPlaying)).toBe(true);
      expect(squeezeDisconnectPlayer).not.toHaveBeenCalled();
    },
  );

  it("preserves the connection and playback when server stop fails", async () => {
    const ownsTarget = squeeze.captureSqueezeTargetOwnership("A");
    const { context, toggle } = loadServerStop(async () => { throw new Error("stop failed"); });
    await toggle();

    expect(get(squeeze.activeSqueezePlayer)).toBe("A");
    expect(get(activeBackend)).toBe("squeeze");
    expect(get(currentTrack)?.id).toBe(1);
    expect(get(currentTime)).toBe(12);
    expect(get(duration)).toBe(100);
    expect(get(isPlaying)).toBe(true);
    expect(get(squeeze.discoveredSqueezePlayers)).toEqual([discoveredPlayer]);
    expect(ownsTarget()).toBe(true);
    expect(context.squeezeRunning).toBe(true);
    expect(context.squeezeStarting).toBe(false);
    expect(squeezeDisconnectPlayer).not.toHaveBeenCalled();
  });
});
