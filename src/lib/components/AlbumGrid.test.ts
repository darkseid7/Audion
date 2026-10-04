import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { compile } from "svelte/compiler";
import * as svelte from "svelte";
import { render } from "svelte/server";
import { writable } from "svelte/store";
import ts from "typescript";
import { describe, expect, it } from "vitest";

// The compiler-generated SSR runtime does not expose a public declaration entrypoint.
const serverRuntime: string = "svelte/internal/server";
const server = await import(serverRuntime) as Record<string, unknown>;

const albums = [
  { id: 2, name: "Beta", artist: "Zed", year: 2023 },
  { id: 1, name: "Alpha", artist: "Ada", year: 2001 },
];

function renderAlbums(savedView?: string, deniedStorage = false, playing = true) {
  const saved = new Map(savedView ? [["audion_album_view", savedView]] : []);
  const storage = {
    getItem(key: string) { if (deniedStorage) throw new Error("denied"); return saved.get(key) ?? null; },
    setItem(key: string, value: string) { if (deniedStorage) throw new Error("denied"); saved.set(key, value); },
  };
  const modules: Record<string, unknown> = {
    "$lib/application/bootstrap": { applicationMode: writable("desktop") },
    "$lib/application/view-actions": { viewActions: {} },
    "./ControllerBrowse.svelte": { default: () => {} },
    "svelte": svelte,
    "svelte/internal/server": server,
    "$lib/stores/view": { goToAlbumDetail() {}, goToArtistDetail() {} },
    "$lib/stores/library": {
      loadLibrary() {}, loadMoreAlbums() {}, getAlbumCoverFromTracks: () => "cover.jpg",
      tracks: writable([{ id: 10, album_id: 1, format: "FLAC", metadata_json: '{"__sample_rate_hz":44100,"__bit_depth":16}' }]),
    },
    "$lib/stores/ui": { contextMenu: writable(null) },
    "$lib/api/tauri": { deleteAlbum() {}, getTracksByAlbum: async () => [] },
    "@tauri-apps/plugin-opener": { revealItemInDir() {} },
    "$lib/stores/player": {
      currentAlbumId: writable(1), isPlaying: writable(playing), playTracks() {}, togglePlay() {}, appendToQueueEnd() {}, playNext() {},
    },
    "$lib/stores/dialogs": { confirm() {}, prompt() {} },
    "$lib/stores/scrollMemory": { saveScroll() {}, getScroll: () => 0 },
    "$lib/stores/pinned": { pinnedItems: writable([]), pinItem() {}, unpinItem() {}, isPinned: () => true },
    "$lib/stores/customArtwork": { setCustomArtwork() {} },
    "$lib/stores/toast": { addToast() {} },
    "$lib/stores/listen-later": { isInListenLater: () => false, toggleListenLater() {} },
    "$lib/stores/liked-albums": { likedAlbumIds: writable(new Set([1])) },
    "svelte-i18n": {
      _: writable((key: string) => ({ "album.gridView": "Grid view", "album.listView": "List view", "album.viewMode": "Album view" }[key] ?? key)),
    },
    "./Virtualizedgrid.svelte": { default: (renderer: { push(html: string): void }, props: any) => {
      renderer.push(`<section data-layout="${props.layout ?? "grid"}">`);
      for (const item of props.items) props.$$slots.default(renderer, { item });
      renderer.push("</section>");
    } },
  };
  const load = (file: string) => {
    const source = readFileSync(new URL(file, import.meta.url), "utf8");
    const code = file.endsWith(".svelte") ? compile(source, { filename: file, generate: "server" }).js.code : source;
    const { outputText } = ts.transpileModule(code, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } });
    const exports: Record<string, any> = {};
    runInNewContext(outputText, {
      exports, localStorage: storage,
      require(name: string) {
        if (name === "$lib/stores/album-view") return load("../stores/album-view.ts");
        if (!(name in modules)) throw new Error(`Missing test dependency: ${name}`);
        return modules[name];
      },
    });
    return exports;
  };
  modules["./MediaCard.svelte"] = { default: load("./MediaCard.svelte").default };
  return render(load("./AlbumGrid.svelte").default, { props: { albums } }).body;
}

function button(html: string, label: string) {
  return html.match(/<button\b[^>]*>[\s\S]*?<\/button>/g)?.find((markup) => markup.includes(`aria-label="${label}"`));
}

describe("Album grid/list rendering", () => {
  it("defaults to grid with accessible pressed-state layout controls", () => {
    const html = renderAlbums();
    expect(button(html, "Grid view") ?? "").toContain('aria-pressed="true"');
    expect(button(html, "List view") ?? "").toContain('aria-pressed="false"');
  });

  it("restores list mode without changing the sorted albums or album actions", () => {
    const html = renderAlbums("list");
    expect(button(html, "List view") ?? "").toContain('aria-pressed="true"');
    expect(html).toContain('data-layout="list"');
    expect(html.indexOf('aria-label="Alpha"')).toBeLessThan(html.indexOf('aria-label="Beta"'));
    expect(html).toContain("Ada · 2001");
    expect(html).toContain("44.1kHz");
    expect(html).toContain("16bit");
    expect(html).toContain('aria-label="Pinned to top"');
    expect(html).toContain('aria-label="Liked"');
    expect(html).toContain('data-mediacard-play');
  });

  it("uses native playback tooltips for compact list artwork", () => {
    const playingList = renderAlbums("list");
    expect(button(playingList, "Play album") ?? "").toContain('title="Play album"');
    expect(button(playingList, "Pause") ?? "").toContain('title="Pause"');
    const pausedList = renderAlbums("list", false, false);
    expect(button(pausedList, "Resume album") ?? "").toContain('title="Resume album"');
  });

  it("keeps existing custom playback tooltips in grid mode", () => {
    const html = renderAlbums();
    expect(button(html, "Play album") ?? "").toContain('data-play-tooltip="Play album"');
    expect(button(html, "Play album") ?? "").not.toContain('title=');
    expect(button(html, "Pause") ?? "").not.toContain('title=');
  });

  it("renders usable default-grid controls when browser storage is denied", () => {
    const html = renderAlbums(undefined, true);
    expect(button(html, "Grid view") ?? "").toContain('aria-pressed="true"');
  });
});


function loadSelection(context: Record<string, unknown>) {
  const component = readFileSync(new URL("./AlbumGrid.svelte", import.meta.url), "utf8");
  const script = component.match(/<script lang="ts">([\s\S]*?)<\/script>/)?.[1];
  if (!script) throw new Error("AlbumGrid script missing");
  const source = ts.createSourceFile("AlbumGrid.ts", script, ts.ScriptTarget.Latest, true);
  const handler = source.statements.find((node) => ts.isFunctionDeclaration(node) && node.name?.text === "selectAlbumView");
  if (!handler) throw new Error("Album view selection not implemented");
  const { outputText } = ts.transpileModule(handler.getText(source), { compilerOptions: { target: ts.ScriptTarget.ES2022 } });
  return runInNewContext(`${outputText}; selectAlbumView;`, context) as (view: string) => void;
}

describe("Album layout selection", () => {
  it("changes and persists layout while replaying a short fade without replacing the grid body", () => {
    const saved: string[] = [];
    const fades: unknown[] = [];
    let cancelled = 0;
    const body = { animate(_frames: unknown, options: unknown) { fades.push(options); return { cancel() { cancelled += 1; } }; } };
    const state = { albumView: "grid", viewAnimation: null, albumGridBody: body,
      saveAlbumView: (view: string) => saved.push(view), albumViewTransitionDuration: () => 180 };
    const select = loadSelection(state);
    select("list");
    select("grid");
    expect(state.albumView).toBe("grid");
    expect(saved).toEqual(["list", "grid"]);
    expect(fades).toEqual([{ duration: 180, easing: "ease-out" }, { duration: 180, easing: "ease-out" }]);
    expect(cancelled).toBe(1);
    expect(state.albumGridBody).toBe(body);
  });

  it("switches layout without animation when reduced motion is active", () => {
    let fades = 0;
    const state = { albumView: "grid", viewAnimation: null, albumGridBody: { animate() { fades += 1; } },
      saveAlbumView() {}, albumViewTransitionDuration: () => 0 };
    loadSelection(state)("list");
    expect(state.albumView).toBe("list");
    expect(fades).toBe(0);
  });
});
