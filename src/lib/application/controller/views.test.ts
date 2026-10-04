import { it, expect, vi } from "vitest";
import { get, writable } from "svelte/store";
import { createControllerPage } from "./views";
import type { ControllerState } from "./session";
it("pages using host cursors and drops late results after host or library changes", async () => {
 const state = writable({ ready: true, snapshot: { hostId: "one", hostEpoch: "epoch", revisions: { libraryRevision: 1, queueRevision: 2 } } } as ControllerState);
 let release!: (value: any) => void;
 const query = vi.fn().mockResolvedValueOnce({ type: "tracks", page: { items: [{ id: 8 }], nextCursor: "host-cursor", revision: 1 } }).mockImplementationOnce(() => new Promise(r => release = r));
 const page = createControllerPage(() => ({ query } as any), state);
 page.setQuery({ type: "tracks" }); await vi.waitFor(() => expect(get(page.state).items).toEqual([{ id: 8 }]));
 const next = page.more(); expect(query.mock.calls[1][0]).toEqual({ type: "tracks", limit: 100, cursor: "host-cursor" });
 state.set({ ready: false, snapshot: null } as ControllerState);
 release({ type: "tracks", page: { items: [{ id: 9 }], nextCursor: null, revision: 1 } }); await next;
 expect(get(page.state).items).toEqual([]); page.dispose();
});
it("rejects pages with unconfirmed revisions and clears old presentation on query changes", async () => {
 const state = writable({ ready: true, snapshot: { hostId: "one", hostEpoch: "epoch", revisions: { libraryRevision: 4, queueRevision: 2 } } } as ControllerState);
 const query = vi.fn().mockResolvedValue({ type: "albums", page: { items: [{ id: 8 }], nextCursor: null, revision: 3 } });
 const page = createControllerPage(() => ({ query } as any), state); page.setQuery({ type: "albums", sort: "year-desc", likedOnly: true });
 await vi.waitFor(() => expect(get(page.state).error).toBeTruthy()); expect(get(page.state).items).toEqual([]); page.dispose();
});

it("loads authoritative album detail at the current library revision",async()=>{
 const state=writable({ready:true,snapshot:{hostId:"one",hostEpoch:"epoch",revisions:{libraryRevision:4,queueRevision:2}}} as ControllerState);
 const detail={album:{id:42,name:"PC album",artist:"Ada",year:2000,qualityBadges:[],sortSummary:{artist:"Ada",year:2000,dateAdded:null,name:"PC album"}},originalYear:1999,trackCount:201,liked:true};
 const page=createControllerPage(()=>({query:async()=>({type:"album_detail",detail,revision:4})} as any),state);
 page.setQuery({type:"album_detail",albumId:42});await vi.waitFor(()=>expect(get(page.state).detail).toEqual(detail));page.dispose();
});

it("switching album liked filter discards late all-member pages",async()=>{
 const state=writable({ready:true,snapshot:{hostId:"one",hostEpoch:"epoch",revisions:{libraryRevision:4,queueRevision:2}}} as ControllerState);
 let finish!: (v:any)=>void;
 const query=vi.fn().mockImplementationOnce(()=>new Promise(r=>finish=r)).mockResolvedValueOnce({type:"album_tracks",page:{items:[{id:201,liked:true}],nextCursor:null,revision:4}});
 const page=createControllerPage(()=>({query} as any),state);
 page.setQuery({type:"album_tracks",albumId:42,likedOnly:false});
 page.setQuery({type:"album_tracks",albumId:42,likedOnly:true});
 await vi.waitFor(()=>expect(get(page.state).items).toEqual([{id:201,liked:true}]));
 finish({type:"album_tracks",page:{items:[{id:1,liked:false}],nextCursor:"all-page",revision:4}});
 await Promise.resolve();expect(get(page.state).items).toEqual([{id:201,liked:true}]);expect(get(page.state).nextCursor).toBeNull();page.dispose();
});
