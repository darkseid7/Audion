import { afterEach, describe, expect, it, vi } from "vitest";
import { loadAlbumView, saveAlbumView, albumViewTransitionDuration } from "./album-view";

afterEach(() => vi.unstubAllGlobals());

describe("Album view preference", () => {
  it.each([null, "", "compact", "LIST", "{}"])("defaults invalid or absent preference %s to grid", (saved) => {
    vi.stubGlobal("localStorage", { getItem: () => saved });
    expect(loadAlbumView()).toBe("grid");
  });

  it("restores list after persisting a selected layout", () => {
    const values = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => values.set(key, value),
    });
    saveAlbumView("list");
    expect(values.get("audion_album_view")).toBe("list");
    expect(loadAlbumView()).toBe("list");
    saveAlbumView("grid");
    expect(loadAlbumView()).toBe("grid");
  });

  it("does not block viewing when storage reads and writes are denied", () => {
    vi.stubGlobal("localStorage", { getItem() { throw new Error("denied"); }, setItem() { throw new Error("denied"); } });
    expect(loadAlbumView()).toBe("grid");
    expect(() => saveAlbumView("list")).not.toThrow();
  });

  it("defaults safely outside the browser", () => {
    vi.stubGlobal("localStorage", undefined);
    vi.stubGlobal("window", undefined);
    expect(loadAlbumView()).toBe("grid");
    expect(() => saveAlbumView("list")).not.toThrow();
    expect(albumViewTransitionDuration()).toBe(0);
  });
});

describe("Album view motion preference", () => {
  it.each([[false, 180], [true, 0]])("respects reduced motion %s", (matches, duration) => {
    vi.stubGlobal("window", { matchMedia: () => ({ matches }) });
    expect(albumViewTransitionDuration()).toBe(duration);
  });
});
