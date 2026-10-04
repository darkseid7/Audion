import { get, writable, readonly, type Readable } from "svelte/store";
import { controllerState } from "./bootstrap";
import { getApplicationPort } from "../port";
import type { ControllerState } from "./session";
import type { ApplicationPort, ApplicationQuery, DisplayTrack, DisplayAlbum, DisplayArtist, DisplayPlaylist, AlbumDetail, QueueEntry, SearchMatch } from "../types";
export type DisplayItem = DisplayTrack | DisplayAlbum | DisplayArtist | DisplayPlaylist | QueueEntry | SearchMatch;
export interface PageState { hasEarlier?: boolean; detail?: AlbumDetail; items: DisplayItem[]; nextCursor: string | null; loading: boolean; error: string; revision: number | null }
/** A bounded visible page, never a phone library or ordering authority. */
export function createControllerPage(port: () => ApplicationPort = getApplicationPort, connection: Readable<ControllerState> = controllerState) {
 const empty = (): PageState => ({ items: [], nextCursor: null, loading: false, error: "", revision: null });
 const state = writable<PageState>(empty());
 let query: ApplicationQuery | undefined, key = "", generation = 0, abort: AbortController | undefined, disposed = false;
 function identity(s: ControllerState) { return s.ready && s.snapshot ? `${s.snapshot.hostId}/${s.snapshot.hostEpoch}/${query?.type === "queue" ? s.snapshot.revisions.queueRevision : s.snapshot.revisions.libraryRevision}` : ""; }
 function reset() { generation++; abort?.abort(); state.set(empty()); key = identity(get(connection)); if (key && query && !disposed) void load(); }
 const unsubscribe = connection.subscribe(s => { if (identity(s) !== key) reset(); });
 async function load(cursor?: string) {
  if (!query || !key || disposed || get(state).loading) return;
  const ticket = generation, source = query, snapshot = get(connection).snapshot!;
  abort = new AbortController(); state.update(s => ({ ...s, loading: true, error: "" }));
  try {
   const request = source.type === "album_detail" ? source : { ...source, limit: 100, ...(cursor ? { cursor } : {}) } as ApplicationQuery;
   const result = await port().query(request, abort.signal);
   if (ticket !== generation || disposed) return;
   const expected = source.type === "queue" ? snapshot.revisions.queueRevision : snapshot.revisions.libraryRevision;
   if (result.type === "album_detail" && source.type === "album_detail" && result.revision === expected) { state.set({ ...empty(), detail: result.detail, revision: result.revision }); return; }
   if (!("page" in result) || result.type !== source.type || result.page.revision !== expected) throw new Error("The PC library changed. Refresh this view.");
   if (result.page.nextCursor && result.page.nextCursor === cursor) throw new Error("The PC returned a repeated page cursor.");
   state.update(s => ({ hasEarlier: !!(cursor && s.hasEarlier) || (cursor ? s.items.length : 0) + result.page.items.length > 1000, items: [...(cursor ? s.items : []), ...result.page.items].slice(-1000), nextCursor: result.page.nextCursor, revision: result.page.revision, loading: false, error: "" }));
  } catch (e) {
   if (ticket === generation && !disposed) state.update(s => ({ ...s, loading: false, error: e && typeof e === "object" && "message" in e ? String(e.message) : "The PC could not load this view." }));
  }
 }
 return { state: readonly(state), setQuery(value: ApplicationQuery) { if (JSON.stringify(value) !== JSON.stringify(query)) { query = value; reset(); } },
  more: () => get(state).nextCursor ? load(get(state).nextCursor!) : Promise.resolve(), refresh: reset,
  dispose() { disposed = true; generation++; abort?.abort(); unsubscribe(); state.set(empty()); } };
}
