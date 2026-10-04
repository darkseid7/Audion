<script lang="ts">
 import { onDestroy } from "svelte";
 import { controllerState } from "$lib/application/controller/bootstrap";
 import { canExecute } from "$lib/application/capabilities";
 import { createControllerLibraryPage } from "$lib/stores/library";
 import { createControllerSearchPage } from "$lib/stores/search";
 import { goToAlbumDetail, goToArtistDetail, goToPlaylistDetail } from "$lib/stores/view";
 import type { ApplicationQuery, ApplicationIntent, ExecutionResult, DisplayTrack, DisplayAlbum, DisplayArtist, DisplayPlaylist, SearchMatch, PlaybackContext, AlbumSort, AlbumView } from "$lib/application/types";
 import ControllerArtwork from "./ControllerArtwork.svelte";
 import MediaCard from "./MediaCard.svelte";
 export let query: ApplicationQuery = {type:"tracks"};
 export let heading = "Tracks";
 export let onNavigate: () => void = () => {};
 function openAlbum(id: number) { onNavigate(); goToAlbumDetail(id); }
 function openArtist(name: string) { onNavigate(); goToArtistDetail(name); }
 function openPlaylist(id: number, name: string) { onNavigate(); goToPlaylistDetail(id, name); }
 export let context: PlaybackContext | null = null;
 export let execute: (intent: ApplicationIntent) => Promise<ExecutionResult>;
 export let enqueue: (query: ApplicationQuery, placement: "next" | "after_user_queue" | "end") => Promise<ExecutionResult>;
 export let layout: AlbumView = "grid";
 export let selectLayout: (value: AlbumView) => void = value => layout=value;
 const details = createControllerLibraryPage(), detailState = details.state;
 $: if (query.type === "album_tracks") details.setQuery({type:"album_detail",albumId:query.albumId});
 const page = query.type === "search" ? createControllerSearchPage() : createControllerLibraryPage(), state=page.state;
 let sort: AlbumSort="artist-asc", text="", likedOnly=false;
  function filteredQuery(query: ApplicationQuery, sort: AlbumSort, text: string, likedOnly: boolean): ApplicationQuery {
  return query.type === "albums" ? { ...query, sort, likedOnly, ...(text.trim() ? {text:text.trim()} : {}) } : query;
 }
 $: request=filteredQuery(query,sort,text,likedOnly);
 $: page.setQuery(request);
 $: albums = $state.items as DisplayAlbum[];
 $: tracks = $state.items as DisplayTrack[];
 $: artists = $state.items as DisplayArtist[];
 $: playlists = $state.items as DisplayPlaylist[];
 $: matches = $state.items as SearchMatch[];
 $: entityContext=context?.type==="album" ? {...context,playMode:likedOnly ? "liked_only" as const : "all" as const} : context;
 function play(trackId?:number) {
  if(entityContext?.type==="album") return execute({type:"play_album",albumId:entityContext.albumId,playMode:entityContext.playMode,...(trackId===undefined?{}:{startTrackId:trackId})});
  if(entityContext?.type==="artist") return execute({type:"play_artist",artistName:entityContext.artistName,...(trackId===undefined?{}:{startTrackId:trackId})});
  if(entityContext?.type==="playlist") return execute({type:"play_playlist",playlistId:entityContext.playlistId,...(trackId===undefined?{}:{startTrackId:trackId})});
  if(entityContext?.type==="liked") return execute({type:"play_liked",...(trackId===undefined?{}:{startTrackId:trackId})});
  if(trackId!==undefined)return execute({type:"play_track",trackId});
 }
 function playAlbumCard(albumId:number) {
  const current=$controllerState.snapshot?.playback;
  return current?.context?.type==="album" && current.context.albumId===albumId ? execute({type:current.status==="playing"?"pause":"resume"}) : execute({type:"play_album",albumId,playMode:"all"});
 }
 async function shufflePlay() { const result=await execute({type:"set_shuffle",enabled:true}); if(result.status==="applied") await play(); }
 function enqueueEntity(placement: "next" | "after_user_queue" | "end") {
  if(entityContext && ["album","artist","playlist","liked"].includes(entityContext.type)) return execute({type:"queue_entity",entity:entityContext as import("$lib/application/types").QueueEntity,placement});
 }
 const trackTitle=(track:DisplayTrack)=>track.title??"Untitled";
 onDestroy(()=>{page.dispose();details.dispose();});
</script>
<section class="browse-view" aria-label={heading}>
 <header><span class="eyebrow">YOUR PC LIBRARY</span><h1>{$detailState.detail?.album.name ?? heading}</h1>{#if $detailState.detail}<p>{$detailState.detail.album.artist} · {$detailState.detail.trackCount} tracks · {$detailState.detail.album.year ?? ""}</p>{/if}</header>
 {#if query.type==="albums"}<div class="toolbar">
  <input aria-label="Filter PC albums" placeholder="Filter albums" bind:value={text} />
  <select aria-label="Sort PC albums" bind:value={sort}><option value="artist-asc">Artist A–Z</option><option value="artist-desc">Artist Z–A</option><option value="year-desc">Newest year</option><option value="year-asc">Oldest year</option><option value="added-desc">Recently added</option><option value="added-asc">Oldest added</option><option value="name-asc">Name A–Z</option><option value="name-desc">Name Z–A</option></select>
  <label><input type="checkbox" bind:checked={likedOnly} /> Liked albums</label>
  <div role="group" aria-label="Album view"><button aria-label="Grid view" aria-pressed={layout==="grid"} on:click={()=>selectLayout("grid")}>▦</button><button aria-label="List view" aria-pressed={layout==="list"} on:click={()=>selectLayout("list")}>☰</button></div>
 </div>{/if}
 {#if context}<div class="toolbar"><button class="primary" disabled={!canExecute($controllerState,context.type==="album"?"play_album":context.type==="artist"?"play_artist":context.type==="playlist"?"play_playlist":"play_liked")} on:click={()=>play()}>▶ Play</button>
 <button disabled={!canExecute($controllerState,"set_shuffle")} on:click={shufflePlay}>Shuffle play</button>
 <button disabled={!canExecute($controllerState,"queue_entity")} on:click={()=>enqueueEntity("next")}>Play next</button><button disabled={!canExecute($controllerState,"queue_entity")} on:click={()=>enqueueEntity("after_user_queue")}>Add to queue</button><button disabled={!canExecute($controllerState,"queue_entity")} on:click={()=>enqueueEntity("end")}>Add to end</button>
 {#if context.type==="album"}<label><input type="checkbox" bind:checked={likedOnly} /> Liked tracks only</label>{/if}
 </div>{/if}
 {#if query.type==="albums" || query.type==="artist_albums"}<div class="album-grid-body" class:list={layout==="list"}>
 {#each albums as album (album.id)}<div class="album-item">
  <MediaCard primaryText={album.name} secondaryText={album.artist??"Unknown artist"} {layout} ariaLabel={`Play ${album.name}`} on:click={()=>openAlbum(album.id)} on:play={()=>playAlbumCard(album.id)} on:pause={()=>execute({type:"pause"})} isNowPlaying={$controllerState.snapshot?.playback.context?.type==="album" && $controllerState.snapshot.playback.context.albumId===album.id} isPaused={$controllerState.snapshot?.playback.status!=="playing"}>
   <svelte:fragment slot="cover"><ControllerArtwork reference={album.artwork} alt={album.name} /></svelte:fragment><svelte:fragment slot="extra-info"><small>{album.qualityBadges.join(" · ")}</small></svelte:fragment>
  </MediaCard><div class="album-actions"><button disabled={!canExecute($controllerState,"queue_entity")} on:click={()=>enqueue({type:"album_tracks",albumId:album.id},"next")}>Play next</button><button disabled={!canExecute($controllerState,"queue_entity")} on:click={()=>enqueue({type:"album_tracks",albumId:album.id},"end")}>Add to end</button></div>
 </div>{/each}</div>
 {:else if query.type==="artists"}<div class="entity-list">{#each artists as artist}<button on:click={()=>openArtist(artist.name)}><strong>{artist.name}</strong><small>{artist.albumCount} albums · {artist.trackCount} tracks</small></button>{/each}</div>
 {:else if query.type==="playlists"}<div class="entity-list">{#each playlists as playlist}<button on:click={()=>openPlaylist(playlist.id,playlist.name)}><strong>{playlist.name}</strong><small>{playlist.trackCount} tracks</small></button>{/each}</div>
 {:else if query.type==="search"}<div class="entity-list">{#each matches as match}{#if match.type==="album"}<button on:click={()=>openAlbum(match.album.id)}>{match.album.name}<small>Album · {match.album.artist}</small></button>{:else if match.type==="artist"}<button on:click={()=>openArtist(match.artist.name)}>{match.artist.name}<small>Artist</small></button>{:else}<button disabled={!canExecute($controllerState,"play_track")} on:click={()=>execute({type:"play_track",trackId:match.track.id})}>{trackTitle(match.track)}<small>{match.track.artist}</small></button>{/if}{/each}</div>
 {:else}<div class="track-list">{#each tracks.filter(t=>context?.type!=="album" || !likedOnly || t.liked) as track, trackIndex (trackIndex)}
  <div class="track-row"><div class="art"><ControllerArtwork reference={track.artwork} /></div><button class="track-title" disabled={!canExecute($controllerState,context?.type==="album"?"play_album":context?.type==="artist"?"play_artist":context?.type==="playlist"?"play_playlist":context?.type==="liked"?"play_liked":"play_track")} on:click={()=>play(track.id)}><strong>{trackTitle(track)}</strong><small>{track.artist} · {track.album}</small></button><span class="quality">{track.quality.badges.join(" · ")}</span><button aria-label={`Play ${trackTitle(track)} next`} disabled={!canExecute($controllerState,"queue_insert")} on:click={()=>execute({type:"queue_insert",trackIds:[track.id],placement:"next"})}>Next</button><button aria-label={`Add ${trackTitle(track)} to queue`} disabled={!canExecute($controllerState,"queue_insert")} on:click={()=>execute({type:"queue_insert",trackIds:[track.id],placement:"after_user_queue"})}>+</button><button aria-label={`Append ${trackTitle(track)} to queue`} disabled={!canExecute($controllerState,"queue_append")} on:click={()=>execute({type:"queue_append",trackIds:[track.id]})}>End</button></div>
 {/each}</div>{/if}
 {#if $state.error}<p role="alert">{$state.error}</p><button on:click={page.refresh}>Refresh</button>{/if}
 {#if $state.loading}<p role="status">Loading from PC…</p>{:else if !$state.items.length && !$state.error}<p>No items in this view.</p>{/if}
 {#if $state.hasEarlier}<p>Showing the latest 1,000 loaded entries.</p><button on:click={page.refresh}>Back to first page</button>{/if}
 {#if $state.nextCursor}<button class="load-more" on:click={page.more} disabled={$state.loading}>Load more</button>{/if}
</section>
<style>.browse-view{padding:var(--spacing-lg);height:100%;overflow:auto}header{margin:12px 0 24px}.eyebrow{font-size:.65rem;letter-spacing:.12em;color:var(--text-secondary)}h1{font-size:clamp(1.8rem,4vw,3rem);margin:8px 0;font-weight:750;letter-spacing:-.03em}.toolbar{display:flex;align-items:center;gap:10px;flex-wrap:wrap;margin-bottom:20px}.toolbar label{font-size:.85rem}.album-grid-body{display:grid;grid-template-columns:repeat(auto-fill,minmax(155px,1fr));gap:20px}.album-grid-body.list{grid-template-columns:1fr;gap:6px}.album-actions{display:flex;justify-content:space-between;font-size:.7rem}.track-row{display:flex;align-items:center;gap:12px;min-height:58px;border-bottom:1px solid var(--border-subtle)}.art{width:40px;height:40px;flex:none;border-radius:var(--radius-sm);overflow:hidden}.track-title{flex:1;min-width:0;text-align:left}strong,small{display:block;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}small,p,.quality{color:var(--text-secondary);font-size:.8rem}.entity-list{display:flex;flex-direction:column}.entity-list button{text-align:left;padding:16px}button,input,select{font:inherit;border:1px solid var(--border-subtle);border-radius:var(--radius-sm);padding:8px 12px;color:var(--text-primary);background:var(--bg-highlight)}button{cursor:pointer}.track-row button,.album-actions button{border:0;background:none;padding:8px}.primary,button[aria-pressed=true]{background:var(--accent-primary);color:var(--bg-base)}button:disabled{opacity:.4;cursor:not-allowed}.load-more{display:block;margin:24px auto}@media(max-width:650px){.browse-view{padding:16px}.quality{display:none}.track-row{gap:6px}.track-row button{padding:8px 4px}.album-grid-body{grid-template-columns:repeat(2,minmax(0,1fr));gap:12px}}@media(prefers-reduced-motion:reduce){*{animation:none!important;transition:none!important}}</style>
