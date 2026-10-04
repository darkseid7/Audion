import { describe, it, expect, vi } from "vitest";
import { get, writable } from "svelte/store";
import { createViewActions } from "./view-actions";
import type { ControllerState } from "./controller/session";
import type { ApplicationPort, HostSnapshot } from "./types";
const snapshot = (): HostSnapshot => ({ hostId: "PC-id", hostEpoch: "epoch", revision: 8,
 revisions: { libraryRevision: 4, outputRevision: 3, queueRevision: 7, settingsRevision: 0 },
 playback: { status: "paused", track: null, context: null, position: 5, duration: 200, volume: .4, shuffle: false, repeat: "none" },
 queue: { count: 2, currentEntryId: "occurrence-a" }, output: { kind: "pc" },
 outputs: [{ output: { kind: "pc" }, name: "Studio PC", available: true, capabilities: { playback: true, seek: true, volume: true, shuffle: true, repeat: true, equalizer: false } }],
 capabilities: { queries: ["queue", "album_tracks"], intents: ["play_album", "play_playlist", "play_artist", "play_liked", "play_track", "queue_entity", "queue_insert", "queue_append", "queue_play", "queue_remove", "queue_reorder", "queue_clear_upcoming", "set_volume", "set_shuffle", "set_repeat", "seek", "pause", "resume", "next", "previous", "select_output"] }, settings: {}, jobs: [] });
function setup() {
 const state = writable<ControllerState>({ currentHostId: "PC-id", snapshot: snapshot(), grants: { control: true }, ready: true, status: "connected" });
 const execute = vi.fn<ApplicationPort["execute"]>(async () => ({ status: "applied", revision: 9 }));
 const port = { execute } as unknown as ApplicationPort;
 return { state, execute, actions: createViewActions(() => port, state) };
}
describe("confirmed view intents", () => {
 it("plays a PC album and selected track with liked mode and confirmed revisions, never Track arrays", async () => {
  const { actions, execute } = setup();
  await actions.playAlbum(42, "liked_only", 90);
  expect(execute).toHaveBeenCalledExactlyOnceWith({ type: "play_album", albumId: 42, playMode: "liked_only", startTrackId: 90 }, { hostEpoch: "epoch", libraryRevision: 4, outputRevision: 3 });
  actions.dispose();
 });
 it("keeps volume and shuffle acknowledged, using host duration for seek", async () => {
  const { actions, state, execute } = setup();
  await actions.setVolume(.8); await actions.setShuffle(true); await actions.seek(.25);
  expect(execute.mock.calls.map(c => c[0])).toEqual([{ type: "set_volume", volume: .8 }, { type: "set_shuffle", enabled: true }, { type: "seek", seconds: 50 }]);
  expect(get(state).snapshot?.playback).toMatchObject({ volume: .4, shuffle: false, position: 5 });
  actions.dispose();
 });
 it("selects/removes/reorders duplicate occurrences by entry ID and queue revision", async () => {
  const { actions, execute } = setup();
  await actions.playQueueEntry("occurrence-b"); await actions.removeQueueEntry("occurrence-b"); await actions.reorderQueueEntry("occurrence-b", "occurrence-a");
  expect(execute.mock.calls).toEqual([
   [{ type: "queue_play", entryId: "occurrence-b" }, { hostEpoch: "epoch", queueRevision: 7, outputRevision: 3 }],
   [{ type: "queue_remove", entryId: "occurrence-b" }, { hostEpoch: "epoch", queueRevision: 7, outputRevision: 3 }],
   [{ type: "queue_reorder", entryId: "occurrence-b", beforeEntryId: "occurrence-a" }, { hostEpoch: "epoch", queueRevision: 7, outputRevision: 3 }]
  ]); actions.dispose();
 });
 it.each(["offline", "denied", "unsupported"])("fails closed for %s without dispatch", async reason => {
  const { actions, state, execute } = setup();
  state.update(s => ({ ...s, ready: reason !== "offline", grants: { control: reason !== "denied" }, snapshot: reason === "unsupported" ? { ...s.snapshot!, output: { kind: "desktop_only", reason: "Cloud playback" } } : s.snapshot }));
  expect(actions.canExecute("play_album")).toBe(false);
  expect((await actions.playAlbum(42, "all")).status).toBe("failed");
  expect(execute).not.toHaveBeenCalled(); actions.dispose();
 });
 it("accepted jobs stay pending until confirmed completion and unknown is never replayed", async () => {
  const { actions, state, execute } = setup();
  execute.mockResolvedValueOnce({ status: "accepted", jobId: "job", revision: 9 });
  await actions.playAlbum(42, "all"); expect(get(actions.feedback).status).toBe("pending");
  state.update(s => ({ ...s, snapshot: { ...s.snapshot!, jobs: [{ jobId: "job", status: "completed", result: { status: "applied", revision: 10 } }] } }));
  expect(get(actions.feedback).status).toBe("applied");
  execute.mockRejectedValueOnce({ code: "outcome_unknown", message: "Unknown outcome", retryable: false });
  await actions.setVolume(.9); expect(get(actions.feedback).status).toBe("unknown"); expect(execute).toHaveBeenCalledTimes(2);
  actions.dispose();
 });
});

it("queues an entity once with all three authoritative revisions, never collecting phone pages",async()=>{
 const {actions,execute}=setup();await actions.queueQuery({type:"album_tracks",albumId:42},"next");
 expect(execute).toHaveBeenCalledExactlyOnceWith({type:"queue_entity",entity:{type:"album",albumId:42,playMode:"all"},placement:"next"},{hostEpoch:"epoch",libraryRevision:4,queueRevision:7,outputRevision:3});actions.dispose();
});
it.each([NaN,Infinity,-1])( "rejects invalid view seek %s before calling the port",async value=>{
 const {actions,execute}=setup();expect(await actions.seek(value)).toMatchObject({status:"failed",error:{code:"invalid_request"}});expect(execute).not.toHaveBeenCalled();actions.dispose();
});

it.each(["execution_failed","outcome_unknown"] as const)("retains earlier %s after a later action finishes",async code=>{
 const {actions,execute}=setup();let finish!: (result:any)=>void;
 execute.mockImplementationOnce(()=>new Promise(r=>finish=r));
 const first=actions.playAlbum(42,"all");await actions.setVolume(.8);
 expect(get(actions.outcomes).map(x=>x.status)).toEqual(["pending","applied"]);
 finish({status:"failed",error:{code,message:"Earlier action failed",retryable:false},revision:9,partialEffects:["Output stopped"]});await first;
 expect(get(actions.outcomes)).toEqual(expect.arrayContaining([expect.objectContaining({status:code==="outcome_unknown"?"unknown":"error",message:expect.stringContaining("Output stopped")}),expect.objectContaining({status:"applied"})]));
 actions.dispose();
});
it("tracks both accepted jobs independently and declines at capacity without IPC",async()=>{
 const {actions,state,execute}=setup();
 execute.mockResolvedValueOnce({status:"accepted",jobId:"a",revision:9}).mockResolvedValueOnce({status:"accepted",jobId:"b",revision:9});
 await actions.playAlbum(1,"all");await actions.setVolume(.2);
 state.update(s=>({...s,snapshot:{...s.snapshot!,jobs:[{jobId:"b",status:"completed",result:{status:"applied",revision:10}},{jobId:"a",status:"completed",result:{status:"failed",revision:10,error:{code:"outcome_unknown",message:"Unknown first",retryable:false},partialEffects:[]}}]}}));
 expect(get(actions.outcomes).map(x=>x.status)).toEqual(["unknown","applied"]);
 execute.mockResolvedValue({status:"accepted",jobId:"held",revision:10});
 for(let i=0;i<40;i++)await actions.setVolume(.3);
 expect(execute).toHaveBeenCalledTimes(33);
 expect(get(actions.outcomes)).toHaveLength(32);expect(get(actions.admissionError)).toContain("Dismiss");
 expect(get(actions.outcomes)[0].status).toBe("unknown");actions.dismiss(get(actions.outcomes)[0].id);
 await actions.setVolume(.4);expect(execute).toHaveBeenCalledTimes(34);actions.dispose();
});
