import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

const { invoke, triggerSync } = vi.hoisted(() => ({ invoke: vi.fn(), triggerSync: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("$lib/stores/sync", async () => {
  const { writable, derived } = await import("svelte/store");
  const authState = writable({ is_logged_in: true });
  return { authState, isLoggedIn: derived(authState, state => state.is_logged_in), triggerSync };
});
vi.mock("$lib/stores/settings", async () => {
  const { writable } = await import("svelte/store");
  return { appSettings: writable({ remoteControlEnabled: true }) };
});

class TestSocket {
  static OPEN = 1;
  readyState = 1;
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: string }) => void) | null = null;
  onclose: ((event: { wasClean: boolean; code: number; reason: string }) => void) | null = null;
  onerror: ((error: unknown) => void) | null = null;
  send = vi.fn();
  close = vi.fn(() => { this.readyState = 3; });
  constructor(readonly url: string) { sockets.push(this); }
}
let sockets: TestSocket[];
const authValues: Record<string, string> = {
  sync_get_server_url: "https://desktop.example",
  sync_get_access_token: "token",
  sync_get_device_id: "desktop-id",
};
const closed = { wasClean: true, code: 1000, reason: "closed" };
async function flushAuthentication() {
  // Drain the three native authentication awaits and their finalizers.
  for (let turn = 0; turn < 12; turn++) await Promise.resolve();
}

beforeEach(() => {
  vi.resetModules();
  vi.useFakeTimers();
  sockets = [];
  invoke.mockReset().mockImplementation(async (command: string) => authValues[command]);
  triggerSync.mockClear();
  vi.stubGlobal("WebSocket", TestSocket);
  vi.spyOn(console, "log").mockImplementation(() => {});
});
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); vi.restoreAllMocks(); });

describe("desktop WebSocket lifetime", () => {
  it.each(Object.keys(authValues))("ignores pending %s authentication after disposal", async (blocked) => {
    let release!: (value: string) => void;
    const pending = new Promise<string>(resolve => { release = resolve; });
    invoke.mockImplementation(async (command: string) => command === blocked ? pending : authValues[command]);
    const { wsStore } = await import("./websocket");
    const dispose = wsStore.initialize();
    await flushAuthentication();
    expect(invoke).toHaveBeenCalledWith(blocked);
    dispose();
    const callsAtDisposal = invoke.mock.calls.length;
    release(authValues[blocked]);
    await flushAuthentication();
    expect(sockets).toHaveLength(0);
    expect(invoke.mock.calls).toHaveLength(callsAtDisposal);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("does not reconnect after intentional disposal even if close was already queued", async () => {
    const { wsStore } = await import("./websocket");
    const dispose = wsStore.initialize();
    await flushAuthentication();
    const socket = sockets.at(-1)!;
    const queuedClose = socket.onclose!;
    dispose();
    const countAtDisposal = sockets.length;
    queuedClose(closed);
    expect(vi.getTimerCount()).toBe(0);
    await vi.advanceTimersByTimeAsync(60_000);
    expect(sockets).toHaveLength(countAtDisposal);
    expect(get(wsStore).statusText).toBe("Disconnected");
  });

  it("stale callbacks and the old disposer cannot clear or publish into a new owner", async () => {
    const { wsStore } = await import("./websocket");
    const disposeOld = wsStore.initialize();
    await flushAuthentication();
    const old = sockets.at(-1)!;
    const stale = { open: old.onopen!, message: old.onmessage!, close: old.onclose!, error: old.onerror! };
    disposeOld();
    const disposeNew = wsStore.initialize();
    await flushAuthentication();
    const current = sockets.at(-1)!;
    current.onopen!();
    const stateBefore = get(wsStore);
    const published = vi.fn();
    const off = wsStore.onMessage(published);
    const errors = vi.spyOn(console, "error").mockImplementation(() => {});
    stale.open();
    stale.message({ data: JSON.stringify({ type: "sync_notify", payload: {} }) });
    stale.error(new Error("old socket"));
    stale.close(closed);
    disposeOld();
    wsStore.send("probe", { owner: "new" });
    expect(get(wsStore)).toEqual(stateBefore);
    expect(current.send).toHaveBeenCalledWith(JSON.stringify({ type: "probe", payload: { owner: "new" } }));
    expect(current.close).not.toHaveBeenCalled();
    expect(published).not.toHaveBeenCalled();
    expect(triggerSync).not.toHaveBeenCalled();
    expect(errors).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
    off();
    disposeNew();
  });

  it("coalesces authentication and reconnects an unexpectedly closed current socket", async () => {
    const { wsStore } = await import("./websocket");
    const dispose = wsStore.initialize();
    await flushAuthentication();
    expect(sockets).toHaveLength(1);
    sockets[0].onclose!({ wasClean: false, code: 1006, reason: "network" });
    expect(vi.getTimerCount()).toBe(1);
    await vi.advanceTimersByTimeAsync(1000);
    expect(sockets).toHaveLength(2);
    sockets[1].onopen!();
    expect(get(wsStore).connected).toBe(true);
    dispose();
  });
});
