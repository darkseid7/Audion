import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import { afterEach, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import { createControllerSession, type ControllerNativeBridge } from "../application/controller/session";
import { getApplicationPort, installApplicationPort } from "../application/port";
import type { ArtworkReference, EventBatch, HostSnapshot } from "../application/types";
const image = { mime: "image/png", bytes: [137, 80, 78, 71] }, reference = { resourceId: "cover", revision: 1 };
const snapshot = (): HostSnapshot => ({ hostId: "pc", hostEpoch: "epoch", revision: 0, revisions: { libraryRevision: 1, queueRevision: 2, outputRevision: 3, settingsRevision: 0 }, playback: { status: "paused", track: null, context: null, position: 0, duration: null, volume: .5, shuffle: false, repeat: "none" }, queue: { count: 0, currentEntryId: null }, output: { kind: "pc" }, outputs: [], capabilities: { queries: [], intents: [] }, settings: {}, jobs: [] });
const cleanup: (() => void)[] = [];
afterEach(() => { cleanup.splice(0).reverse().forEach(fn => fn()); vi.useRealTimers(); vi.restoreAllMocks(); });
async function settle() { for (let i = 0; i < 40; i++)
    await Promise.resolve(); }
function deferred<T>() { let resolve!: (v: T) => void; const promise = new Promise<T>(r => resolve = r); return { promise, resolve }; }
// Execute the actual component script; only module imports and reactive scheduling
// are adapted. This exercises lifecycle/read behavior, not DOM or Android I/O.
function mount(session: ReturnType<typeof createControllerSession>) {
    const script = readFileSync(resolve("src/lib/components/ControllerArtwork.svelte"), "utf8").match(/<script lang="ts">([\s\S]*?)<\/script>/)![1];
    const ast = ts.createSourceFile("artwork.ts", script, ts.ScriptTarget.Latest, true), declarations: string[] = [], reactive: string[] = [], destroys: (() => void)[] = [];
    for (const node of ast.statements) {
        if (ts.isImportDeclaration(node))
            continue;
        if (ts.isLabeledStatement(node) && node.label.text === "$")
            reactive.push(node.statement.getText(ast));
        else
            declarations.push(node.getText(ast).replace(/^export\s+/, ""));
    }
    const code = ts.transpileModule(`(function(){${declarations.join("\n")}\nlet identity="";return {flush(state,ref){$controllerState=state;reference=ref;${reactive.join("\n")}},src(){return src;},destroy(){destroys.forEach(fn=>fn());}};})()`, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
    const component = runInNewContext(code, { getApplicationPort, AbortController, setTimeout, clearTimeout, $controllerState: get(session.state), onDestroy: (fn: () => void) => destroys.push(fn), destroys });
    cleanup.push(() => component.destroy());
    return component;
}
async function setup() { vi.useFakeTimers(); const poll = deferred<EventBatch>(), native = { beginScope: vi.fn(async () => "scope"), connect: vi.fn(async () => ({ snapshot: snapshot(), grants: { control: true } })), poll: vi.fn<ControllerNativeBridge["poll"]>(() => new Promise(() => { })), suspend: vi.fn(async () => { }), media: vi.fn<ControllerNativeBridge["media"]>(async () => image) }; native.poll.mockReturnValueOnce(poll.promise); const session = createControllerSession(native as unknown as ControllerNativeBridge); await session.connectController("pc"); cleanup.push(() => session.suspendController()); cleanup.push(installApplicationPort(session.port)); const component = mount(session); return { session, native, poll, component }; }
it("initial artwork recovers transient Busy without changing reference or reactive identity", async () => {
    const { session, native, component } = await setup();
    native.media.mockRejectedValueOnce({ code: "busy", retryable: true });
    component.flush(get(session.state), reference);
    await settle();
    expect(component.src()).toBe("");
    await vi.advanceTimersByTimeAsync(200);
    expect(component.src()).toMatch(/^blob:/);
    expect(native.media).toHaveBeenCalledTimes(2);
});
it("library replacement revokes the old Blob and recovers the current reference after Busy", async () => {
    const { session, native, poll, component } = await setup(), revoke = vi.spyOn(URL, "revokeObjectURL");
    component.flush(get(session.state), reference);
    await settle();
    const old = component.src();
    expect(old).toMatch(/^blob:/);
    poll.resolve({ hostEpoch: "epoch", revision: 1, events: [{ type: "library", revision: 1, libraryRevision: 2 }] });
    await settle();
    expect(revoke).toHaveBeenCalledWith(old);
    native.media.mockRejectedValueOnce({ code: "busy", retryable: true });
    component.flush(get(session.state), { ...reference, revision: 2 });
    await settle();
    expect(component.src()).toBe("");
    await vi.advanceTimersByTimeAsync(200);
    expect(component.src()).toMatch(/^blob:/);
    expect(component.src()).not.toBe(old);
    expect(revoke.mock.calls.filter(c => c[0] === old)).toHaveLength(1);
});
it.each(["destroy", "reference", "missing"] as const)("component aborts pending Busy replacement on %s", async (change) => {
    const { session, native, component } = await setup();
    const resolve = vi.spyOn(session.port, "resolveArtwork");
    native.media.mockRejectedValueOnce({ code: "busy", retryable: true });
    component.flush(get(session.state), reference);
    await settle();
    const signal = resolve.mock.calls[0][1];
    expect(signal).toBeInstanceOf(AbortSignal);
    if (change === "destroy")
        component.destroy();
    else
        component.flush(get(session.state), change === "missing" ? undefined : { resourceId: "new", revision: 1 });
    await settle();
    expect(signal!.aborted).toBe(true);
    expect(vi.getTimerCount()).toBe(0);
    await vi.advanceTimersByTimeAsync(5000);
    expect(native.media).toHaveBeenCalledTimes(change === "reference" ? 2 : 1);
    if (change === "reference")
        expect(component.src()).toMatch(/^blob:/);
    else
        expect(component.src()).toBe("");
});
it("destroy cancels in-flight artwork and never adopts its late Blob", async () => {
    const { session, native, component } = await setup(), held = deferred<typeof image>(), resolve = vi.spyOn(session.port, "resolveArtwork"), revoke = vi.spyOn(URL, "revokeObjectURL");
    native.media.mockReturnValue(held.promise);
    component.flush(get(session.state), reference);
    await settle();
    const signal = resolve.mock.calls[0][1];
    expect(signal).toBeInstanceOf(AbortSignal);
    component.destroy();
    expect(signal!.aborted).toBe(true);
    held.resolve(image);
    await settle();
    expect(component.src()).toBe("");
    expect(revoke).toHaveBeenCalledOnce();
});
it("missing artwork remains display-only and exhausted Busy never loops indefinitely", async () => {
    const { session, native, component } = await setup();
    component.flush(get(session.state), undefined);
    await settle();
    expect(native.media).not.toHaveBeenCalled();
    native.media.mockRejectedValue({ code: "busy", retryable: true });
    component.flush(get(session.state), reference);
    await vi.advanceTimersByTimeAsync(1700);
    expect(component.src()).toBe("");
    expect(native.media).toHaveBeenCalledTimes(4);
    component.flush(get(session.state), { ...reference });
    await vi.advanceTimersByTimeAsync(30000);
    expect(native.media).toHaveBeenCalledTimes(4);
    expect(get(session.state).ready).toBe(true);
});
