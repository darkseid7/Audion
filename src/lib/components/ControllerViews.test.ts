import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { resolve } from "node:path";
import { compile } from "svelte/compiler";
import * as svelte from "svelte";
import { render } from "svelte/server";
import { writable } from "svelte/store";
import ts from "typescript";
import { it, expect, vi } from "vitest";
import { createViewActions } from "../application/view-actions";
const native = vi.hoisted(() => vi.fn(() => { throw new Error("Unexpected native controller-view call"); }));
vi.mock("@tauri-apps/api/core", async original => ({ ...await original<object>(), invoke: native }));
const serverRuntime: string = "svelte/internal/server";
const server = await import(serverRuntime);
const connection = writable<any>({ ready: true, grants: { control: true }, currentHostId: "pc", status: "connected", snapshot: {
 hostId: "pc", hostEpoch: "epoch", revision: 1, revisions: { libraryRevision: 2, outputRevision: 3, queueRevision: 4, settingsRevision: 0 },
 output: { kind: "pc" }, outputs: [{ output: { kind: "pc" }, name: "Studio PC", available: true, capabilities: { playback: true, seek: true, volume: true, shuffle: true, repeat: true, equalizer: false } }],
 playback: { status: "paused", track: null, context: null, position: 0, duration: 200, volume: .5, shuffle: false, repeat: "none" }, queue: { count: 0, currentEntryId: null }, jobs: [], settings: {},
 capabilities: { queries: [], intents: ["play_album", "play_artist", "play_playlist", "play_liked", "play_track", "set_volume", "set_shuffle", "seek", "queue_play", "select_output"] }
} });
async function mountEntry(name: string, props: Record<string, unknown> = {}) {
 const execute = vi.fn(async () => ({ status: "applied" as const, revision: 2 }));
 const actions = createViewActions(() => ({ execute } as any), connection);
 const mounts: (() => unknown)[] = [], destroys: (() => void)[] = [];
 let child: any;
 const source = readFileSync(new URL(`./${name}.svelte`, import.meta.url), "utf8");
 const code = ts.transpileModule(compile(source, { filename: `${name}.svelte`, generate: "server" }).js.code, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
 const modules: Record<string, any> = {
  svelte: { ...svelte, onMount: (f: () => unknown) => mounts.push(f), onDestroy: (f: () => void) => destroys.push(f) }, "svelte/internal/server": server,
  "$lib/application/bootstrap": { applicationMode: writable("controller"), desktopEffectsEnabled: writable(false) },
  "$lib/application/view-actions": { viewActions: actions }, "$lib/application/controller/bootstrap": { controllerState: connection },
  "svelte-i18n": { _: writable((k: string) => k), locale: writable("en") }, "$app/navigation": { goto: native },
 };
 for (const match of code.matchAll(/require\("([^"]+)"\)/g)) {
  const id = match[1]; if (id in modules) continue;
  if (id.endsWith(".svelte")) modules[id] = { default: (renderer: any, p: any) => { if (id.startsWith("./Controller")) { child = { ...p }; renderer.push(`<section data-controller="${id}"></section>`); } } };
  else if (id.startsWith("$lib/")) {
   const imported=await import(/* @vite-ignore */ resolve("src/lib", id.slice(5))); modules[id]=imported;
   if(id==="$lib/api/tauri") modules[id]={...imported,...Object.fromEntries(["getAlbum","getTracksByAlbum","getAlbumsByArtist","getTracksByArtist","getPlaylistTracks","getLikedTracks","squeezeIsRunning","squeezeStartServer","squeezeStopServer"].map(key=>[key,native]))};
  }
  else modules[id] = await import(/* @vite-ignore */ id);
 }
 const exports: any = {};
 runInNewContext(code, { exports, require: (id: string) => modules[id], console, setTimeout, clearTimeout, localStorage: { getItem: () => null, setItem() {} } });
 const html = render(exports.default, { props }).body;
 for (const mount of mounts) { const cleanup = await mount(); if (typeof cleanup === "function") destroys.push(cleanup as () => void); }
 return { child, html, execute, dispose() { destroys.forEach(d => d()); actions.dispose(); } };
}
it.each([
 ["AlbumGrid", {}, { type: "play_album", albumId: 42, playMode: "liked_only", startTrackId: 7 }],
 ["AlbumDetail", { albumId: 42 }, { type: "play_album", albumId: 42, playMode: "all", startTrackId: 7 }],
 ["ArtistDetail", { artistName: "Ada" }, { type: "play_artist", artistName: "Ada", startTrackId: 7 }],
 ["PlaylistDetail", { playlistId: 9 }, { type: "play_playlist", playlistId: 9, startTrackId: 7 }],
 ["LikedSongs", {}, { type: "play_liked", startTrackId: 7 }],
 ["TrackList", {}, { type: "play_track", trackId: 7 }],
 ["PlayerBar", {}, { type: "set_volume", volume: .7 }],
 ["MiniPlayer", {}, { type: "seek", seconds: 50 }],
 ["FullScreenPlayer", {}, { type: "set_shuffle", enabled: true }],
 ["QueuePanel", { forceVisible: true }, { type: "queue_play", entryId: "duplicate-second" }],
 ["ConnectPanel", {}, { type: "select_output", output: { kind: "pc" } }],
] as const)("%s mounts the actual controller branch and routes its handler without desktop effects", async (name, props, intent) => {
 native.mockClear(); const mounted = await mountEntry(name, props);
 try {
  expect(mounted.html).toContain("data-controller"); expect(mounted.child).toBeDefined();
  await mounted.child.execute(intent);
  expect(mounted.execute).toHaveBeenCalledExactlyOnceWith(intent, expect.objectContaining({ hostEpoch: "epoch", outputRevision: 3 }));
  expect(native).not.toHaveBeenCalled();
 } finally { mounted.dispose(); }
});

it("output presentation labels the actual PC, omits Android/cloud choices and shows unsupported state",async()=>{
 const mounted=await mountEntry("ControllerOutputs",{execute:async()=>({status:"applied",revision:2})});
 expect(mounted.html).toContain("Studio PC");expect(mounted.html).toContain("Cloud playback is unavailable");expect(mounted.html).not.toContain("This Device");expect(mounted.html).not.toContain("Android");mounted.dispose();
});

function handler(component: string,name:string,scope:Record<string,unknown>) {
 const source=readFileSync(new URL(`./${component}.svelte`,import.meta.url),"utf8");
 const script=source.match(/<script lang="ts">([\s\S]*?)<\/script>/)![1];
 const ast=ts.createSourceFile(component,script,ts.ScriptTarget.Latest,true);
 const fn=ast.statements.find(n=>ts.isFunctionDeclaration(n)&&n.name?.text===name)!;
 const code=ts.transpileModule(fn.getText(ast),{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText;
 return runInNewContext(`${code};${name}`,scope);
}
it.each([
 [{type:"album",albumId:42,playMode:"liked_only"},{type:"play_album",albumId:42,playMode:"liked_only",startTrackId:7}],
 [{type:"artist",artistName:"Ada"},{type:"play_artist",artistName:"Ada",startTrackId:7}],
 [{type:"playlist",playlistId:9},{type:"play_playlist",playlistId:9,startTrackId:7}],
 [{type:"liked"},{type:"play_liked",startTrackId:7}],
 [null,{type:"play_track",trackId:7}],
])("browse row uses PC entity context %j instead of a loaded Track array",async(entityContext,intent)=>{
 const execute=vi.fn(async()=>({status:"applied",revision:1}));await handler("ControllerBrowse","play",{execute,entityContext})(7);expect(execute).toHaveBeenCalledExactlyOnceWith(intent);
});
it("real transport change handlers restore confirmed DOM values and send absolute seek/volume",async()=>{
 const execute=vi.fn(async()=>({status:"applied",revision:1})),playback={position:20,volume:.4};
 const target={value:"80"};await handler("ControllerTransport","seek",{execute,playback})({target});expect(target.value).toBe("20");expect(execute).toHaveBeenLastCalledWith({type:"seek",seconds:80});
 target.value="0.8";await handler("ControllerTransport","volume",{execute,playback})({target});expect(target.value).toBe("0.4");expect(execute).toHaveBeenLastCalledWith({type:"set_volume",volume:.8});
});
it("shuffle-play does not treat an accepted mode job as finished playback",async()=>{
 const execute=vi.fn(async()=>({status:"accepted",jobId:"pending",revision:1})),play=vi.fn();
 await handler("ControllerBrowse","shufflePlay",{execute,play})();expect(play).not.toHaveBeenCalled();
});
it("album card resumes acknowledged album context instead of rebuilding a phone queue",async()=>{
 const execute=vi.fn();handler("ControllerBrowse","playAlbumCard",{execute,$controllerState:{snapshot:{playback:{status:"paused",context:{type:"album",albumId:42}}}}})(42);
 expect(execute).toHaveBeenCalledExactlyOnceWith({type:"resume"});
});

it("disposes artwork returned after a shared artwork view is torn down",async()=>{
 const {installApplicationPort}=await import("../application/port");let resolve!: (value:any)=>void;
 const dispose=vi.fn(),resolveArtwork=vi.fn(()=>new Promise(r=>resolve=r));
 const uninstall=installApplicationPort({resolveArtwork} as any);
 try { const mounted=await mountEntry("ControllerArtwork",{reference:{resourceId:"safe-art",revision:1}});expect(resolveArtwork).toHaveBeenCalledOnce();mounted.dispose();resolve({src:"blob:fixture",dispose});await Promise.resolve();await Promise.resolve();expect(dispose).toHaveBeenCalledOnce(); } finally {uninstall();}
});

it("omits an empty album filter instead of sending invalid optional native text",()=>{
 expect(handler("ControllerBrowse","filteredQuery",{})({type:"albums"},"year-desc","   ",true)).toEqual({type:"albums",sort:"year-desc",likedOnly:true});
});

it("search album navigation clears the search shell before opening the shared detail",()=>{
 const calls:string[]=[];handler("ControllerBrowse","openAlbum",{onNavigate:()=>calls.push("clear"),goToAlbumDetail:(id:number)=>calls.push(`album:${id}`)})(42);
 expect(calls).toEqual(["clear","album:42"]);
});
