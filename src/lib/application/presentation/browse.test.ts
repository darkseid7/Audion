import { existsSync, readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import { expect, it, vi } from "vitest";
import type { DisplayAlbum } from "../types";
const path = "src/lib/application/presentation/browse.ts";
const exports: any = {};
if (existsSync(path)) runInNewContext(ts.transpileModule(readFileSync(path,"utf8"),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,require:(id:string)=>{throw Error(`Unexpected presentation dependency:${id}`);}});
const album: DisplayAlbum = {id:7,name:"Kind of Blue",artist:"Miles Davis",year:1959,qualityBadges:["FLAC","96kHz","24bit"],sortSummary:{artist:"Miles Davis",year:1959,dateAdded:null,name:"Kind of Blue"},artwork:{resourceId:"album:7",revision:4}};
it("album_metadata_matches_desktop",()=>{
 expect(exports.toAlbumCard?.(album)).toEqual({id:7,primaryText:"Kind of Blue",secondaryText:"Miles Davis · 1959",qualityBadges:album.qualityBadges,artwork:album.artwork});
});
it("missing_projection_is_not_fabricated",()=>{
 const card=exports.toAlbumCard?.({...album,artist:null,year:null,artwork:undefined});
 expect(card).toBeDefined(); expect(card?.secondaryText).toBe("Unknown Artist");
 expect(card).not.toHaveProperty("liked"); expect(card).not.toHaveProperty("pinned"); expect(card).not.toHaveProperty("trackCount");
});
it("controller_album_gesture_is_host_intent", async()=>{
 const execute=vi.fn(async()=>({status:"applied"}));
 await exports.playAlbumGesture?.(7,"liked_only",{enabled:true},execute);
 expect(execute).toHaveBeenCalledExactlyOnceWith({type:"play_album",albumId:7,playMode:"liked_only"});
 execute.mockClear(); await exports.playAlbumGesture?.(7,"all",{enabled:false,reason:"No control grant"},execute);expect(execute).not.toHaveBeenCalled();
});
it("presentation page mapping preserves authoritative revision, earlier/loading/error and item identity",()=>{
 const items=[album];const state={items,revision:4,hasEarlier:true,nextCursor:"next",loading:true,error:"Busy"};const presentation=exports.toBrowsePresentation?.(state);
 expect(presentation).toEqual({items,revision:4,hasEarlier:true,hasMore:true,loading:true,error:"Busy"});expect(presentation?.items).toBe(items);
});
import * as stores from 'svelte/store';
it("paging_does_not_mix_revisions uses the unchanged real page owner",async()=>{
 const connection=stores.writable({ready:true,snapshot:{hostId:'pc',hostEpoch:'epoch',revisions:{libraryRevision:1,queueRevision:1}}});
 const pending:((value:any)=>void)[]=[];const port={query:vi.fn(()=>new Promise(resolve=>pending.push(resolve)))};
 const pageModule = {} as {createControllerPage: (...args: unknown[]) => ReturnType<typeof import("../controller/views").createControllerPage>};
 const js=ts.transpileModule(readFileSync('src/lib/application/controller/views.ts','utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
 runInNewContext(js,{exports:pageModule,AbortController,require:(id:string)=>{if(id==='svelte/store')return stores;if(id==='./bootstrap')return {controllerState:connection};if(id==='../port')return {getApplicationPort:()=>port};throw Error(`Unexpected page dependency:${id}`);}});
 const page=pageModule.createControllerPage(()=>port,connection);page.setQuery({type:'albums'});
 connection.set({ready:true,snapshot:{hostId:'pc',hostEpoch:'epoch',revisions:{libraryRevision:2,queueRevision:1}}});
 pending[0]({type:'albums',page:{items:[album],revision:1,nextCursor:null}});pending[1]({type:'albums',page:{items:[{...album,id:8}],revision:2,nextCursor:'next'}});await vi.waitFor(()=>expect(stores.get(page.state).revision).toBe(2));
 const current=exports.toBrowsePresentation(stores.get(page.state));expect(current.revision).toBe(2);expect(current.items.map((item:any)=>item.id)).toEqual([8]);expect(current.hasMore).toBe(true);
 page.dispose();expect(stores.get(page.state).items).toEqual([]);
});
