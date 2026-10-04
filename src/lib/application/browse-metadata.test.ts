import { describe, it, expect } from "vitest";
import { parseBrowseMetadataQuery, parseBrowseMetadataResult } from "./browse-metadata";
const request = { metadataVersion: 1 as const, hostEpoch: "epoch", libraryRevision: 7, trackIds: [42, 43], albumId: 8 };
const reply = () => ({ metadataVersion: 1, hostEpoch: "epoch", libraryRevision: 7, tracks: [{ trackId: 42, playCount: 0 }, { trackId: 43, playCount: null }], album: { albumId: 8, totalDurationSeconds: 202 } });
describe("browse metadata contract", () => {
  it("metadata_preserves_zero_and_null", () => {
    expect(parseBrowseMetadataResult(reply(), request).tracks).toEqual([{ trackId: 42, playCount: 0 }, { trackId: 43, playCount: null }]);
    expect(parseBrowseMetadataQuery({ trackIds: [42, 42] })).toEqual({ trackIds: [42] });
    expect(parseBrowseMetadataQuery({ trackIds: [], albumId: 8 })).toEqual({ trackIds: [], albumId: 8 });
  });
  it.each([{}, { trackIds: [] }, { trackIds: Array(1001).fill(42) }, { trackIds: [-1] }, { trackIds: [NaN] }, { trackIds: [Number.MAX_SAFE_INTEGER + 1] }, { trackIds: [1], albumId: null }, { trackIds: [1], extra: true }])("rejects invalid query %j", value => {
    expect(() => parseBrowseMetadataQuery(value)).toThrow();
  });
  it.each([
    { metadataVersion: 2 }, { hostEpoch: "old" }, { libraryRevision: 8 }, { extra: 1 },
    { tracks: [{ trackId: 42, playCount: -1 }, { trackId: 43, playCount: 0 }] },
    { tracks: [{ trackId: 42, playCount: Number.MAX_SAFE_INTEGER + 1 }, { trackId: 43, playCount: 0 }] },
    { tracks: [{ trackId: 42, playCount: 1.5 }, { trackId: 43, playCount: 0 }] },
    { tracks: [{ trackId: 42, playCount: 0 }, { trackId: 42, playCount: 0 }] },
    { tracks: [{ trackId: 44, playCount: 0 }, { trackId: 43, playCount: 0 }] },
    { tracks: [{ trackId: 42, playCount: 0 }] },
    { tracks: [{ trackId: 42, playCount: 0, path: "secret" }, { trackId: 43, playCount: 0 }] },
    { album: undefined }, { album: { albumId: 9, totalDurationSeconds: 0 } },
    { album: { albumId: 8, totalDurationSeconds: -1 } },
    { album: { albumId: 8, totalDurationSeconds: 202, extra: true } },
  ])("rejects invalid reply %j", patch => expect(() => parseBrowseMetadataResult({ ...reply(), ...patch }, request)).toThrow());
  it("rejects unsolicited album", () => expect(() => parseBrowseMetadataResult(reply(), { ...request, albumId: undefined })).toThrow());
});
