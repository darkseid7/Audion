import { it, expect, vi } from "vitest";
import { get, writable } from "svelte/store";
import { createControllerPage } from "./views";
import type { ControllerState } from "./session";
it("keeps an entity presentation key across opaque artwork rotation, never across different entities", async () => {
 const initial = { ready: true, snapshot: { hostId: "one", hostEpoch: "epoch", revisions: { libraryRevision: 1, queueRevision: 2 } } } as ControllerState;
 const connection = writable(initial), album = { id: 8, name: "Album", artist: "Ada", qualityBadges: [], sortSummary: {}, artwork: { resourceId: "token-one", revision: 1 } };
 const query = vi.fn().mockResolvedValueOnce({ type: "albums", page: { items: [album], nextCursor: null, revision: 1 } }).mockResolvedValueOnce({ type: "albums", page: { items: [{ ...album, artwork: { resourceId: "token-two", revision: 2 } }, { ...album, id: 9, artwork: { resourceId: "token-three", revision: 2 } }], nextCursor: null, revision: 2 } });
 const page = createControllerPage(() => ({ query } as any), connection); page.setQuery({ type: "albums" });
 await vi.waitFor(() => expect(get(page.state).items).toHaveLength(1));
 const before = (get(page.state).items[0] as any).artwork;
 expect(before.presentationKey).toBeTruthy();
 connection.set({ ...initial, snapshot: { ...initial.snapshot!, revisions: { ...initial.snapshot!.revisions, libraryRevision: 2 } } });
 await vi.waitFor(() => expect(get(page.state).items).toHaveLength(2));
 const after = get(page.state).items as any[];
 expect(after[0].artwork.presentationKey).toBe(before.presentationKey); expect(after[0].artwork.resourceId).toBe("token-two");
 expect(after[1].artwork.presentationKey).not.toBe(before.presentationKey); page.dispose();
});
it("retains the visible library page during compatible revision refresh, but drops old cursors", async () => {
 const initial = { ready: true, snapshot: { hostId: "one", hostEpoch: "epoch", revisions: { libraryRevision: 1, queueRevision: 2 } } } as ControllerState;
 const connection = writable(initial);
 let finish!: (v: any) => void;
 const query = vi.fn().mockResolvedValueOnce({ type: "albums", page: { items: [{ id: 8 }], nextCursor: "old", revision: 1 } }).mockImplementationOnce(() => new Promise(r => finish = r));
 const page = createControllerPage(() => ({ query } as any), connection);
 page.setQuery({ type: "albums" }); await vi.waitFor(() => expect(get(page.state).items).toEqual([{ id: 8 }]));
 connection.set({ ...initial, snapshot: { ...initial.snapshot!, revisions: { ...initial.snapshot!.revisions, libraryRevision: 2 } } });
 expect(get(page.state)).toMatchObject({ items: [{ id: 8 }], loading: false, nextCursor: null });
 await page.more(); expect(query).toHaveBeenCalledTimes(2); expect(query.mock.calls[1][0]).not.toHaveProperty("cursor");
 finish({ type: "albums", page: { items: [{ id: 9 }], nextCursor: null, revision: 2 } });
 await vi.waitFor(() => expect(get(page.state).items).toEqual([{ id: 9 }])); page.dispose();
});
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
