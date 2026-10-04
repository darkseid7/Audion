import { existsSync, readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { runInNewContext } from "node:vm";
import { compile } from "svelte/compiler";
import { render } from "svelte/server";
import * as svelte from "svelte";
import ts from "typescript";
import { expect, it, vi } from "vitest";
const runtime: string = "svelte/internal/server";
const server = await import(runtime);

function load(path: string): any {
  if (!existsSync(path)) return { default: () => {} };
  const compiled = compile(readFileSync(path, "utf8"), { filename: path, generate: "server" });
  const code = ts.transpileModule(compiled.js.code, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  const exports: any = {};
  runInNewContext(code, { exports, require(id: string) {
    if (id === "svelte/internal/server") return server;
    if (id === "svelte") return svelte;
    if (id.endsWith(".svelte")) return load(resolve(dirname(path), id));
    throw new Error(`Unexpected presentation dependency: ${id}`);
  } });
  return exports;
}
const base = resolve("src/lib/components/presentation");
const rows = [
  { id: "home", label: "Home", icon: "home", active: false, availability: { enabled: false, reason: "Requires a desktop projection" } },
  { id: "albums", label: "Albums", icon: "albums", active: true, count: 12, availability: { enabled: true } },
  { id: "liked-songs", label: "Liked Songs", icon: "liked-songs", active: false, availability: { enabled: true } },
];
const sections = [{ id: "library", label: "Library", rows }, { id: "playlists", label: "Playlists", rows: [{ id: "playlists", label: "All Playlists", icon: "playlists", active: false, availability: { enabled: true } }] }];

it("shared expanded navigation preserves desktop groups, SVGs, active theme hooks and truthful unavailable state", () => {
  const html = render(load(resolve(base, "Navigation.svelte")).default, { props: { sections } }).body;
  expect(html).toContain('class="sidebar');
  expect(html).toContain('class="sidebar-nav');
  expect(html).toContain('nav-section-title');
  expect(html).toContain("Library"); expect(html).toContain("All Playlists");
  expect(html).toContain("M10 20v-6h4v6h5v-8h3L12 3 2 12h3v8z");
  expect(html).toMatch(/class="nav-item[^\"]*active/);
  expect(html).toContain('aria-current="page"');
  expect(html).toContain("Requires a desktop projection"); expect(html).toContain("disabled");
  expect(html.match(/class="nav-count/g)).toHaveLength(1);
  expect(html).not.toContain("▦");
});

it("compact navigation uses the existing Home Library Plugins Actions presentation", () => {
  const tabs = ["home", "library", "plugins", "actions"].map((id) => ({ id, icon: id, label: id[0].toUpperCase() + id.slice(1), active: id === "library", availability: { enabled: id === "library", reason: "Desktop only" } }));
  const html = render(load(resolve(base, "Navigation.svelte")).default, { props: { variant: "compact", sections: [{ id: "tabs", label: "", rows: tabs }], hasPlayer: true } }).body;
  expect(html).toContain('class="bottom-nav'); expect(html).toContain("has-player");
  for (const label of ["Home", "Library", "Plugins", "Actions"]) expect(html).toContain(label);
  expect(html).toContain("M7 2v11h3v9l7-12h-4l4-8z");
  expect(html).not.toContain('class="sidebar');
});

it("an unavailable navigation gesture never reaches its callback", () => {
  const fn = load(resolve(base, "Navigation.svelte")).navigateRow;
  expect(fn).toBeTypeOf("function");
  if (!fn) return;
  const navigate = vi.fn();
  fn(rows[0], navigate); expect(navigate).not.toHaveBeenCalled();
  fn(rows[1], navigate); expect(navigate).toHaveBeenCalledExactlyOnceWith("albums");
});

it.each(["expanded", "compact"])("AppShell renders one stable content/player/panel surface for %s", layout => {
  const component = load(resolve(base, "AppShell.svelte")).default;
  const slot = (text: string) => (renderer: any) => renderer.push(text);
  const html = render(component, { props: { layout, safeArea: true, $$slots: {
    sidebar: slot("sidebar-marker"), default: slot("content-marker"), panels: slot("panel-marker"), player: slot("player-marker"), "bottom-navigation": slot("bottom-marker"), status: slot("offline-marker"), feedback: slot("unknown-outcome-marker"),
  } } }).body;
  expect(html).toContain(`data-layout="${layout}"`);
  for (const text of ["content-marker", "player-marker", "panel-marker", "offline-marker", "unknown-outcome-marker"]) expect(html.split(text)).toHaveLength(2);
  expect(html.includes("bottom-marker")).toBe(layout === "compact");
  expect(html).toContain("safe-area"); expect(html).not.toContain("data-tauri-drag-region");
});
it("library subnavigation retains the desktop mobile pills and all supported destinations", () => {
  const tabs = ["tracks", "albums", "artists", "playlists"].map((id, index) => ({ id, icon: id, label: ["Songs", "Albums", "Artists", "Playlists"][index], active: id === "albums", availability: { enabled: true } }));
  const html = render(load(resolve(base, "Navigation.svelte")).default, { props: { variant: "library", sections: [{ id: "library", label: "", rows: tabs }] } }).body;
  expect(html).toContain("mobile-library-tabs"); expect(html).toContain("lib-tab");
  for (const label of ["Songs", "Albums", "Artists", "Playlists"]) expect(html).toContain(label);
});
import { writable } from "svelte/store";
import * as themeCatalog from "../stores/theme";

function controllerFixture(compact = false, settings = false, translationReady = true) {
  const captures: { name: string; props: any }[] = [];
  const native = vi.fn(() => { throw new Error("Unexpected native/domain effect in UI fixture"); });
  const currentView = writable({ type: settings ? "settings" : "albums" });
  const state = { ready: true, status: "connected", grants: { control: true }, snapshot: { outputs: [{ output: { kind: "pc" }, name: "Studio PC" }], playback: { track: null }, capabilities: { queries: ["albums", "tracks", "artists", "playlists", "liked_tracks"] } } };
  const modules: Record<string, any> = {
    "$lib/stores/mobile": { isMobile: writable(compact) },
    "$lib/stores/view": { currentView, navigationHistory: writable({ canGoBack: false }), goBack: native },
    "$lib/application/controller/bootstrap": { controllerState: writable(state) },
    "$lib/application/controller-ui": { registerControllerBack: () => () => {}, handleControllerBack: () => false },
    "$lib/application/view-actions": { viewActions: { execute: native, queueQuery: native } },
    "$lib/stores/ui": { isQueueVisible: writable(false), isFullScreen: writable(false), isMiniPlayer: writable(false) },
    "$lib/stores/theme": themeCatalog,
    "svelte-i18n": { locale: writable(translationReady ? "en" : null), _: writable((key: string, args: any) => { if (!translationReady) throw new Error("Locale is not initialized"); return args?.default ?? key; }) },
  };
  function real(path: string): any {
    const source = readFileSync(path, "utf8");
    const code = ts.transpileModule(compile(source, { filename: path, generate: "server" }).js.code, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
    const exports: any = {};
    runInNewContext(code, { exports, require(id: string) {
      if (id === "svelte/internal/server") return server;
      if (id === "svelte") return { ...svelte, onMount: () => {}, onDestroy: () => {} };
      if (id in modules) return modules[id];
      if (id.includes("presentation/") && id.endsWith(".svelte")) {
        const child = real(resolve(dirname(path), id));
        return { ...child, default: (renderer: any, props: any) => { captures.push({ name: id, props }); child.default(renderer, props); } };
      }
      if (id.endsWith(".svelte")) return { default: () => {} };
      throw new Error(`Unexpected container dependency: ${id}`);
    } });
    return exports;
  }
  const html = render(real(resolve("src/lib/components/ControllerShell.svelte")).default).body;
  return { html, captures, native, currentView };
}

it.each([false, true])("controller container actually renders the shared shell/navigation for compact=%s without domain effects", compact => {
  const fixture = controllerFixture(compact);
  expect(fixture.html).toContain(`data-layout="${compact ? "compact" : "expanded"}"`);
  expect(fixture.html).toContain("Music library"); expect(fixture.html).toContain("Studio PC");
  expect(fixture.html).toContain("M10 20v-6h4v6h5v-8h3L12 3 2 12h3v8z");
  expect(fixture.html).not.toContain("data-tauri-drag-region");
  const navigation = fixture.captures.find(child => child.name.endsWith("Navigation.svelte") && !child.props.variant)!;
  navigation.props.onNavigate("plugins"); expect(fixture.currentView).toBeDefined();
  navigation.props.onNavigate("artists");
  let view: any; const stop = fixture.currentView.subscribe(value => view = value); stop();
  expect(view.type).toBe("artists"); expect(fixture.native).not.toHaveBeenCalled();
});
it("controller appearance reaches the real full-catalog shared settings with local callbacks", () => {
  const fixture = controllerFixture(false, true);
  expect(fixture.html).toContain("This controller");
  for (const preset of themeCatalog.themePresets) expect(fixture.html).toContain(preset.name);
  const appearance = fixture.captures.find(child => child.name.endsWith("AppearanceSettings.svelte"))!;
  expect(appearance.props.onModeChange).toBe(themeCatalog.theme.setMode);
  expect(appearance.props.onAccentChange).toBe(themeCatalog.theme.setAccentColor);
  expect(fixture.native).not.toHaveBeenCalled();
});
it("controller SSR navigation remains usable before translation initialization", () => {
  expect(() => controllerFixture(false, false, false)).not.toThrow();
});
it("controller appearance also supports SSR before translation initialization", () => {
  expect(() => controllerFixture(false, true, false)).not.toThrow();
});
function desktopFixture(name: "Sidebar" | "MobileBottomNav") {
  const captures: any[] = [], events: string[] = [];
  const native = vi.fn(() => { throw new Error("Unexpected native/domain effect"); });
  const currentView = writable({ type: "albums" });
  const pluginDrawerOpen = writable(false);
  const navigate = Object.fromEntries(["Home", "Tracks", "Albums", "Artists", "Playlists", "Plugins", "Settings", "LikedSongs", "ListenLater", "ListenBrainz", "Discover", "RecentlyPlayed"].map(label => [`goTo${label}`, () => currentView.set({ type: ({ LikedSongs: "liked-songs", ListenLater: "listen-later", ListenBrainz: "listenbrainz", RecentlyPlayed: "recently-played" } as Record<string, string>)[label] ?? label.toLowerCase() })]));
  const modules: Record<string, any> = {
    "$lib/application/bootstrap": { desktopEffectsEnabled: writable(true) },
    "$lib/stores/view": { currentView, ...navigate },
    "$lib/stores/library": { playlists: writable([]), trackCount: writable(40), albumCount: writable(12), artistCount: writable(3), loadPlaylists: native, loadAlbumsAndArtists: native, getTrackAlbumCover: native, getAlbumCoverFromTracks: native },
    "$lib/api/tauri": new Proxy({}, { get: () => native }),
    "$lib/stores/ui": { isSettingsOpen: writable(false), toggleSettings: native, contextMenu: writable({}) },
    "$lib/stores/settings": { appSettings: writable({ listenBrainzEnabled: false, listenBrainzTokenSet: false, showDiscord: false, showResonate: false }) },
    "$lib/stores/liked": { likedCount: writable(4) }, "$lib/stores/listen-later": { listenLaterCount: writable(2) },
    "$lib/stores/playlistCovers": { playlistCovers: writable({}), setPlaylistCover: native },
    "$lib/stores/progressiveScan": { progressiveScan: writable({}) }, "$lib/stores/dialogs": { confirm: native, prompt: native },
    "$lib/stores/player": { currentTrack: writable(null), isPlaying: writable(false), queue: writable([]), currentPlaylistId: writable(null), playTracks: native, addToQueue: native, playNext: native },
    "$lib/plugins/ui-slots": { uiSlotManager: { registerContainer: native, unregisterContainer: native } },
    "$lib/stores/updates": { updates: writable({ hasUpdate: false }) },
    "$lib/stores/pinned": { pinnedItems: writable([]), pinItem: native, unpinItem: native, isPinned: () => false },
    "$lib/stores/customArtwork": { setCustomArtwork: native }, "$lib/stores/toast": { addToast: native },
    "$lib/stores/mobile": { mobileSearchOpen: writable(false) }, "$lib/stores/search": { clearSearch: () => {} },
    "$lib/stores/plugin-drawer": { pluginDrawerOpen },
    "svelte-i18n": { _: writable((key: string, args: any) => args?.default ?? key) },
  };
  const path = resolve(`src/lib/components/${name}.svelte`);
  const code = ts.transpileModule(compile(readFileSync(path, "utf8"), { filename: path, generate: "server" }).js.code, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  const exports: any = {};
  runInNewContext(code, { exports, require(id: string) {
    if (id === "svelte/internal/server") return server;
    if (id === "svelte") return { ...svelte, onMount: () => {}, createEventDispatcher: () => (event: string) => events.push(event) };
    if (id in modules) return modules[id];
    if (id.includes("presentation/") && id.endsWith(".svelte")) { const child = load(resolve(dirname(path), id)); return { ...child, default: (renderer: any, props: any) => { captures.push(props); child.default(renderer, props); } }; }
    if (id.endsWith(".svelte")) return { default: () => {} };
    throw new Error(`Unexpected desktop fixture dependency: ${id}`);
  } });
  const html = render(exports.default).body;
  return { html, captures, events, currentView, pluginDrawerOpen, native };
}
it("desktop Sidebar uses the real shared navigation and retains navigation-event ownership", () => {
  const fixture = desktopFixture("Sidebar");
  expect(fixture.html).toContain("Library"); expect(fixture.html).toContain("All Playlists"); expect(fixture.html).toContain("Add Music Folder");
  fixture.captures[0].onNavigate("artists");
  let view: any; const stop = fixture.currentView.subscribe(value => view = value); stop();
  expect(view.type).toBe("artists"); expect(fixture.events).toEqual(["navigate"]); expect(fixture.native).not.toHaveBeenCalled();
});
it("desktop compact navigation retains its plugin drawer callback without mounting plugin effects in SSR", () => {
  const fixture = desktopFixture("MobileBottomNav");
  expect(fixture.html).toContain("Actions"); expect(fixture.html).toContain("Home");
  fixture.captures[0].onNavigate("actions");
  let open: any; const stop = fixture.pluginDrawerOpen.subscribe(value => open = value); stop();
  expect(open).toBe(true); expect(fixture.native).not.toHaveBeenCalled();
});
it("flow-player shells can reserve compact navigation space without changing desktop shells", () => {
  const component = load(resolve(base, "AppShell.svelte")).default;
  const compact = render(component, { props: { layout: "compact", reserveBottomNavigation: true } }).body;
  const expanded = render(component, { props: { layout: "expanded", reserveBottomNavigation: true } }).body;
  expect(compact).toMatch(/class="bottom-navigation[^\"]*reserved/);
  expect(expanded).not.toContain("bottom-navigation");
});
it.each(["Sidebar", "MobileBottomNav", "MainView", "Settings", "ControllerShell", "presentation/AppShell", "presentation/Navigation", "presentation/AppearanceSettings"])("%s compiles in-memory for SSR and client without executing native effects", name => {
  const path = resolve(`src/lib/components/${name}.svelte`);
  const source = readFileSync(path, "utf8");
  expect(() => compile(source, { filename: path, generate: "server" })).not.toThrow();
  expect(() => compile(source, { filename: path, generate: "client" })).not.toThrow();
});
it("the page composition compiles in-memory without running bootstrap", () => {
  const path = resolve("src/routes/+page.svelte");
  const source = readFileSync(path, "utf8");
  expect(() => compile(source, { filename: path, generate: "server" })).not.toThrow();
  expect(() => compile(source, { filename: path, generate: "client" })).not.toThrow();
});
it("compact controller library keeps Liked Songs reachable without native effects", () => {
 const fixture = controllerFixture(true);
 const nav = fixture.captures.find(child => child.name.endsWith("Navigation.svelte") && child.props.variant === "library")!;
 expect(nav.props.sections[0].rows.some((row: any) => row.id === "liked-songs" && row.availability.enabled)).toBe(true);
 nav.props.onNavigate("liked-songs");
 let view: any; const stop = fixture.currentView.subscribe(value => view = value); stop();
 expect(view.type).toBe("liked-songs"); expect(fixture.native).not.toHaveBeenCalled();
});
it.each(["expanded", "compact", "library"])("%s navigation exposes unavailable reasons through touch-operable disclosure", variant => {
 const html = render(load(resolve(base,"Navigation.svelte")).default,{props:{variant,sections}}).body;
 expect(html).toMatch(/<details[^>]*class="availability-disclosure/);
 expect(html).toMatch(/<summary[^>]*>Unavailable views<\/summary>/);
 expect(html).toMatch(/<li[^>]*>Home: Requires a desktop projection<\/li>/);
});
it.each(["ControllerShell", "Sidebar"])("%s reactive navigation avoids comma operators and preserves explicit store dependencies", name => {
 const source = readFileSync(resolve("src/lib/components",`${name}.svelte`),"utf8");
 expect(source).not.toMatch(/\$:\s*\w+Sections\s*=\s*\(\$/);
 expect(source).toContain("void $currentView;");
 if(name === "ControllerShell") for(const dependency of ["$controllerState","search","$_","$locale"]) expect(source).toContain(`void ${dependency};`);
});
it("reserved controller navigation and disclosure stay in document flow in closed and open states", () => {
 const shell = readFileSync(resolve(base,"AppShell.svelte"),"utf8");
 const nav = readFileSync(resolve(base,"Navigation.svelte"),"utf8");
 expect(shell).toMatch(/\.reserved\s+:global\(\.bottom-nav\)\s*\{[^}]*position:\s*static;[^}]*height:\s*auto;/);
 expect(nav).not.toMatch(/\.bottom-nav \.availability-disclosure\s*\{[^}]*position:\s*absolute/);
 const html = render(load(resolve(base,"Navigation.svelte")).default,{props:{variant:"compact",sections}}).body;
 expect(html).toMatch(/class="compact-navigation/);
 expect(html.indexOf('<details')).toBeLessThan(html.indexOf('<nav'));
 expect(nav).toMatch(/\.compact-navigation \.availability-disclosure\[open\]\s*\{[^}]*max-height:\s*40dvh;[^}]*overflow:\s*auto;/);
});

it("collapsible navigation exposes reversible accessible icon rail without changing commands", () => {
  const component = load(resolve(base, "Navigation.svelte")).default;
  const expanded = render(component, { props: { sections, collapsible: true } }).body;
  expect(expanded).toContain('aria-label="Collapse sidebar"');
  expect(expanded).toContain('aria-expanded="true"');
  const collapsed = render(component, { props: { sections, collapsible: true, collapsed: true } }).body;
  expect(collapsed).toMatch(/class="sidebar[^\"]*collapsed/);
  expect(collapsed).toContain('aria-label="Expand sidebar"');
  expect(collapsed).toContain('aria-expanded="false"');
  expect(collapsed).toContain('aria-label="Albums"');
  expect(collapsed).toContain('aria-current="page"');
  expect(collapsed).toContain("Requires a desktop projection");
  const original = render(component, { props: { sections } }).body;
  expect(original).not.toContain('aria-label="Collapse sidebar"');
});

it('controller header omits global search and duplicate Settings shortcut', () => {
 const source=readFileSync('src/lib/components/ControllerShell.svelte','utf8');
 const header=source.slice(source.indexOf('<header slot="header"'),source.indexOf('</header>',source.indexOf('<header slot="header"')));
 expect(header).not.toContain('Search PC library');
 expect(header).not.toContain('appearance-link');
 expect(source).toContain('navigationRow("settings"');
});
