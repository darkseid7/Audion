import { get, writable, readonly, type Readable } from "svelte/store";
import { controllerState } from "./bootstrap";
import { getApplicationPort } from "../port";
import type { ControllerState } from "./session";
import type { ApplicationPort, ApplicationQuery, DisplayTrack, DisplayAlbum, DisplayArtist, DisplayPlaylist, AlbumDetail, QueueEntry, SearchMatch } from "../types";
export type DisplayItem = DisplayTrack | DisplayAlbum | DisplayArtist | DisplayPlaylist | QueueEntry | SearchMatch;
export interface PageState { hasEarlier?: boolean; detail?: AlbumDetail; items: DisplayItem[]; nextCursor: string | null; loading: boolean; error: string; revision: number | null }
/** Opaque native tokens rotate with revisions; the displayed entity does not. */
function presentationEntity<T extends DisplayTrack | DisplayAlbum | DisplayArtist | DisplayPlaylist>(item: T): T {
 if (!item.artwork) return item;
 const kind = "qualityBadges" in item ? "album" : "quality" in item ? "track" : "albumCount" in item ? "artist" : "playlist";
 const identity = "id" in item ? item.id : "name" in item ? item.name : "";
 return { ...item, artwork: { ...item.artwork, presentationKey: JSON.stringify([kind, identity]) } };
}
function presentationItem(item: DisplayItem): DisplayItem {
 if ("entryId" in item) return { ...item, track: presentationEntity(item.track) };
 if ("type" in item) {
  if (item.type === "track") return { ...item, track: presentationEntity(item.track) };
  if (item.type === "album") return { ...item, album: presentationEntity(item.album) };
  return { ...item, artist: presentationEntity(item.artist) };
 }
 return presentationEntity(item);
}
/** A bounded visible page, never a phone library or ordering authority. */
export function createControllerPage(port: () => ApplicationPort = getApplicationPort, connection: Readable<ControllerState> = controllerState) {
 const empty = (): PageState => ({ items: [], nextCursor: null, loading: false, error: "", revision: null });
 const state = writable<PageState>(empty());
 let query: ApplicationQuery | undefined, key = "", scope = "", generation = 0, abort: AbortController | undefined, disposed = false, pending = false;
 function authority(s: ControllerState) { return s.ready && s.snapshot ? JSON.stringify([s.snapshot.hostId, s.snapshot.hostEpoch, s.grants?.control]) : ""; }
 function identity(s: ControllerState) { const host = authority(s); return host ? JSON.stringify([host, query?.type === "queue" ? s.snapshot!.revisions.queueRevision : s.snapshot!.revisions.libraryRevision]) : ""; }
 function reset(retain = false) {
  generation++; abort?.abort(); pending = false;
  state.update(s => retain ? { ...s, nextCursor: null, loading: false, error: "" } : empty());
  const connectionState = get(connection); key = identity(connectionState); scope = authority(connectionState);
  if (key && query && !disposed) void load();
 }
 const unsubscribe = connection.subscribe(s => { if (identity(s) !== key) reset(!!scope && authority(s) === scope && !!query && query.type !== "queue"); });
 async function load(cursor?: string, refresh = false) {
  if (!query || !key || disposed || pending) return;
  const ticket = generation, source = query, snapshot = get(connection).snapshot!;
  pending = true; abort = new AbortController();
  // Background replacement must not resize the grid with initial-loading content.
  state.update(s => ({ ...s, loading: !s.items.length && !s.detail, error: "" }));
  try {
   const request = source.type === "album_detail" ? source : { ...source, limit: 100, ...(cursor ? { cursor } : {}) } as ApplicationQuery;
   const result = await port().query(request, abort.signal, { refresh });
   if (ticket !== generation || disposed) return;
   const expected = source.type === "queue" ? snapshot.revisions.queueRevision : snapshot.revisions.libraryRevision;
   if (result.type === "album_detail" && source.type === "album_detail" && result.revision === expected) { state.set({ ...empty(), detail: { ...result.detail, album: presentationEntity(result.detail.album) }, revision: result.revision }); return; }
   if (!("page" in result) || result.type !== source.type || result.page.revision !== expected) throw new Error("The PC library changed. Refresh this view.");
   if (result.page.nextCursor && result.page.nextCursor === cursor) throw new Error("The PC returned a repeated page cursor.");
   state.update(s => ({ hasEarlier: !!(cursor && s.hasEarlier) || (cursor ? s.items.length : 0) + result.page.items.length > 1000, items: [...(cursor ? s.items : []), ...result.page.items.map(presentationItem)].slice(-1000), nextCursor: result.page.nextCursor, revision: result.page.revision, loading: false, error: "" }));
  } catch (e) {
   if (ticket === generation && !disposed) state.update(s => ({ ...s, loading: false, error: e && typeof e === "object" && "message" in e ? String(e.message) : "The PC could not load this view." }));
  } finally {
   if (ticket === generation) pending = false;
  }
 }
 return { state: readonly(state), setQuery(value: ApplicationQuery) { if (JSON.stringify(value) !== JSON.stringify(query)) { query = value; reset(); } },
  more: () => get(state).nextCursor ? load(get(state).nextCursor!) : Promise.resolve(), refresh() {
   if (disposed || pending) return;
   if (identity(get(connection)) !== key) { reset(); return; }
   // Same identity: retain the valid presentation until first-page replacement.
   generation++; abort?.abort(); void load(undefined, true);
  },
  dispose() { disposed = true; generation++; abort?.abort(); unsubscribe(); state.set(empty()); } };
}
