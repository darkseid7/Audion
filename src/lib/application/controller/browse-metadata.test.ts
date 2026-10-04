import { afterEach, expect, it, vi } from "vitest";
import { get, writable } from "svelte/store";
import { createControllerBrowseMetadata } from "./browse-metadata";
import { createControllerSession, type ControllerNativeBridge, type ControllerState } from "./session";
import type { BrowseMetadataResult, HostSnapshot } from "../types";
const snapshot: HostSnapshot = { hostId: "pc", hostEpoch: "epoch", revision: 0, revisions: { libraryRevision: 1, queueRevision: 0, outputRevision: 0, settingsRevision: 0 }, playback: { status: "paused", track: null, context: null, position: 0, duration: null, volume: 1, shuffle: false, repeat: "none" }, queue: { count: 0, currentEntryId: null }, output: { kind: "pc" }, outputs: [], capabilities: { queries: ["tracks"], intents: [] }, settings: {}, jobs: [] };
function deferred<T>() { let resolve!: (v:T)=>void; const promise=new Promise<T>(r=>resolve=r); return {promise,resolve}; }
const cleanup: (()=>void)[]=[];
afterEach(()=>{cleanup.splice(0).reverse().forEach(f=>f());vi.useRealTimers();});
async function settle(){for(let i=0;i<30;i++)await Promise.resolve();}
async function setup(){
 vi.useFakeTimers();
 const native={beginScope:vi.fn(async()=>"scope"),connect:vi.fn(async()=>({snapshot,grants:{control:true}})),updateEndpoint:vi.fn(),suspend:vi.fn(),forget:vi.fn(),scan:vi.fn(),pair:vi.fn(),query:vi.fn(),command:vi.fn(),commandStatus:vi.fn(),poll:vi.fn<ControllerNativeBridge["poll"]>(()=>new Promise(()=>{})),media:vi.fn(),browseMetadata:vi.fn<ControllerNativeBridge["browseMetadata"]>(async(_f,r)=>({metadataVersion:1,hostEpoch:r.hostEpoch,libraryRevision:r.libraryRevision,tracks:r.trackIds.map(trackId=>({trackId,playCount:0})),...(r.albumId?{album:{albumId:r.albumId,totalDurationSeconds:202}}:{})}))} satisfies ControllerNativeBridge;
 const session=createControllerSession(native);await session.connectController("pc");
 const visible=writable(true), connection=writable(get(session.state));
 const owner=createControllerBrowseMetadata(()=>session.port,connection,visible);
 cleanup.push(()=>session.suspendController(),()=>owner.dispose());
 return {native,session,owner,visible,connection};
}
it("single-flight and interval starts 5000ms after completion, without page/image reads",async()=>{
 const {native,owner}=await setup(); const held=deferred<BrowseMetadataResult>();
 native.browseMetadata.mockImplementationOnce(()=>held.promise);
 owner.setQuery({trackIds:[42],albumId:8});await settle();expect(native.browseMetadata).toHaveBeenCalledTimes(1);
 await vi.advanceTimersByTimeAsync(15000);expect(native.browseMetadata).toHaveBeenCalledTimes(1);
 held.resolve({metadataVersion:1,hostEpoch:"epoch",libraryRevision:1,tracks:[{trackId:42,playCount:0}],album:{albumId:8,totalDurationSeconds:202}});await settle();
 expect(get(owner.state).counts.get(42)).toBe(0); expect(get(owner.state).album?.totalDurationSeconds).toBe(202);
 native.browseMetadata.mockImplementation(async(_f,r)=>({metadataVersion:1,hostEpoch:r.hostEpoch,libraryRevision:r.libraryRevision,tracks:[{trackId:42,playCount:1}],album:{albumId:8,totalDurationSeconds:202}}));
 await vi.advanceTimersByTimeAsync(4999);expect(native.browseMetadata).toHaveBeenCalledTimes(1);
 await vi.advanceTimersByTimeAsync(1);expect(get(owner.state).counts.get(42)).toBe(1);
 expect(native.query).not.toHaveBeenCalled();expect(native.media).not.toHaveBeenCalled();
});
it("hidden aborts slow responses and resumes fresh, disposal cancels timers",async()=>{
 const {native,owner,visible}=await setup();const held=deferred<BrowseMetadataResult>();native.browseMetadata.mockImplementationOnce(()=>held.promise);
 owner.setQuery({trackIds:[42]});await settle();visible.set(false);
 held.resolve({metadataVersion:1,hostEpoch:"epoch",libraryRevision:1,tracks:[{trackId:42,playCount:12}]});await settle();
 expect(get(owner.state).counts.size).toBe(0);await vi.advanceTimersByTimeAsync(10000);expect(native.browseMetadata).toHaveBeenCalledTimes(1);
 visible.set(true);await settle();expect(get(owner.state).counts.get(42)).toBe(0);
 owner.dispose();await vi.advanceTimersByTimeAsync(10000);expect(native.browseMetadata).toHaveBeenCalledTimes(2);
});
it("filters providers, retains same-scope stale values, skips empty queries",async()=>{
 const {native,owner}=await setup();owner.setQuery({trackIds:[42,-1,42]});await settle();
 expect(native.browseMetadata.mock.calls[0][1].trackIds).toEqual([42]);
 native.browseMetadata.mockRejectedValue({code:"not_found",message:"Missing metadata route",retryable:false});
 await vi.advanceTimersByTimeAsync(5000);expect(get(owner.state)).toMatchObject({stale:true,error:expect.stringContaining("PC")});expect(get(owner.state).counts.get(42)).toBe(0);
 owner.setQuery({trackIds:[]});await settle();expect(get(owner.state).counts.size).toBe(0);await vi.advanceTimersByTimeAsync(10000);expect(native.browseMetadata).toHaveBeenCalledTimes(2);
});
it.each(["host","epoch","library","grant"])("scope change %s clears old values and fences late reads",async(kind)=>{
 const {native,owner,connection}=await setup();const held=deferred<BrowseMetadataResult>();native.browseMetadata.mockImplementationOnce(()=>held.promise);
 owner.setQuery({trackIds:[42]});await settle();
 const c=get(connection), s=structuredClone(c.snapshot!);
 if(kind==="host")s.hostId="other";if(kind==="epoch")s.hostEpoch="other";if(kind==="library")s.revisions.libraryRevision++;
 connection.set({...c,snapshot:s,grants:{control:kind!=="grant"}});await settle();
 held.resolve({metadataVersion:1,hostEpoch:"epoch",libraryRevision:1,tracks:[{trackId:42,playCount:12}]});await settle();
 expect(get(owner.state).counts.size).toBe(0);
});
it("navigation coalesces held generations and removes unrelated counts",async()=>{
 const {native,owner}=await setup();const held=deferred<BrowseMetadataResult>();
 native.browseMetadata.mockImplementationOnce(()=>held.promise);
 owner.setQuery({trackIds:[42],albumId:8});await settle();
 owner.setQuery({trackIds:[43]});await settle();
 expect(native.browseMetadata).toHaveBeenCalledTimes(1);
 held.resolve({metadataVersion:1,hostEpoch:"epoch",libraryRevision:1,tracks:[{trackId:42,playCount:12}],album:{albumId:8,totalDurationSeconds:202}});await settle();
 expect(get(owner.state).counts.has(42)).toBe(false);expect(get(owner.state).counts.get(43)).toBe(0);expect(get(owner.state).album).toBeNull();
});
it("local Busy stays quiet, defers 5 seconds, and current IDs are bounded",async()=>{
 const {native,owner}=await setup();native.browseMetadata.mockRejectedValueOnce({code:"busy",retryable:false});
 owner.setQuery({trackIds:Array.from({length:1001},(_,i)=>i+1)});await settle();
 expect(native.browseMetadata.mock.calls[0][1].trackIds).toHaveLength(1000);
 expect(get(owner.state)).toMatchObject({loading:false,error:""});await vi.advanceTimersByTimeAsync(4999);expect(native.browseMetadata).toHaveBeenCalledTimes(1);
 await vi.advanceTimersByTimeAsync(1);expect(native.browseMetadata).toHaveBeenCalledTimes(2);
});
it("query changes cannot mark retained failed-refresh values fresh",async()=>{
 const {native,owner}=await setup();owner.setQuery({trackIds:[42],albumId:8});await settle();
 native.browseMetadata.mockRejectedValue({code:"host_not_ready",retryable:false});await vi.advanceTimersByTimeAsync(5000);
 expect(get(owner.state).stale).toBe(true);
 const held=deferred<BrowseMetadataResult>();native.browseMetadata.mockImplementationOnce(()=>held.promise);
 owner.setQuery({trackIds:[42,43],albumId:8});await settle();
 expect(get(owner.state).counts.get(42)).toBe(0);expect(get(owner.state).stale).toBe(true);
 held.resolve({metadataVersion:1,hostEpoch:"epoch",libraryRevision:1,tracks:[{trackId:42,playCount:1},{trackId:43,playCount:0}],album:{albumId:8,totalDurationSeconds:202}});await settle();expect(get(owner.state).stale).toBe(false);
});
it("unstructured metadata failures remain inline without an unhandled owner rejection",async()=>{
 const {native,owner}=await setup();native.browseMetadata.mockRejectedValue(null);
 owner.setQuery({trackIds:[42]});await settle();
 expect(get(owner.state)).toMatchObject({loading:false,error:"Could not read PC metadata."});
});
