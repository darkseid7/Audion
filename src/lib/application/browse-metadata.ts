import type { BrowseMetadataQuery, BrowseMetadataRequest, BrowseMetadataResult } from "./types";
const invalid = (): never => { throw { code: "invalid_request", message: "Invalid browse metadata.", retryable: false }; };
function object(value: unknown, keys: string[]): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) return invalid();
  const result = value as Record<string, unknown>;
  if (Object.keys(result).some(key => !keys.includes(key))) return invalid();
  return result;
}
const integer = (v: unknown): v is number => typeof v === "number" && Number.isSafeInteger(v) && v >= 0;
const id = (v: unknown): v is number => integer(v) && v > 0;
const metric = (v: unknown): v is number | null => v === null || integer(v);
export function parseBrowseMetadataQuery(value: unknown): BrowseMetadataQuery {
  const v = object(value, ["trackIds", "albumId"]);
  if (!Array.isArray(v.trackIds) || v.trackIds.length > 1000 || !v.trackIds.every(id) ||
    ("albumId" in v && !id(v.albumId)) || (!v.trackIds.length && !id(v.albumId))) return invalid();
  return { trackIds: [...new Set(v.trackIds)], ...(id(v.albumId) ? { albumId: v.albumId } : {}) };
}
export function parseBrowseMetadataResult(value: unknown, request: BrowseMetadataRequest): BrowseMetadataResult {
  const query = parseBrowseMetadataQuery({ trackIds: request.trackIds, ...(request.albumId !== undefined ? { albumId: request.albumId } : {}) });
  const v = object(value, ["metadataVersion", "hostEpoch", "libraryRevision", "tracks", "album"]);
  if (v.metadataVersion !== 1 || v.hostEpoch !== request.hostEpoch || typeof v.hostEpoch !== "string" || !v.hostEpoch || v.hostEpoch.length > 256 ||
    !integer(v.libraryRevision) || v.libraryRevision !== request.libraryRevision || !Array.isArray(v.tracks) || v.tracks.length !== query.trackIds.length) return invalid();
  const pending = new Set(query.trackIds);
  const tracks = v.tracks.map(entry => {
    const t = object(entry, ["trackId", "playCount"]);
    if (!id(t.trackId) || !pending.delete(t.trackId) || !metric(t.playCount)) return invalid();
    return { trackId: t.trackId, playCount: t.playCount };
  });
  let album;
  if (query.albumId !== undefined) {
    const a = object(v.album, ["albumId", "totalDurationSeconds"]);
    if (a.albumId !== query.albumId || !metric(a.totalDurationSeconds)) return invalid();
    album = { albumId: query.albumId, totalDurationSeconds: a.totalDurationSeconds };
  } else if ("album" in v) return invalid();
  return { metadataVersion: 1, hostEpoch: v.hostEpoch, libraryRevision: v.libraryRevision, tracks, ...(album ? { album } : {}) };
}
