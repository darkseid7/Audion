import { afterEach, expect, it, vi } from "vitest";
import { installApplicationPort, createUnavailablePort } from "./port";
import { queryLibrary } from "./library";

let release = () => {};
afterEach(() => release());
it("delegates bounded host queries without a local database fallback", async () => {
  const query = vi.fn().mockResolvedValue({ type: "tracks", page: { items: [], nextCursor: null, revision: 7 } });
  release = installApplicationPort({ ...createUnavailablePort(), query });
  const signal = new AbortController().signal;
  expect(await queryLibrary({ type: "tracks", limit: 200 }, signal)).toMatchObject({ page: { revision: 7 } });
  expect(query).toHaveBeenCalledWith({ type: "tracks", limit: 200 }, signal);
  await expect(queryLibrary({ type: "tracks", limit: 201 })).rejects.toThrow();
  expect(query).toHaveBeenCalledTimes(1);
});
it("rejects cancellation before dispatch and ignores late query results", async () => {
  let finish!: (value: unknown) => void;
  const query = vi.fn(() => new Promise<any>(resolve => { finish = resolve; }));
  release = installApplicationPort({ ...createUnavailablePort(), query });
  const aborted = new AbortController(); aborted.abort();
  await expect(queryLibrary({ type: "albums" }, aborted.signal)).rejects.toMatchObject({ name: "AbortError" });
  expect(query).not.toHaveBeenCalled();
  const active = new AbortController();
  const pending = queryLibrary({ type: "tracks" }, active.signal);
  active.abort();
  await expect(pending).rejects.toMatchObject({ name: "AbortError" });
  finish({ type: "tracks", page: { items: [], nextCursor: null, revision: 1 } });
});

it("accepts the closed MVP queries and rejects unknown/private inputs", async () => {
  const { parseLibraryQuery } = await import("./library");
  for (const query of [{ type: "album_detail", albumId: 1 }, { type: "playlists" }, { type: "playlist_tracks", playlistId: 2 }, { type: "artist_tracks", artistName: "Björk" }, { type: "liked_tracks" }, { type: "albums", likedOnly: true, text: "Album", sort: "year-desc" }]) expect(parseLibraryQuery(query)).toEqual(query);
  for (const query of [{ type: "tracks", path: "/private" }, { type: "playlist_tracks", playlistId: 0 }, { type: "artist_tracks", artistName: "" }, { type: "tracks", cursor: null }, { type: "albums", likedOnly: "yes" }, { type: "command" }]) expect(() => parseLibraryQuery(query)).toThrow();
});

it("shares the native closed-query fixtures", async () => {
  const fixtures = (await import("../../../tests/fixtures/controller/library-queries.json")).default;
  const { parseLibraryQuery } = await import("./library");
  for (const query of fixtures.valid) expect(parseLibraryQuery(query)).toEqual(query);
  for (const query of fixtures.invalid) expect(() => parseLibraryQuery(query)).toThrow();
});
