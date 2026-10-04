import { writable, readonly } from "svelte/store";
import { parseEnvelope } from "../protocol";
import { parseBrowseMetadataQuery, parseBrowseMetadataResult } from "../browse-metadata";
import type { BrowseMetadataRequest, BrowseMetadataResult } from "../types";
import type { ApplicationPort, ApplicationQuery, ApplicationUpdate, ArtworkHandle, ArtworkReference, CommandEnvelope, ExecutionResult, EventBatch, EventCursor, HostSnapshot, QueryResult, ControlError } from "../types";
import { createArtworkHandle, type NativeImage } from "./media";
import { createReadAdmission, recoverRead } from "./read-recovery";
export interface NativeControllerFence {
    scopeId: string;
    generation: number;
}
export interface AuthenticatedConnection { snapshot: HostSnapshot; grants: { control: boolean } }
export interface ControllerNativeBridge {
    browseMetadata(fence: NativeControllerFence, request: BrowseMetadataRequest): Promise<BrowseMetadataResult>;
    beginScope(): Promise<string>;
    connect(hostId: string, fence: NativeControllerFence): Promise<AuthenticatedConnection>;
    updateEndpoint(hostId: string, endpoint: string, fence: NativeControllerFence): Promise<AuthenticatedConnection>;
    suspend(fence: NativeControllerFence): Promise<void>;
    forget(hostId: string, fence: NativeControllerFence): Promise<void>;
    scan(fence: NativeControllerFence): Promise<{
        status: "cancelled" | "invitation_ready";
        hostId?: string;
        fingerprint?: string;
    }>;
    pair(fence: NativeControllerFence, deviceName: string): Promise<{
        hostId: string;
    }>;
    query(fence: NativeControllerFence, query: ApplicationQuery): Promise<QueryResult>;
    command(fence: NativeControllerFence, envelope: CommandEnvelope): Promise<ExecutionResult>;
    commandStatus(fence: NativeControllerFence, requestId: string): Promise<ExecutionResult | {
        status: "pending";
    }>;
    poll(fence: NativeControllerFence, cursor: EventCursor): Promise<EventBatch>;
    media(fence: NativeControllerFence, reference: ArtworkReference): Promise<NativeImage>;
}
interface ControllerConnectionReceipt {
    readonly hostId: string;
    isCurrent(): boolean;
}
export interface ControllerState {
    currentHostId: string | null;
    snapshot: HostSnapshot | null;
    grants: { control: boolean } | null;
    ready: boolean;
    status: "disconnected" | "connecting" | "connected" | "unavailable" | "pairing" | "pairing_required" | "permission_required" | "protocol_error";
    error?: ControlError;
    pairingFingerprint?: string;
}
const failure = (code: ControlError["code"], message = "Controller session is unavailable."): ControlError => ({ code, message, retryable: code === "host_not_ready" });
/** Session-owned, memory-only LRU. Values are private copies, never live capabilities. */
function navigationCache<T>(maxEntries: number, maxBytes: number) {
    const entries = new Map<string, { value: T; bytes: number }>();
    let bytes = 0;
    return {
        get(key: string): T | undefined {
            const entry = entries.get(key);
            if (!entry) return;
            entries.delete(key); entries.set(key, entry); return entry.value;
        },
        set(key: string, value: T, weight: number) {
            const old = entries.get(key); if (old) { bytes -= old.bytes; entries.delete(key); }
            if (weight > maxBytes) return;
            entries.set(key, { value, bytes: weight }); bytes += weight;
            while (entries.size > maxEntries || bytes > maxBytes) {
                const oldest = entries.keys().next().value!;
                bytes -= entries.get(oldest)!.bytes; entries.delete(oldest);
            }
        },
        clear() { entries.clear(); bytes = 0; }
    };
}
export function createControllerSession(native: ControllerNativeBridge, options: {
    random?: () => number;
} = {}) {
    let value: ControllerState = { currentHostId: null, snapshot: null, grants: null, ready: false, status: "disconnected" };
    const state = writable(value), listeners = new Set<(update: ApplicationUpdate) => void>(), media = new Set<ArtworkHandle>();
    const pages = navigationCache<QueryResult>(8, 2 * 1024 * 1024);
    const images = navigationCache<NativeImage>(192, 24 * 1024 * 1024);
    let generation = 0, scope: Promise<string> | undefined, timer: ReturnType<typeof setTimeout> | undefined, attempts = 0;
    let foreground = true;
    let lifecycle = new AbortController();
    const admitRead = createReadAdmission();
    type ReadRevision = "libraryRevision" | "queueRevision" | "outputRevision";
    const reads = new Set<{ revision: ReadRevision; cancel: AbortController }>();
    let pairing: { generation: number; phase: "capture" | "awaiting-foreground" | "approval"; wake?: () => void } | undefined;
    const publish = (patch: Partial<ControllerState>) => {
        if (patch.snapshot && value.snapshot) {
            for (const read of reads)
                if (patch.snapshot.revisions[read.revision] !== value.snapshot.revisions[read.revision]) read.cancel.abort();
        }
        value = { ...value, ...patch };
        state.set(value);
    };
    const invalidate = () => {
        generation++;
        lifecycle.abort();
        lifecycle = new AbortController();
        const cancelled = pairing;
        pairing = undefined;
        cancelled?.wake?.();
        if (timer)
            clearTimeout(timer);
        timer = undefined;
        for (const h of media)
            h.dispose();
        media.clear();
        pages.clear(); images.clear();
        publish({ ready: false, snapshot: null, grants: null, pairingFingerprint: undefined });
        return generation;
    };
    const fence = async (g: number) => {
        scope ??= native.beginScope();
        const scopeId = await scope;
        if (g !== generation)
            throw failure("resync_required");
        return { scopeId, generation: g };
    };
    const current = (g: number, epoch?: string) => {
        if (g !== generation || (epoch !== undefined && epoch !== value.snapshot?.hostEpoch))
            throw failure("resync_required");
    };
    const ready = () => {
        if (!value.ready || !value.snapshot)
            throw failure("host_not_ready");
        return { g: generation, snapshot: value.snapshot };
    };
    async function read<T>(revision: ReadRevision, signal: AbortSignal | undefined, operation: (f: NativeControllerFence, check: () => void) => Promise<T>, discard?: (result: T) => void, admission: "foreground" | "optional" = "foreground"): Promise<T> {
        const { g, snapshot } = ready();
        const owner = lifecycle.signal, cancel = new AbortController();
        const abort = () => cancel.abort();
        const entry = { revision, cancel };
        owner.addEventListener("abort", abort, { once: true });
        signal?.addEventListener("abort", abort, { once: true });
        if (owner.aborted || signal?.aborted) abort();
        reads.add(entry);
        const check = () => {
            current(g, snapshot.hostEpoch);
            if (cancel.signal.aborted || !value.ready || value.snapshot?.hostId !== snapshot.hostId || value.snapshot.revisions[revision] !== snapshot.revisions[revision])
                throw failure("resync_required");
        };
        try {
            const result = await recoverRead(cancel.signal, () => (admission === "optional" ? admitRead.tryRun : admitRead)(cancel.signal, async () => {
                check();
                const f = await fence(g);
                // Scope resolution and admission are asynchronous; check at native entry.
                check();
                const result = await operation(f, check);
                try { check(); }
                catch (error) { discard?.(result); throw error; }
                return result;
            }), discard);
            try { check(); }
            catch (error) { discard?.(result); throw error; }
            return result;
        } finally {
            reads.delete(entry);
            owner.removeEventListener("abort", abort);
            signal?.removeEventListener("abort", abort);
        }
    }
    const emit = (update: ApplicationUpdate) => {
        for (const listener of listeners)
            listener(structuredClone(update));
    };
    function disconnected(error: unknown, g: number, retry = true) {
        if (g !== generation)
            return;
        const e = (error && typeof error === "object" && "code" in error ? error : failure("host_not_ready")) as ControlError;
        const terminal = e.code === "unauthorized" || e.code === "permission_required" || e.code === "unsupported" || e.code === "invalid_request";
        const host = value.currentHostId;
        const next = invalidate();
        publish({ status: e.code === "unauthorized" ? "pairing_required" : e.code === "permission_required" ? "permission_required" : terminal ? "protocol_error" : "unavailable", error: e });
        void fence(next).then(f => native.suspend(f)).catch(() => {
        });
        if (retry && !terminal && host) {
            const delay = Math.min(8000, 1000 * 2 ** Math.min(attempts++, 3)) * (0.8 + 0.2 * (options.random ?? Math.random)());
            timer = setTimeout(() => {
                timer = undefined;
                if (generation === next)
                    void connectController(host, false);
            }, delay);
        }
    }
    function adopt(receipt: AuthenticatedConnection, host: string, g: number) {
        if (!receipt?.snapshot || typeof receipt.grants?.control !== "boolean") throw failure("unsupported", "Missing authenticated permissions.");
        const { snapshot, grants } = receipt;
        current(g);
        if (snapshot.hostId !== host)
            throw failure("unsupported", "Unexpected PC identity.");
        publish({
            snapshot: structuredClone(snapshot), grants: { control: grants.control }, currentHostId: host, ready: true, status: "connected", error: undefined
        });
        attempts = 0;
        emit({ type: "snapshot", snapshot });
    }
    async function polling(g: number, f: NativeControllerFence) {
        const signal = lifecycle.signal;
        while (g === generation && value.ready && value.snapshot) {
            const before = value.snapshot;
            try {
                const batch = await recoverRead(signal, () => {
                    current(g, before.hostEpoch);
                    if (!value.ready) throw failure("resync_required");
                    return native.poll(f, { hostEpoch: before.hostEpoch, revision: before.revision });
                });
                current(g, before.hostEpoch);
                if (batch.hostEpoch !== before.hostEpoch || batch.events.length > 256 || batch.revision !== before.revision + batch.events.length || batch.events.some((e, i) => e.revision !== before.revision + i + 1)) {
                    await connectController(value.currentHostId!, false);
                    return;
                }
                const next = structuredClone(before);
                for (const event of batch.events) {
                    next.revision = event.revision;
                    switch (event.type) {
                        case "playback":
                            next.playback = event.playback;
                            break;
                        case "queue":
                            next.queue = event.queue;
                            next.revisions.queueRevision = event.queueRevision;
                            break;
                        case "library":
                            next.revisions.libraryRevision = event.libraryRevision;
                            pages.clear(); images.clear();
                            for (const h of media)
                                h.dispose();
                            media.clear();
                            break;
                        case "output":
                            next.output = event.output;
                            next.outputs = event.outputs;
                            next.revisions.outputRevision = event.outputRevision;
                            break;
                        case "settings":
                            next.settings = event.settings;
                            next.revisions.settingsRevision = event.settingsRevision;
                            break;
                        case "capabilities":
                            next.capabilities = event.capabilities;
                            break;
                        case "job":
                            next.jobs = [...next.jobs.filter(j => j.jobId !== event.job.jobId), event.job];
                            break;
                    }
                }
                publish({ snapshot: next });
                if (batch.events.length)
                    emit({ type: "events", hostEpoch: batch.hostEpoch, events: batch.events });
            }
            catch (error) {
                if (g !== generation)
                    return;
                if ((error as ControlError)?.code === "resync_required") {
                    await connectController(value.currentHostId!, false);
                    return;
                }
                disconnected(error, g);
                return;
            }
        }
    }
    async function connectController(hostId: string, reset = true): Promise<ControllerConnectionReceipt | undefined> {
        const g = invalidate();
        if (reset)
            attempts = 0;
        publish({ currentHostId: hostId, status: "connecting", error: undefined });
        try {
            const f = await fence(g);
            const snapshot = await native.connect(hostId, f);
            adopt(snapshot, hostId, g);
            void polling(g, f);
            // The consumer must check ownership again at its eventual side effect.
            return { hostId, isCurrent: () => g === generation };
        }
        catch (error) {
            disconnected(error, g);
        }
    }
    async function updateEndpoint(hostId: string, endpoint: string): Promise<ControllerConnectionReceipt | undefined> {
        const g = invalidate();
        publish({ currentHostId: hostId, status: "connecting", error: undefined });
        try {
            const f = await fence(g);
            const receipt = await native.updateEndpoint(hostId, endpoint, f);
            adopt(receipt, hostId, g);
            void polling(g, f);
            return { hostId, isCurrent: () => g === generation };
        } catch (error) {
            // Never reconnect to the old endpoint and imply the edit succeeded.
            disconnected(error, g, false);
        }
    }
    function suspendController(): void {
        const g = invalidate();
        publish({ status: "disconnected", error: undefined });
        if (scope)
            void fence(g).then(f => native.suspend(f)).catch(() => {
            });
    }
    // Visibility preserves only generation-owned local capture/validation.
    // Explicit suspend remains cancellation, including during capture.
    function visibilityChanged(visible: boolean, resumeHostId: string | null): void {
        foreground = visible;
        if (pairing?.generation === generation && pairing.phase !== "approval") {
            if (visible) pairing.wake?.();
            return;
        }
        if (!visible) suspendController();
        else if (!pairing && resumeHostId) void connectController(resumeHostId);
    }
    async function forgetController(hostId: string): Promise<void> {
        const g = invalidate();
        publish({ currentHostId: null, status: "disconnected" });
        await native.forget(hostId, await fence(g));
    }
    async function pairController(onPaired: (hostId: string) => void = () => {}): Promise<ControllerConnectionReceipt | undefined> {
        const g = invalidate();
        const operation: NonNullable<typeof pairing> = { generation: g, phase: "capture" };
        pairing = operation;
        publish({ status: "pairing", error: undefined });
        try {
            const f = await fence(g);
            const input = await native.scan(f);
            current(g);
            if (input.status === "cancelled") {
                suspendController();
                return;
            }
            if (!input.fingerprint || !/^[0-9a-f]{64}$/.test(input.fingerprint)) {
                throw failure("invalid_request", "Invalid invitation fingerprint. Create a new invitation on the PC.");
            }
            publish({ pairingFingerprint: input.fingerprint });
            // Publishing is synchronous: subscribers can hide, cancel or replace us.
            current(g);
            while (!foreground) {
                operation.phase = "awaiting-foreground";
                await new Promise<void>(resolve => { operation.wake = resolve; });
                operation.wake = undefined;
                current(g);
            }
            current(g);
            // No await/publication between the foreground check and native entry.
            operation.phase = "approval";
            const paired = await native.pair(f, "Android controller");
            current(g);
            // Catalog membership is committed before connection, independently of selection.
            onPaired(paired.hostId);
            current(g);
            return await connectController(paired.hostId);
        }
        catch (error) {
            // A local capture failure must not start old-host network retry hidden.
            disconnected(error, g, foreground);
            return;
        }
        finally {
            if (pairing === operation) pairing = undefined;
        }
    }
    const port: ApplicationPort = {
        async queryBrowseMetadata(query, signal) {
            const input = parseBrowseMetadataQuery(query);
            const { snapshot } = ready();
            const request: BrowseMetadataRequest = { ...input, metadataVersion: 1, hostEpoch: snapshot.hostEpoch, libraryRevision: snapshot.revisions.libraryRevision };
            let settlement: Promise<unknown> | undefined;
            try {
                return await read("libraryRevision", signal, async f => {
                    const pending = native.browseMetadata(f, request);
                    settlement = pending.catch(() => undefined);
                    return parseBrowseMetadataResult(await pending, request);
                }, undefined, "optional");
            } finally {
                // Owner coalescing must track native completion, not just consumer abort.
                // Foreground reads still keep the other lane and cancel promptly.
                await settlement;
            }
        },
        async query(query, signal, options) {
            const revision = query.type === "queue" ? "queueRevision" : query.type === "outputs" ? "outputRevision" : "libraryRevision";
            return read(revision, signal, async (f, check) => {
                // Snapshot, queue and outputs contain state outside the library domain.
                const cacheable = query.type !== "snapshot" && query.type !== "queue" && query.type !== "outputs";
                const key = JSON.stringify(query), cached = cacheable && !options?.refresh ? pages.get(key) : undefined;
                if (cached) return structuredClone(cached);
                const result = await native.query(f, query);
                check();
                if (result.type !== query.type) throw failure("unsupported");
                const confirmed = result.type === "album_detail" ? result.revision : "page" in result ? result.page.revision : undefined;
                if (cacheable && confirmed === value.snapshot?.revisions.libraryRevision) {
                    const copy = structuredClone(result);
                    pages.set(key, copy, JSON.stringify(copy).length * 2 + key.length * 2);
                    return structuredClone(copy);
                }
                return result;
            });
        },
        async execute(intent, preconditions) {
            const { g, snapshot } = ready();
            if (!value.grants?.control) throw failure("permission_required", "This controller does not have playback permission.");
            if (preconditions.hostEpoch !== snapshot.hostEpoch)
                throw failure("resync_required");
            for (const key of ["libraryRevision", "queueRevision", "outputRevision"] as const)
                if (preconditions[key] !== undefined && preconditions[key] !== snapshot.revisions[key])
                    throw failure("revision_conflict");
            const envelope = parseEnvelope({ protocolVersion: 1, requestId: crypto.randomUUID(), preconditions, intent });
            const f = await fence(g);
            current(g, snapshot.hostEpoch);
            try {
                const result = await native.command(f, envelope);
                current(g, snapshot.hostEpoch);
                if ((result.status === "failed" || result.status === "superseded") && ["unauthorized", "permission_required"].includes(result.error.code)) disconnected(result.error,g);
                return result;
            }
            catch (error) {
                current(g, snapshot.hostEpoch);
                if ((error as ControlError)?.code !== "outcome_unknown") {
                    if (["unauthorized", "permission_required"].includes((error as ControlError)?.code)) disconnected(error, g);
                    throw error;
                }
                try {
                    const result = await native.commandStatus(f, envelope.requestId);
                    current(g, snapshot.hostEpoch);
                    if (result.status === "pending")
                        throw failure("outcome_unknown", "The PC has not confirmed this command. It was not replayed.");
                    if ((result.status === "failed" || result.status === "superseded") && ["unauthorized", "permission_required"].includes(result.error.code)) {
                        disconnected(result.error, g);
                    }
                    return result;
                } catch (statusError) {
                    current(g, snapshot.hostEpoch);
                    if (["unauthorized", "permission_required"].includes((statusError as ControlError)?.code)) {
                        disconnected(statusError, g);
                    }
                    throw statusError;
                }
            }
        },
        subscribe(listener) {
            listeners.add(listener);
            if (value.snapshot)
                listener({ type: "snapshot", snapshot: structuredClone(value.snapshot) });
            return () => listeners.delete(listener);
        },
        async resolveArtwork(reference, signal) {
            return read("libraryRevision", signal, async (f, check) => {
                // Presentation identity must never cross the native capability boundary.
                const request = { resourceId: reference.resourceId, revision: reference.revision };
                const key = JSON.stringify(request), cached = request.revision === value.snapshot?.revisions.libraryRevision ? images.get(key) : undefined;
                const image = cached ?? await native.media(f, request);
                const handle = createArtworkHandle(image);
                try { check(); }
                catch (error) { handle.dispose(); throw error; }
                if (!cached && reference.revision === value.snapshot?.revisions.libraryRevision) {
                    images.set(key, { mime: image.mime, bytes: new Uint8Array(image.bytes) }, image.bytes.length + key.length * 2);
                }
                const owned = { src: handle.src, dispose() {
                    handle.dispose();
                    media.delete(owned);
                } };
                media.add(owned);
                return owned;
            }, handle => handle.dispose());
        },
    };
    return {
        port, state: readonly(state), initialize: async () => {
            await fence(generation);
        }, connectController, updateEndpoint, suspendController, visibilityChanged, forgetController, pairController
    };
}
const sessions = new WeakMap<ControllerNativeBridge, ReturnType<typeof createControllerSession>>();
export function controllerSession(native: ControllerNativeBridge) {
    let session = sessions.get(native);
    if (!session) {
        session = createControllerSession(native);
        sessions.set(native, session);
    }
    return session;
}
