import { invoke } from "@tauri-apps/api/core";
import { writable, readonly } from "svelte/store";
import type { ApplicationHandle } from "../bootstrap";
import type { HostSnapshot, QueryResult, ExecutionResult, EventBatch, BrowseMetadataResult } from "../types";
import { controllerSession, type ControllerNativeBridge, type ControllerState, type NativeControllerFence } from "./session";
import { createControllerAdapter } from "./adapter";
import type { NativeImage } from "./media";
function nativeBridge(): ControllerNativeBridge {
    async function request<T>(fence: NativeControllerFence, request: object, type: string, key: string): Promise<T> {
        const reply = await invoke<Record<string, unknown>>("controller_request", { fence, request });
        if (reply.type !== type)
            throw { code: "unsupported", message: "Incompatible native controller protocol.", retryable: false };
        return reply[key] as T;
    }
    return {
        async beginScope() {
            const reply = await invoke<{
                type: string;
                scopeId: string;
            }>("controller_connection", { request: { type: "begin_scope" } });
            if (reply.type !== "scope" || !reply.scopeId)
                throw new Error("Invalid native scope");
            return reply.scopeId;
        },
        async connect(hostId, fence) {
            const reply = await invoke<{
                type: string;
                snapshot: HostSnapshot;
                grants: { control: boolean };
            }>("controller_connection", { request: { type: "connect", hostId, fence } });
            if (reply.type !== "connected")
                throw new Error("Invalid native session");
            return { snapshot: reply.snapshot, grants: reply.grants };
        },
        async updateEndpoint(hostId, endpoint, fence) {
            const reply = await invoke<{ type: string; snapshot: HostSnapshot; grants: { control: boolean } }>(
                "controller_connection", { request: { type: "update_endpoint", hostId, endpoint, fence } });
            if (reply.type !== "connected") throw new Error("Invalid native session");
            return { snapshot: reply.snapshot, grants: reply.grants };
        },
        suspend: fence => invoke("controller_suspend", { fence }), forget: (hostId, fence) => invoke("controller_forget", { hostId, fence }),
        scan: fence => invoke("controller_scan_pair", { fence, invitation: null }), pair: (fence, deviceName) => invoke("controller_pair", { fence, deviceName }),
        query: (fence, query) => request<QueryResult>(fence, { type: "query", query }, "query", "result"),
        browseMetadata: (fence, input) => request<BrowseMetadataResult>(fence, { type: "browse_metadata", request: input }, "browse_metadata", "result"),
        command: (fence, envelope) => request<ExecutionResult>(fence, { type: "command", envelope }, "command", "result"),
        commandStatus: (fence, requestId) => request<ExecutionResult | {
            status: "pending";
        }>(fence, { type: "command_status", requestId }, "command_status", "result"),
        poll: (fence, cursor) => request<EventBatch>(fence, { type: "poll", cursor }, "poll", "batch"),
        media: (fence, reference) => request<NativeImage>(fence, { type: "media", reference }, "media", "image"),
    };
}
const empty: ControllerState = { currentHostId: null, snapshot: null, grants: null, ready: false, status: "disconnected" };
const state = writable<ControllerState>(empty);
export const controllerState = readonly(state);
const hosts = writable<string[]>([]);
export const pairedHostIds = readonly(hosts);
let active: ReturnType<typeof controllerSession> | undefined;
let known: string[] = [];
let selected: string | null = null;
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
function savePreferences() {
    try {
        localStorage.setItem("audion_controller_hosts", JSON.stringify({ known, selected }));
    }
    catch {
    }
    hosts.set([...known]);
}
export async function connectController(hostId: string): Promise<void> {
    if (!uuid.test(hostId) || !active)
        return;
    selected = hostId;
    savePreferences();
    await active.connectController(hostId);
}
export async function updateControllerEndpoint(hostId: string, endpoint: string): Promise<void> {
    const session = active;
    if (!session || !known.includes(hostId)) return;
    const receipt = await session.updateEndpoint(hostId, endpoint);
    if (active !== session || !receipt?.isCurrent()) return;
    selected = hostId;
    savePreferences();
}
export function suspendController(): void {
    active?.suspendController();
}
export async function forgetController(hostId: string): Promise<void> {
    if (!active)
        return;
    await active.forgetController(hostId);
    known = known.filter(h => h !== hostId);
    if (selected === hostId)
        selected = null;
    savePreferences();
}
export async function pairController(): Promise<void> {
    const session = active;
    if (!session)
        return;
    const receipt = await session.pairController(host => {
        if (active !== session)
            return;
        const candidates = [host, ...known.filter(h => h !== host)];
        known = candidates.slice(0, 32);
        // A newly persisted pairing must not evict the selected resume target
        // while its follow-up connection can still fail or be cancelled.
        if (selected && candidates.includes(selected) && !known.includes(selected)) {
            known[known.length - 1] = selected;
        }
        savePreferences();
    });
    if (active !== session || !receipt?.isCurrent())
        return;
    selected = receipt.hostId;
    savePreferences();
}
/** Only local UI preferences and a native scope: no music DB, engine or queue restoration. */
export async function bootstrapController(): Promise<ApplicationHandle> {
    const native = nativeBridge(), session = controllerSession(native);
    active?.suspendController();
    active = session;
    const unsubscribe = session.state.subscribe(value => {
        if (active === session)
            state.set(value);
    });
    try {
        await session.initialize();
        if (active !== session) throw new Error("Controller bootstrap superseded");
    }
    catch (error) {
        unsubscribe();
        session.suspendController();
        if (active === session)
            active = undefined;
        throw error;
    }
    known = [];
    selected = null;
    try {
        const prefs = JSON.parse(localStorage.getItem("audion_controller_hosts") ?? "null");
        known = Array.isArray(prefs?.known) ? prefs.known.filter((h: unknown) => typeof h === "string" && uuid.test(h)).slice(0, 32) : [];
        selected = known.includes(prefs?.selected) ? prefs.selected : null;
    }
    catch {
    }
    hosts.set([...known]);
    const visibility = () => {
        if (active !== session)
            return;
        session.visibilityChanged(typeof document === "undefined" || document.visibilityState !== "hidden", selected);
    };
    if (typeof document !== "undefined")
        document.addEventListener("visibilitychange", visibility);
    visibility();
    return { port: createControllerAdapter(native), async dispose() {
            unsubscribe();
            if (typeof document !== "undefined")
                document.removeEventListener("visibilitychange", visibility);
            session.suspendController();
            if (active === session) {
                active = undefined;
                state.set(empty);
            }
        } };
}
