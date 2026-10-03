import { getApplicationPort } from "./port";
import type { ApplicationQuery, QueryResult } from "./types";
export type LibraryQuery = Exclude<ApplicationQuery, { type: "snapshot" | "outputs" }>;
/** Delegates to the selected role; cancellation never falls back to local data. */
export async function queryLibrary(query: LibraryQuery, signal?: AbortSignal): Promise<QueryResult> {
  query = parseLibraryQuery(query);
  signal?.throwIfAborted();
  if ("limit" in query && query.limit !== undefined && (!Number.isSafeInteger(query.limit) || query.limit < 1 || query.limit > 200)) throw new Error("Invalid page limit");
  const pending = getApplicationPort().query(query, signal);
  if (!signal) return pending;
  return new Promise((resolve, reject) => {
    const abort = () => reject(signal.reason ?? new DOMException("Query cancelled", "AbortError"));
    signal.addEventListener("abort", abort, { once: true });
    if (signal.aborted) abort();
    void pending.then(value => { if (!signal.aborted) resolve(value); }, reject).finally(() => signal.removeEventListener("abort", abort));
  });
}

/** Closed validator shared by local and remote consumers; never a native proxy. */
export function parseLibraryQuery(value: unknown): LibraryQuery {
  const invalid = (): never => { throw new Error("Invalid library query"); };
  if (!value || typeof value !== "object" || Array.isArray(value)) return invalid();
  const q = value as Record<string, unknown>;
  const keys: Record<string, string[]> = { albums: ["sort", "likedOnly", "text"], album_detail: ["albumId"], album_tracks: ["albumId"], tracks: [], artists: [], artist_albums: ["artistName"], artist_tracks: ["artistName"], search: ["text"], queue: [], playlists: [], playlist_tracks: ["playlistId"], liked_tracks: [] };
  if (typeof q.type !== "string" || !Object.hasOwn(keys, q.type)) return invalid();
  if (Reflect.ownKeys(q).some(k => typeof k !== "string" || !["type", ...keys[q.type as string], ...(q.type === "album_detail" ? [] : ["cursor", "limit"])].includes(k))) return invalid();
  const text = (v: unknown, max: number) => { if (typeof v !== "string" || !v.length || new TextEncoder().encode(v).length > max || /[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/u.test(v)) invalid(); };
  for (const key of ["albumId", "playlistId"]) if (keys[q.type].includes(key) && (!Number.isSafeInteger(q[key]) || (q[key] as number) < 1)) invalid();
  if (keys[q.type].includes("artistName")) text(q.artistName, 256);
  if (q.type === "search" || Object.hasOwn(q, "text")) text(q.text, q.type === "search" ? 2048 : 256);
  if (Object.hasOwn(q, "cursor")) text(q.cursor, 256);
  if (Object.hasOwn(q, "limit") && (!Number.isSafeInteger(q.limit) || (q.limit as number) < 1 || (q.limit as number) > 200)) invalid();
  if (Object.hasOwn(q, "likedOnly") && typeof q.likedOnly !== "boolean") invalid();
  if (Object.hasOwn(q, "sort") && !["artist-asc", "artist-desc", "year-desc", "year-asc", "added-desc", "added-asc", "name-asc", "name-desc"].includes(q.sort as string)) invalid();
  return { ...q } as LibraryQuery;
}
