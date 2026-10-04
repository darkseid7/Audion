import { afterEach, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import { setImmediate } from "node:timers/promises";
import type { HostSnapshot } from "../types";
import * as controller from "./bootstrap";

// Keep bootstrap, nativeBridge, session, stores and preference receipts real.
// Only the OS IPC, document and localStorage boundaries are doubled.
const external = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: external.invoke }));
const hostA = "11111111-1111-4111-8111-111111111111";
const hostB = "22222222-2222-4222-8222-222222222222";
const fingerprint = "ab".repeat(32);
const invitation = { status: "invitation_ready", hostId: hostA, fingerprint };
function deferred<T>() {
    let resolve!: (value: T) => void, reject!: (error: unknown) => void;
    const promise = new Promise<T>((a, b) => { resolve = a; reject = b; });
    return { promise, resolve, reject };
}
class TestDocument extends EventTarget {
    visibilityState = "visible";
    change(state: "visible" | "hidden") {
        this.visibilityState = state;
        this.dispatchEvent(new Event("visibilitychange"));
    }
}
const snapshot = (hostId: string): HostSnapshot => ({
    hostId, hostEpoch: "epoch", revision: 0,
    revisions: { libraryRevision: 1, queueRevision: 1, outputRevision: 1, settingsRevision: 0 },
    playback: { status: "paused", track: null, context: null, position: 0, duration: null, volume: 0.5, shuffle: false, repeat: "none" },
    queue: { count: 0, currentEntryId: null }, output: { kind: "pc" }, outputs: [],
    capabilities: { queries: ["snapshot"], intents: ["pause"] }, settings: {}, jobs: [],
});
const cleanups: Array<() => Promise<void>> = [];
afterEach(async () => {
    for (const cleanup of cleanups.splice(0).reverse()) await cleanup();
    vi.unstubAllGlobals();
    external.invoke.mockReset();
});
async function setup(selected: string | null = null) {
    const document = new TestDocument();
    const records = new Map([["audion_controller_hosts", JSON.stringify({ known: selected ? [selected] : [], selected })]]);
    vi.stubGlobal("document", document);
    vi.stubGlobal("localStorage", {
        getItem: (key: string) => records.get(key) ?? null,
        setItem: (key: string, value: string) => { records.set(key, value); },
    });
    const scans: ReturnType<typeof deferred<unknown>>[] = [];
    const approvals: ReturnType<typeof deferred<unknown>>[] = [];
    const operations: Promise<void>[] = [];
    const calls: Array<{ command: string; args: any }> = [];
    let scopes = 0;
    let connection: ReturnType<typeof deferred<unknown>> | undefined;
    external.invoke.mockImplementation((command: string, args: any) => {
        calls.push({ command, args });
        if (command === "controller_connection") {
            if (args.request.type === "begin_scope") return Promise.resolve({ type: "scope", scopeId: `scope-${++scopes}` });
            if (["connect", "update_endpoint"].includes(args.request.type)) {
                return connection?.promise ?? Promise.resolve({ type: "connected", snapshot: snapshot(args.request.hostId), grants: { control: true } });
            }
        }
        if (command === "controller_scan_pair") {
            const scan = deferred<unknown>(); scans.push(scan); return scan.promise;
        }
        if (command === "controller_pair") {
            const approval = deferred<unknown>(); approvals.push(approval); return approval.promise;
        }
        if (command === "controller_suspend" || command === "controller_forget") return Promise.resolve();
        if (command === "controller_request" && args.request.type === "poll") return new Promise(() => {});
        throw new Error(`Unexpected IPC: ${command}`);
    });
    const handle = await controller.bootstrapController();
    // An event-loop checkpoint drains the real async IPC layers; no timed sleeps.
    await setImmediate();
    cleanups.push(async () => {
        await handle.dispose();
        scans.forEach(scan => scan.resolve(invitation));
        approvals.forEach(approval => approval.resolve({ hostId: hostA }));
        connection?.resolve({ type: "connected", snapshot: snapshot(hostA), grants: { control: true } });
        await Promise.all(operations);
    });
    return {
        document, calls, scans, approvals, handle,
        prefs: () => JSON.parse(records.get("audion_controller_hosts")!),
        state: () => get(controller.controllerState),
        connects: () => calls.filter(c => c.command === "controller_connection" && c.args.request.type === "connect").map(c => c.args.request.hostId),
        suspends: () => calls.filter(c => c.command === "controller_suspend"),
        holdConnection: () => { connection = deferred<unknown>(); return connection; },
        async start() {
            const operation = controller.pairController(); operations.push(operation);
            await setImmediate();
            expect(scans.length).toBe(operations.length);
            return { operation, scan: scans.at(-1)! };
        },
        async finish(operation: Promise<void>) {
            approvals.at(-1)!.resolve({ hostId: hostA });
            await operation;
            expect(get(controller.controllerState)).toMatchObject({ status: "connected", ready: true, currentHostId: hostA });
            expect(JSON.parse(records.get("audion_controller_hosts")!)).toEqual({ known: selected && selected !== hostA ? [hostA, selected] : [hostA], selected: hostA });
        },
    };
}

// These tests catch unconditional bootstrap suspend/reconnect and a missing
// generation/foreground check at the scan-to-network handoff.
it.each(["visible-first", "result-first"] as const)("preserves owned capture with %s ordering and the same native fence", async order => {
    const h = await setup(), { operation, scan } = await h.start();
    h.document.change("hidden");
    if (order === "visible-first") h.document.change("visible");
    scan.resolve(invitation); await setImmediate();
    expect(h.state()).toMatchObject({ status: "pairing", pairingFingerprint: fingerprint, ready: false });
    expect(h.suspends()).toEqual([]);
    if (order === "result-first") {
        expect(h.approvals).toHaveLength(0);
        expect(h.connects()).toEqual([]);
        h.document.change("visible"); await setImmediate();
    }
    expect(h.calls.filter(c => c.command === "controller_pair")).toEqual([{
        command: "controller_pair", args: { fence: { scopeId: "scope-1", generation: 1 }, deviceName: "Android controller" },
    }]);
    await h.finish(operation);
});
it.each(["visible-first", "result-first"] as const)("selected old host resume does not supersede capture with %s ordering", async order => {
    const h = await setup(hostB), { operation, scan } = await h.start();
    h.document.change("hidden");
    if (order === "visible-first") h.document.change("visible");
    scan.resolve(invitation); await setImmediate();
    if (order === "result-first") { expect(h.approvals).toHaveLength(0); h.document.change("visible"); await setImmediate(); }
    expect(h.connects()).toEqual([hostB]);
    expect(h.prefs()).toEqual({ known: [hostB], selected: hostB });
    expect(h.approvals).toHaveLength(1);
    await h.finish(operation);
});
it("visible followed synchronously by hidden cannot start network from its queued waiter", async () => {
    const h = await setup(hostB), { operation, scan } = await h.start();
    h.document.change("hidden"); scan.resolve(invitation); await setImmediate();
    h.document.change("visible"); h.document.change("hidden"); await setImmediate();
    expect(h.state()).toMatchObject({ status: "pairing", pairingFingerprint: fingerprint });
    expect(h.approvals).toHaveLength(0); expect(h.connects()).toEqual([hostB]);
    h.document.change("visible"); await setImmediate();
    expect(h.approvals).toHaveLength(1); await h.finish(operation);
});
it.each(["suspend", "switch", "hide"] as const)("fingerprint subscriber %s reentrancy cannot start stale or hidden approval", async intent => {
    const h = await setup(hostB), { operation, scan } = await h.start();
    let acted = false;
    const unsubscribe = controller.controllerState.subscribe(value => {
        if (!acted && value.pairingFingerprint) {
            acted = true;
            if (intent === "suspend") controller.suspendController();
            else if (intent === "switch") void controller.connectController(hostB);
            else h.document.change("hidden");
        }
    });
    try {
        scan.resolve(invitation); await setImmediate();
        expect(acted).toBe(true); expect(h.approvals).toHaveLength(0);
        if (intent === "hide") {
            expect(h.state()).toMatchObject({ status: "pairing", pairingFingerprint: fingerprint });
            h.document.change("visible"); await setImmediate();
            expect(h.approvals).toHaveLength(1); await h.finish(operation);
        } else { await operation; expect(h.prefs()).toEqual({ known: [hostB], selected: hostB }); }
    } finally { unsubscribe(); }
});
it("hidden reentrancy at native pair entry cancels approval rather than preserving capture", async () => {
    const h = await setup(), { operation, scan } = await h.start();
    const implementation = external.invoke.getMockImplementation()!;
    external.invoke.mockImplementation((command, args) => {
        const result = implementation(command, args);
        if (command === "controller_pair") h.document.change("hidden");
        return result;
    });
    scan.resolve(invitation); await setImmediate();
    expect(h.approvals).toHaveLength(1);
    expect(h.state()).toMatchObject({ status: "disconnected", ready: false });
    expect(h.state().pairingFingerprint).toBeUndefined(); expect(h.suspends()).toHaveLength(1);
    h.approvals[0].resolve({ hostId: hostA }); await operation;
    expect(h.connects()).toEqual([]); expect(h.prefs()).toEqual({ known: [], selected: null });
});
it.each(["suspend", "dispose", "switch", "endpoint", "forget", "replacement"] as const)("explicit %s invalidates late capture before staging approval", async intent => {
    const h = await setup(hostB), { operation, scan } = await h.start();
    if (intent === "suspend") controller.suspendController();
    else if (intent === "dispose") await h.handle.dispose();
    else if (intent === "switch") await controller.connectController(hostB);
    else if (intent === "endpoint") await controller.updateControllerEndpoint(hostB, "192.168.1.9:1234");
    else if (intent === "forget") await controller.forgetController(hostB);
    else {
        const replacement = await controller.bootstrapController();
        cleanups.push(() => replacement.dispose());
    }
    await setImmediate(); // Allow the replacement owner to finish its independent connect.
    const before = h.state(), prefs = h.prefs();
    scan.resolve(invitation); await setImmediate();
    expect(h.approvals).toHaveLength(0); expect(h.state()).toEqual(before); expect(h.prefs()).toEqual(prefs); await operation;
});
it.each(["suspend", "dispose", "switch", "endpoint", "forget", "replacement"] as const)("explicit %s releases a hidden foreground waiter without stale selection", async intent => {
    const h = await setup(hostB), { operation, scan } = await h.start();
    h.document.change("hidden"); scan.resolve(invitation); await setImmediate();
    expect(h.state()).toMatchObject({ status: "pairing", pairingFingerprint: fingerprint });
    if (intent === "suspend") controller.suspendController();
    else if (intent === "dispose") await h.handle.dispose();
    else if (intent === "switch") await controller.connectController(hostB);
    else if (intent === "endpoint") await controller.updateControllerEndpoint(hostB, "192.168.1.9:1234");
    else if (intent === "forget") await controller.forgetController(hostB);
    else { const replacement = await controller.bootstrapController(); cleanups.push(() => replacement.dispose()); }
    await operation;
    expect(h.approvals).toHaveLength(0); expect(h.prefs().known).not.toContain(hostA);
});
it("late old capture cleanup cannot erase a newer capture's visibility ownership", async () => {
    const h = await setup(hostB), old = await h.start(), fresh = await h.start();
    old.scan.resolve(invitation); await old.operation;
    h.document.change("hidden"); fresh.scan.resolve(invitation); await setImmediate();
    expect(h.state()).toMatchObject({ status: "pairing", pairingFingerprint: fingerprint });
    expect(h.approvals).toHaveLength(0); expect(h.connects()).toEqual([hostB]);
    h.document.change("visible"); await setImmediate();
    expect(h.approvals).toHaveLength(1); await h.finish(fresh.operation);
});
it.each(["cancelled", "invalid", "error", "permission"] as const)("hidden scan %s cleans up and permits a later capture", async outcome => {
    const h = await setup(), first = await h.start();
    h.document.change("hidden");
    if (outcome === "cancelled") first.scan.resolve({ status: "cancelled" });
    else if (outcome === "invalid") first.scan.resolve({ ...invitation, fingerprint: "invalid" });
    else first.scan.reject({ code: outcome === "permission" ? "permission_required" : "invalid_request", message: "Synthetic scan failure", retryable: false });
    await first.operation; await setImmediate();
    expect(h.state().status).toBe(outcome === "cancelled" ? "disconnected" : outcome === "permission" ? "permission_required" : "protocol_error");
    expect(h.state().pairingFingerprint).toBeUndefined(); expect(h.approvals).toHaveLength(0);
    h.document.change("visible");
    const second = await h.start(); second.scan.resolve(invitation); await setImmediate();
    expect(h.approvals).toHaveLength(1); await h.finish(second.operation);
});
it("ordinary hidden during approval cancels late trust selection and resumes the old host", async () => {
    const h = await setup(hostB), { operation, scan } = await h.start();
    scan.resolve(invitation); await setImmediate(); expect(h.approvals).toHaveLength(1);
    h.document.change("hidden"); await setImmediate();
    expect(h.state()).toMatchObject({ status: "disconnected", ready: false });
    expect(h.state().pairingFingerprint).toBeUndefined();
    h.approvals[0].resolve({ hostId: hostA }); await operation;
    expect(h.prefs()).toEqual({ known: [hostB], selected: hostB });
    h.document.change("visible"); await setImmediate();
    expect(h.connects()).toEqual([hostB, hostB]); expect(h.state().ready).toBe(true);
});
it("ordinary connected background clears controls and foreground confirms a new snapshot", async () => {
    const h = await setup(hostB); expect(h.state().ready).toBe(true);
    h.document.change("hidden"); await setImmediate();
    expect(h.state()).toMatchObject({ status: "disconnected", ready: false, snapshot: null, grants: null });
    expect(h.suspends()).toHaveLength(1);
    h.document.change("visible"); await setImmediate();
    expect(h.connects()).toEqual([hostB, hostB]); expect(h.state().ready).toBe(true);
});
it("ordinary connecting background rejects its late snapshot before resuming", async () => {
    const h = await setup(hostB), held = h.holdConnection();
    const connecting = controller.connectController(hostB); await setImmediate();
    h.document.change("hidden");
    held.resolve({ type: "connected", snapshot: snapshot(hostB), grants: { control: true } }); await connecting;
    expect(h.state().ready).toBe(false);
    h.document.change("visible"); await setImmediate(); expect(h.state().ready).toBe(true);
});
it("transient scan failure while hidden cannot schedule old-host network retry", async () => {
    vi.useFakeTimers();
    try {
        const h = await setup(hostB), first = await h.start();
        h.document.change("hidden");
        first.scan.reject({ code: "host_not_ready", message: "Synthetic capture failure", retryable: true });
        await first.operation;
        expect(h.state()).toMatchObject({ status: "unavailable", ready: false });
        await vi.advanceTimersByTimeAsync(9000);
        expect(h.connects()).toEqual([hostB]);
        expect(h.approvals).toHaveLength(0);
        h.document.change("visible"); await setImmediate();
        expect(h.connects()).toEqual([hostB, hostB]);
    } finally { vi.useRealTimers(); }
});