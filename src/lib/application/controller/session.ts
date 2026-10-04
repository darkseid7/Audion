import { writable, readonly } from "svelte/store";
import { parseEnvelope } from "../protocol";
import type { ApplicationPort, ApplicationQuery, ApplicationUpdate, ArtworkHandle, ArtworkReference, CommandEnvelope, ExecutionResult, EventBatch, EventCursor, HostSnapshot, QueryResult, ControlError } from "../types";
import { createArtworkHandle, type NativeImage } from "./media";
export interface NativeControllerFence {
    scopeId: string;
    generation: number;
}
export interface AuthenticatedConnection { snapshot: HostSnapshot; grants: { control: boolean } }
export interface ControllerNativeBridge {
    beginScope(): Promise<string>;
    connect(hostId: string, fence: NativeControllerFence): Promise<AuthenticatedConnection>;
    suspend(fence: NativeControllerFence): Promise<void>;
    forget(hostId: string, fence: NativeControllerFence): Promise<void>;
    scan(fence: NativeControllerFence): Promise<{
        status: "cancelled" | "invitation_ready";
        hostId?: string;
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
}
const failure = (code: ControlError["code"], message = "Controller session is unavailable."): ControlError => ({ code, message, retryable: code === "host_not_ready" });
export function createControllerSession(native: ControllerNativeBridge, options: {
    random?: () => number;
} = {}) {
    let value: ControllerState = { currentHostId: null, snapshot: null, grants: null, ready: false, status: "disconnected" };
    const state = writable(value), listeners = new Set<(update: ApplicationUpdate) => void>(), media = new Set<ArtworkHandle>();
    let generation = 0, scope: Promise<string> | undefined, timer: ReturnType<typeof setTimeout> | undefined, attempts = 0;
    const publish = (patch: Partial<ControllerState>) => {
        value = { ...value, ...patch };
        state.set(value);
    };
    const invalidate = () => {
        generation++;
        if (timer)
            clearTimeout(timer);
        timer = undefined;
        for (const h of media)
            h.dispose();
        media.clear();
        publish({ ready: false, snapshot: null, grants: null });
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
    const emit = (update: ApplicationUpdate) => {
        for (const listener of listeners)
            listener(structuredClone(update));
    };
    function disconnected(error: unknown, g: number) {
        if (g !== generation)
            return;
        const e = (error && typeof error === "object" && "code" in error ? error : failure("host_not_ready")) as ControlError;
        const terminal = e.code === "unauthorized" || e.code === "permission_required" || e.code === "unsupported" || e.code === "invalid_request";
        const host = value.currentHostId;
        const next = invalidate();
        publish({ status: e.code === "unauthorized" ? "pairing_required" : e.code === "permission_required" ? "permission_required" : terminal ? "protocol_error" : "unavailable", error: e });
        void fence(next).then(f => native.suspend(f)).catch(() => {
        });
        if (!terminal && host) {
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
        while (g === generation && value.ready && value.snapshot) {
            const before = value.snapshot;
            try {
                const batch = await native.poll(f, { hostEpoch: before.hostEpoch, revision: before.revision });
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
    function suspendController(): void {
        const g = invalidate();
        publish({ status: "disconnected", error: undefined });
        if (scope)
            void fence(g).then(f => native.suspend(f)).catch(() => {
            });
    }
    async function forgetController(hostId: string): Promise<void> {
        const g = invalidate();
        publish({ currentHostId: null, status: "disconnected" });
        await native.forget(hostId, await fence(g));
    }
    async function pairController(onPaired: (hostId: string) => void = () => {}): Promise<ControllerConnectionReceipt | undefined> {
        const g = invalidate();
        publish({ status: "pairing" });
        try {
            const f = await fence(g);
            const input = await native.scan(f);
            current(g);
            if (input.status === "cancelled") {
                suspendController();
                return;
            }
            const paired = await native.pair(f, "Android controller");
            current(g);
            // Catalog membership is committed before connection, independently of selection.
            onPaired(paired.hostId);
            current(g);
            return await connectController(paired.hostId);
        }
        catch (error) {
            disconnected(error, g);
            return;
        }
    }
    const port: ApplicationPort = {
        async query(query, signal) {
            const { g, snapshot } = ready();
            if (signal?.aborted)
                throw failure("resync_required");
            const result = await native.query(await fence(g), query);
            current(g, snapshot.hostEpoch);
            if (signal?.aborted)
                throw failure("resync_required");
            if (result.type !== query.type)
                throw failure("unsupported");
            return result;
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
                const result = await native.commandStatus(f, envelope.requestId);
                current(g, snapshot.hostEpoch);
                if (result.status === "pending")
                    throw failure("outcome_unknown", "The PC has not confirmed this command. It was not replayed.");
                return result;
            }
        },
        subscribe(listener) {
            listeners.add(listener);
            if (value.snapshot)
                listener({ type: "snapshot", snapshot: structuredClone(value.snapshot) });
            return () => listeners.delete(listener);
        },
        async resolveArtwork(reference, signal) {
            const { g, snapshot } = ready();
            if (signal?.aborted)
                throw failure("resync_required");
            const image = await native.media(await fence(g), reference);
            const handle = createArtworkHandle(image);
            try {
                current(g, snapshot.hostEpoch);
                if (value.snapshot?.revisions.libraryRevision !== snapshot.revisions.libraryRevision)
                    throw failure("resync_required");
                if (signal?.aborted)
                    throw failure("resync_required");
            }
            catch (error) {
                handle.dispose();
                throw error;
            }
            const owned = { src: handle.src, dispose() {
                    handle.dispose();
                    media.delete(owned);
                } };
            media.add(owned);
            return owned;
        },
    };
    return {
        port, state: readonly(state), initialize: async () => {
            await fence(generation);
        }, connectController, suspendController, forgetController, pairController
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
