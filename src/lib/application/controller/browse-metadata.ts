import { writable, readonly, type Readable } from "svelte/store";
import { controllerState } from "./bootstrap";
import { getApplicationPort } from "../port";
import type { ControllerState } from "./session";
import type { ApplicationPort, BrowseMetadataQuery, AlbumDuration, ControlError } from "../types";
export interface BrowseMetadataState { counts: ReadonlyMap<number, number | null>; album: AlbumDuration | null; loading: boolean; stale: boolean; error: string }
const empty = (): BrowseMetadataState => ({ counts: new Map(), album: null, loading: false, stale: false, error: "" });
export function createControllerBrowseMetadata(port: () => ApplicationPort = getApplicationPort, connection: Readable<ControllerState> = controllerState, visible?: Readable<boolean>) {
    let value = empty(), scope = "", query: BrowseMetadataQuery = { trackIds: [] }, queryKey = "", generation = 0, disposed = false;
    let foreground = false, confirmed = false, cancel: AbortController | undefined, timer: ReturnType<typeof setTimeout> | undefined;
    // A cancelled consumer does not free native admission. Session owns that lease.
    let pending = false, wake = false;
    const state = writable(value);
    const publish = (patch: Partial<BrowseMetadataState>) => { value = { ...value, ...patch }; state.set(value); };
    function stop() { generation++; cancel?.abort(); if (timer) clearTimeout(timer); timer = undefined; }
    function active() { return !disposed && foreground && confirmed && !!(query.trackIds.length || query.albumId); }
    function schedule() { if (active()) timer = setTimeout(() => { timer = undefined; void read(); }, 5000); }
    async function read() {
        if (!active()) return;
        if (pending) { wake = true; return; }
        pending = true; wake = false;
        const g = generation, controller = new AbortController(); cancel = controller;
        publish({ loading: true, error: "" });
        try {
            const api = port();
            if (!api.queryBrowseMetadata) throw { code: "unsupported", message: "Update the PC and APK together to read metadata.", retryable: false };
            const result = await api.queryBrowseMetadata(query, controller.signal);
            if (g !== generation || controller.signal.aborted || !active()) return;
            publish({ counts: new Map(result.tracks.map(t => [t.trackId, t.playCount])), album: result.album ?? null, stale: false, error: "" });
        } catch (error) {
            if (g !== generation || controller.signal.aborted || !active()) return;
            const e = (error ?? {}) as Partial<ControlError>;
            publish({ stale: value.counts.size > 0 || value.album !== null, error: e.code === "busy" ? "" : e.code === "not_found" || e.code === "unsupported" ? "PC metadata is unavailable. Update the PC and APK together if their builds differ." : "Could not read PC metadata." });
        } finally {
            pending = false;
            if (g === generation) publish({ loading: false });
            if (wake && active()) { wake = false; void read(); }
            else schedule();
        }
    }
    const unsubscribe = connection.subscribe(c => {
        const next = c.ready && c.snapshot ? JSON.stringify([c.snapshot.hostId, c.snapshot.hostEpoch, c.snapshot.revisions.libraryRevision, c.grants?.control]) : "";
        confirmed = !!next;
        if (next === scope) return;
        scope = next; stop(); query = { trackIds: [] }; queryKey = ""; value = empty(); state.set(value);
        // A new scope must receive IDs from its newly confirmed catalog page.
    });
    const visibility = visible ?? { subscribe(run: (v: boolean) => void) {
        if (typeof document === "undefined") { run(false); return () => {}; }
        const changed = () => run(document.visibilityState !== "hidden");
        changed(); document.addEventListener("visibilitychange", changed);
        return () => document.removeEventListener("visibilitychange", changed);
    } };
    const unvisible = visibility.subscribe(v => {
        if (foreground === v) return; foreground = v; stop();
        publish({ loading: false, stale: value.counts.size > 0 || value.album !== null });
        if (v) void read();
    });
    return { state: readonly(state), setQuery(input: BrowseMetadataQuery) {
        if (disposed) return;
        const ids = [...new Set(input.trackIds.filter(id => Number.isSafeInteger(id) && id > 0))].slice(0, 1000);
        const next = { trackIds: ids, ...(Number.isSafeInteger(input.albumId) && input.albumId! > 0 ? { albumId: input.albumId } : {}) };
        const key = JSON.stringify(next); if (key === queryKey) return;
        stop(); query = next; queryKey = key;
        const counts = new Map([...value.counts].filter(([id]) => ids.includes(id)));
        const album = value.album?.albumId === query.albumId ? value.album : null;
        publish({ counts, album, loading: false, stale: value.stale && (counts.size > 0 || album !== null), error: "" });
        void read();
    }, dispose() { if (disposed) return; disposed = true; stop(); unsubscribe(); unvisible(); value = empty(); state.set(value); } };
}
