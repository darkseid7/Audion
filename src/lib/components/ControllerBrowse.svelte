<script lang="ts">
 import { onDestroy } from "svelte";
 import { controllerState } from "$lib/application/controller/bootstrap";
 import { createControllerBrowseMetadata } from "$lib/application/controller/browse-metadata";
 import { canExecute } from "$lib/application/capabilities";
 import { createControllerLibraryPage } from "$lib/stores/library";
 import { createControllerSearchPage } from "$lib/stores/search";
 import { goBack, goToAlbumDetail, goToArtistDetail, goToPlaylistDetail } from "$lib/stores/view";
 import type { ApplicationQuery, ApplicationIntent, ExecutionResult, DisplayTrack, DisplayAlbum, DisplayArtist, DisplayPlaylist, SearchMatch, PlaybackContext, AlbumSort, AlbumView } from "$lib/application/types";
 import ControllerArtwork from "./ControllerArtwork.svelte";
 import type { BrowseContextAction } from "$lib/application/presentation/types";
 import EntityDetail from "./presentation/EntityDetail.svelte";
 import TrackRows from "./presentation/TrackRows.svelte";
 import AlbumBrowser from "./presentation/AlbumBrowser.svelte";
 import VirtualizedGrid from "./Virtualizedgrid.svelte";
 import { toAlbumCard, toBrowsePresentation, playAlbumGesture, albumSortLabels, albumSortOptions } from "$lib/application/presentation/browse";
 import EntityGrid from "./presentation/EntityGrid.svelte";
 export let query: ApplicationQuery = {type:"tracks"};
 export let heading = "Tracks";
 export let onNavigate: () => void = () => {};
 function dispatch(intent: ApplicationIntent) { if (canExecute($controllerState,intent.type)) return execute(intent); }
 function albumClick(album: DisplayAlbum, event: MouseEvent) { if (!(event.target as HTMLElement)?.closest("[data-mediacard-play]")) openAlbum(album.id); }
 function navigateBack() { onNavigate(); goBack(); }
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
 const metadata = createControllerBrowseMetadata(), metadataState = metadata.state;
 $: confirmedMetadataPage = $controllerState.ready && $state.revision !== null && $state.revision === $controllerState.snapshot?.revisions.libraryRevision;
 $: metadata.setQuery({trackIds: confirmedMetadataPage && ["tracks","album_tracks","artist_tracks","playlist_tracks","liked_tracks"].includes(query.type) ? $state.items.map(item=>"id" in item ? item.id : -1).filter(id=>Number.isSafeInteger(id) && id>0) : [], ...(confirmedMetadataPage && query.type === "album_tracks" ? {albumId:query.albumId} : {})});
 $: albumDuration = context?.type === "album" && $metadataState.album?.albumId === context.albumId ? $metadataState.album.totalDurationSeconds : null;
 function formatAlbumDuration(seconds:number) { return `${Math.floor(seconds/60)}:${String(Math.floor(seconds%60)).padStart(2,"0")}`; }
 let sort: AlbumSort="added-desc", text="", likedOnly=false;
  function filteredQuery(query: ApplicationQuery, sort: AlbumSort, text: string, likedOnly: boolean): ApplicationQuery {
  if (query.type === "album_tracks") return { ...query, likedOnly };
  return query.type === "albums" ? { ...query, sort, likedOnly, ...(text.trim() ? {text:text.trim()} : {}) } : query;
 }
 $: request=filteredQuery(query,sort,text,likedOnly);
 $: page.setQuery(request);
 $: browsePresentation = toBrowsePresentation($state);
 let currentScrollTop = 0;
 function selectSort(value: AlbumSort) { sort = value; }
 async function loadMore(): Promise<boolean> { await page.more(); return !!$state.nextCursor; }
 $: albums = $state.items as DisplayAlbum[];
 $: tracks = $state.items as DisplayTrack[];
 $: artists = $state.items as DisplayArtist[];
 $: playlists = $state.items as DisplayPlaylist[];
 $: matches = $state.items as SearchMatch[];
 $: entityContext=context?.type==="album" ? {...context,playMode:likedOnly ? "liked_only" as const : "all" as const} : context;
 function play(trackId?:number) {
  if(entityContext?.type==="album") return dispatch({type:"play_album",albumId:entityContext.albumId,playMode:entityContext.playMode,...(trackId===undefined?{}:{startTrackId:trackId})});
  if(entityContext?.type==="artist") return dispatch({type:"play_artist",artistName:entityContext.artistName,...(trackId===undefined?{}:{startTrackId:trackId})});
  if(entityContext?.type==="playlist") return dispatch({type:"play_playlist",playlistId:entityContext.playlistId,...(trackId===undefined?{}:{startTrackId:trackId})});
  if(entityContext?.type==="liked") return dispatch({type:"play_liked",...(trackId===undefined?{}:{startTrackId:trackId})});
  if(trackId!==undefined)return dispatch({type:"play_track",trackId});
 }
 function playAlbumCard(albumId:number) {
  const current=$controllerState.snapshot?.playback;
  return current?.context?.type==="album" && current.context.albumId===albumId ? dispatch({type:current.status==="playing"?"pause":"resume"}) : playAlbumGesture(albumId,"all",availability("play_album"),execute);
 }
 async function shufflePlay() { const result=await dispatch({type:"set_shuffle",enabled:true}); if(result?.status==="applied") await play(); }
 function enqueueEntity(placement: "next" | "after_user_queue" | "end") {
  if(entityContext && ["album","artist","playlist","liked"].includes(entityContext.type)) return dispatch({type:"queue_entity",entity:entityContext as import("$lib/application/types").QueueEntity,placement});
 }
 function availability(type: ApplicationIntent["type"]) { return canExecute($controllerState,type) ? {enabled:true as const} : {enabled:false as const,reason:"This command requires permission and support from the connected PC."}; }
 function albumActions(albumId: number, artistName?: string | null): BrowseContextAction[] { return [
 ...(artistName ? [{id:"artist",label:"View Artist",availability:{enabled:true as const},run:()=>openArtist(artistName)}] : []),
 ...(["next", "after_user_queue", "end"] as const).map(placement=>({id:placement,label:placement==="next"?"Play Next":placement==="end"?"Add to End":"Add to Queue",availability:availability("queue_entity"),run:()=>dispatch({type:"queue_entity",entity:{type:"album",albumId,playMode:"all"},placement})})),
 {id:"pin",label:"Pin to Top",availability:{enabled:false,reason:"Album pin state is not supplied by the PC projection."}},
 {id:"liked",label:"Like Album",availability:{enabled:false,reason:"Album like state and changes are not supplied by this projection."}},
 {id:"delete",label:"Delete Album",availability:{enabled:false,reason:"Deleting files is managed on the PC."}}
 ]; }
 function playIntent(): ApplicationIntent["type"] { return context?.type==="album"?"play_album":context?.type==="artist"?"play_artist":context?.type==="playlist"?"play_playlist":context?.type==="liked"?"play_liked":"play_track"; }
 function queueTrack(id:number,placement:"next"|"after_user_queue"|"end") { return placement==="end" ? dispatch({type:"queue_append",trackIds:[id]}) : dispatch({type:"queue_insert",trackIds:[id],placement}); }
 const trackTitle=(track:DisplayTrack)=>track.title??"Untitled";
 onDestroy(()=>{page.dispose();details.dispose();metadata.dispose();});
</script>
<section class="browse-view" class:album-detail-scroll={context?.type === "album"} class:albums-route={query.type === "albums"} aria-label={heading}>
 {#if query.type === "albums"}
 <AlbumBrowser albumView={layout} bind:searchQuery={text} bind:showOnlyFavorites={likedOnly} albumSort={sort} sortOptions={albumSortOptions} selectedSortLabel={albumSortLabels[sort]} selectAlbumView={selectLayout} {selectSort}>
 <VirtualizedGrid items={albums} {layout} bind:currentScrollTop initialScrollTop={currentScrollTop} onItemClick={albumClick} onLoadMore={loadMore} hasMore={browsePresentation.hasMore} cardWidthDesktop={240} cardWidthMobile={170} cardHeightDesktop={layout === "list" ? 104 : 380} cardHeightMobile={layout === "list" ? 104 : 305} gridGapDesktop={layout === "list" ? 8 : 24} gridGapMobile={8} let:item={album}>
 {@const card = toAlbumCard(album)}
 <EntityGrid albumMetadata={true} {layout} playAvailability={canExecute($controllerState,"play_album") ? {enabled:true} : {enabled:false,reason:"Playback requires control permission from the PC."}} actions={albumActions(album.id,album.artist)} primaryText={card.primaryText} secondaryText={card.secondaryText} ariaLabel={album.name} playTooltip="Play album" resumeTooltip="Resume album" pauseTooltip="Pause" onPlay={()=>playAlbumCard(album.id)} on:pause={()=>dispatch({type:"pause"})} isNowPlaying={$controllerState.snapshot?.playback.context?.type==="album" && $controllerState.snapshot.playback.context.albumId===album.id && $controllerState.snapshot.playback.status==="playing"} isPaused={$controllerState.snapshot?.playback.context?.type==="album" && $controllerState.snapshot.playback.context.albumId===album.id && $controllerState.snapshot.playback.status==="paused"}>
 <svelte:fragment slot="cover"><ControllerArtwork reference={album.artwork} alt={album.name}/></svelte:fragment>
 <svelte:fragment slot="extra-info">{#if card.qualityBadges.length}<div class="audio-chips">{#each card.qualityBadges as badge,index}<span class="audio-chip" class:format={index===0}>{badge}</span>{/each}</div>{/if}</svelte:fragment>
 </EntityGrid>
 </VirtualizedGrid>
 </AlbumBrowser>
 {#if browsePresentation.error}<p role="alert">{browsePresentation.error}</p><button on:click={page.refresh}>Refresh</button>{/if}
 {#if browsePresentation.loading}<p role="status">Loading from PC…</p>{:else if !albums.length && !browsePresentation.error}<p>No albums found</p>{/if}
 {#if browsePresentation.hasEarlier}<p>Showing the latest 1,000 loaded entries.</p><button on:click={page.refresh}>Back to first page</button>{/if}
 {:else}
 {#if context}
 {@const kind = context.type === "album" ? "album" : context.type === "artist" ? "artist" : context.type === "liked" ? "liked" : "playlist"}
 <EntityDetail compact={kind === "album"} {kind} ariaLabel={heading+" Header"}>
 {#if kind !== "liked"}<button class="back-btn" on:click={navigateBack} aria-label="Go back"><svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M19 12H5M12 19l-7-7 7-7"/></svg></button>{/if}
 {#if kind === "liked"}<div class="liked-gradient-bg"><svg viewBox="0 0 24 24" width="64" height="64" fill="currentColor"><path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"/></svg></div>{:else}<div class={kind === "artist" ? "artist-avatar" : kind+"-cover"}><ControllerArtwork reference={$detailState.detail?.album.artwork} alt={heading} /></div>{/if}
 <div class={kind === "liked" ? "liked-header-info" : kind+"-info"}>
 <span class={kind === "liked" ? "liked-label" : kind+"-type"}>{kind === "liked" ? "PLAYLIST" : kind.toUpperCase()}</span>
 <h1 class={kind === "artist" ? "artist-name" : kind+"-title"}>{$detailState.detail?.album.name ?? heading}</h1>
 <div class={kind+"-meta"}>{#if $detailState.detail}<button class="album-artist link" on:click={()=> $detailState.detail?.album.artist && openArtist($detailState.detail.album.artist)} disabled={!$detailState.detail.album.artist}>{$detailState.detail.album.artist || "Unknown Artist"}</button>{#if $detailState.detail.originalYear ?? $detailState.detail.album.year}<span class="separator">•</span><span class="album-year-display">{$detailState.detail.originalYear ?? $detailState.detail.album.year}</span>{/if}<span class="separator">•</span><span>{$detailState.detail.trackCount} songs</span>{#if $detailState.detail.album.qualityBadges.length}<span class="separator">•</span><div class="album-audio-meta">{#each $detailState.detail.album.qualityBadges as badge,index}<span class="album-audio-chip" class:format-chip={index===0}>{badge}</span>{/each}</div>{/if}{:else}<span>Track count not supplied by the PC.</span>{/if}{#if albumDuration !== null}<span title={$metadataState.stale ? "Last known album duration; PC metadata could not be refreshed." : "Full album duration"}>{formatAlbumDuration(albumDuration)}</span>{:else}<span>Total duration not supplied by the PC.</span>{/if}</div>
 {#if kind !== "liked"}<div class={kind+"-actions"}>{#if kind === "album"}<div class="btn-primary album-play-split"><button class="play-all-btn play-main" disabled={!canExecute($controllerState,playIntent())} on:click={()=>play()}><svg viewBox="0 0 24 24" fill="currentColor" width="24" height="24"><path d="M8 5v14l11-7z"/></svg>Play</button><details class="play-options"><summary aria-label="Playback options" title="Playback options"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="20" height="20" aria-hidden="true"><path d="m6 9 6 6 6-6"/></svg></summary><div class="play-options-panel"><button disabled={!canExecute($controllerState,"queue_entity")} on:click={()=>enqueueEntity("next")}>Play Next</button><button disabled={!canExecute($controllerState,"queue_entity")} on:click={()=>enqueueEntity("after_user_queue")}>Add to Queue</button><button disabled={!canExecute($controllerState,"queue_entity")} on:click={()=>enqueueEntity("end")}>Add to End</button><p>Metadata, artwork editing, pinning and file operations are managed on the PC.</p>{#if !canExecute($controllerState,playIntent()) || !canExecute($controllerState,"queue_entity")}<p>Playback and queue actions require permission and support from the PC.</p>{/if}</div></details></div>{:else}<button class="btn-primary play-all-btn" disabled={!canExecute($controllerState,playIntent())} on:click={()=>play()}><svg viewBox="0 0 24 24" fill="currentColor" width="24" height="24"><path d="M8 5v14l11-7z"/></svg>Play</button>{/if}{#if kind === "album"}<details class="album-info-disclosure"><summary class="btn-info" aria-label="Album information"><svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="12" y1="16" x2="12" y2="12"/><line x1="12" y1="8" x2="12.01" y2="8"/></svg></summary><div class="album-info-content"><p>Album like changes are managed on the PC.</p><p>Release metadata is not supplied by the PC.</p>{#if albumDuration === null}<p>Total duration not supplied by the PC.</p>{/if}<p>Liked-track total is not supplied by the PC; the filter requests authoritative membership.</p></div></details><button class="btn-like-album" class:liked={$detailState.detail?.liked===true} disabled aria-label={$detailState.detail?.liked ? "Liked album; changes managed on the PC" : "Album like changes managed on the PC"}><svg viewBox="0 0 24 24" width="22" height="22" fill={$detailState.detail?.liked ? "currentColor" : "none"} stroke="currentColor" stroke-width="2"><path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"/></svg></button><button class="btn-filter-liked" class:active={likedOnly} aria-pressed={likedOnly} aria-label={likedOnly ? "Show all tracks" : "Show only liked tracks"} on:click={()=>likedOnly=!likedOnly}><svg viewBox="0 0 24 24" width="18" height="18" fill="currentColor"><path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"/></svg></button>{:else}<button class="shuffle-btn" disabled={!canExecute($controllerState,"set_shuffle") || !canExecute($controllerState,playIntent())} on:click={shufflePlay}>Shuffle</button>{/if}{#if kind !== "album"}<details class="detail-actions-menu"><summary>More actions</summary><button disabled={!canExecute($controllerState,"queue_entity")} on:click={()=>enqueueEntity("next")}>Play Next</button><button disabled={!canExecute($controllerState,"queue_entity")} on:click={()=>enqueueEntity("after_user_queue")}>Add to Queue</button><button disabled={!canExecute($controllerState,"queue_entity")} on:click={()=>enqueueEntity("end")}>Add to End</button>{#if context?.type==="album"}<label><input type="checkbox" bind:checked={likedOnly} />Liked tracks only</label>{/if}<p>Metadata, artwork editing, pinning and file operations are managed on the PC.</p>{#if !canExecute($controllerState,playIntent()) || !canExecute($controllerState,"queue_entity")}<p>Playback and queue actions require permission and support from the PC.</p>{/if}</details>{/if}</div>{/if}
 </div>
 <svelte:fragment slot="controls">{#if kind === "liked"}<div class="liked-controls"><div class="controls-wrapper"><button class="play-all-btn" aria-label="Play All" disabled={!canExecute($controllerState,playIntent())} on:click={()=>play()}><div class="btn-icon"><svg viewBox="0 0 24 24" fill="currentColor" width="24" height="24"><path d="M8 5v14l11-7z"/></svg></div><span>Play All</span></button><button class="shuffle-btn" aria-label="Shuffle Play" disabled={!canExecute($controllerState,"set_shuffle") || !canExecute($controllerState,playIntent())} on:click={shufflePlay}><svg viewBox="0 0 24 24" fill="currentColor" width="20" height="20"><path d="M10.59 9.17L5.41 4L4 5.41l5.17 5.17l1.42-1.41zM14.5 4l2.04 2.04L4 18.59L5.41 20L17.96 7.45L20 9.5V4h-5.5zm.33 9.41l-1.41 1.41l3.13 3.13L14.5 20H20v-5.5l-2.04 2.04l-3.13-3.13z"/></svg><span>Shuffle</span></button></div>{#if !canExecute($controllerState,playIntent())}<p>Playback requires permission and support from the PC.</p>{/if}</div>{/if}</svelte:fragment></EntityDetail>
 {/if}
 {#if query.type==="artist_albums"}
 <VirtualizedGrid items={albums} {layout} bind:currentScrollTop initialScrollTop={currentScrollTop} onItemClick={albumClick} onLoadMore={loadMore} hasMore={browsePresentation.hasMore} cardWidthDesktop={240} cardWidthMobile={170} cardHeightDesktop={layout==="list"?104:380} cardHeightMobile={layout==="list"?104:305} gridGapDesktop={layout==="list"?8:24} gridGapMobile={8} let:item={album}>
 {@const card = toAlbumCard(album)}
 <EntityGrid albumMetadata={true} {layout} primaryText={card.primaryText} secondaryText={card.secondaryText} ariaLabel={album.name} playAvailability={availability("play_album")} actions={albumActions(album.id,album.artist)} onPlay={()=>playAlbumCard(album.id)}><svelte:fragment slot="cover"><ControllerArtwork reference={album.artwork} alt={album.name}/></svelte:fragment><svelte:fragment slot="extra-info"><div class="audio-chips">{#each card.qualityBadges as badge}<span class="audio-chip">{badge}</span>{/each}</div></svelte:fragment></EntityGrid>
 </VirtualizedGrid>
 {:else if query.type==="artists"}
 <VirtualizedGrid items={artists} getItemKey={(artist)=>artist.name} onItemClick={(artist)=>openArtist(artist.name)} onLoadMore={loadMore} hasMore={browsePresentation.hasMore} cardWidthDesktop={200} cardWidthMobile={140} cardHeightDesktop={240} cardHeightMobile={190} let:item={artist}>
 <EntityGrid primaryText={artist.name} secondaryText={artist.albumCount+" albums • "+artist.trackCount+" songs"} variant="round" coverBackground="linear-gradient(135deg, var(--accent-primary) 0%, #1a1a1a 100%)" ariaLabel={artist.name} playTooltip="Play artist" playAvailability={availability("play_artist")} onPlay={()=>dispatch({type:"play_artist",artistName:artist.name})} actions={[{id:"pin",label:"Pin to Top",availability:{enabled:false,reason:"Artist pin state is not supplied by the PC."}}]}><svelte:fragment slot="cover"><ControllerArtwork reference={artist.artwork} alt={artist.name}/></svelte:fragment></EntityGrid>
 </VirtualizedGrid>
 {:else if query.type==="playlists"}
 <EntityGrid page={true}><header class="view-header"><h1>Playlists</h1><div class="header-actions"><button class="btn-secondary" disabled><svg viewBox="0 0 24 24" fill="currentColor" width="20" height="20" aria-hidden="true"><path d="M10 4H4c-1.1 0-2 .9-2 2v12c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V8c0-1.1-.9-2-2-2h-8l-2-2z"/></svg>Import Folder</button><button class="btn-secondary" disabled><svg viewBox="0 0 24 24" fill="currentColor" width="20" height="20" aria-hidden="true"><path d="M19 13h-6v6h-2v-6H5v-2h6V5h2v6h6v2z"/></svg>New Playlist</button></div></header><p>Playlist creation and folder import are managed on the PC.</p>
 <VirtualizedGrid items={playlists} onItemClick={(playlist)=>openPlaylist(playlist.id,playlist.name)} onLoadMore={loadMore} hasMore={browsePresentation.hasMore} let:item={playlist}>
 <EntityGrid primaryText={playlist.name} secondaryText={playlist.trackCount+" songs"} ariaLabel={playlist.name} playTooltip="Play playlist" playAvailability={availability("play_playlist")} onPlay={()=>dispatch({type:"play_playlist",playlistId:playlist.id})} actions={[{id:"rename",label:"Rename Playlist",availability:{enabled:false,reason:"Playlist editing is managed on the PC."}}]}><svelte:fragment slot="cover"><ControllerArtwork reference={playlist.artwork} alt={playlist.name}/></svelte:fragment></EntityGrid>
 </VirtualizedGrid>
 </EntityGrid>
 {:else if query.type==="search"}<div class="entity-list">{#each matches as match}{#if match.type==="album"}<button on:click={()=>openAlbum(match.album.id)}>{match.album.name}<small>Album · {match.album.artist}</small></button>{:else if match.type==="artist"}<button on:click={()=>openArtist(match.artist.name)}>{match.artist.name}<small>Artist</small></button>{:else}<button disabled={!canExecute($controllerState,"play_track")} on:click={()=>dispatch({type:"play_track",trackId:match.track.id})}>{trackTitle(match.track)}<small>{match.track.artist}</small></button>{/if}{/each}</div>
 {:else}
 <TrackRows playCounts={$metadataState.counts} countsStale={$metadataState.stale} disableVirtualScroll={context?.type === "album"} {tracks} showAlbum={context?.type!=="album" && context?.type!=="playlist"} mobileViewMode={context?.type==="album"?"album":context?.type==="playlist"?"playlist":"library"} playAvailability={availability(playIntent())} queueAvailability={(placement)=>availability(placement==="end"?"queue_append":"queue_insert")} onPlay={play} onQueue={queueTrack} onArtist={openArtist} onAlbum={openAlbum} playingTrackId={$controllerState.snapshot?.playback.track?.id ?? null} playing={$controllerState.snapshot?.playback.status==="playing"}>
 <svelte:fragment slot="artwork" let:track><ControllerArtwork reference={track.artwork} /></svelte:fragment>
 </TrackRows>{#if $metadataState.error}<p role="status">{$metadataState.error}</p>{/if}
 {/if}
 {#if $state.error}<p role="alert">{$state.error}</p><button on:click={page.refresh}>Refresh</button>{/if}
 {#if $state.loading}<p role="status">Loading from PC…</p>{:else if !$state.items.length && !$state.error}<p>No items in this view.</p>{/if}
 {#if $state.hasEarlier}<p>Showing the latest 1,000 loaded entries.</p><button on:click={page.refresh}>Back to first page</button>{/if}
 {#if $state.nextCursor}<button class="load-more" on:click={page.more} disabled={$state.loading}>Load more</button>{/if}
 {/if}
</section>
<style>
 .album-info-disclosure { position: relative; }
 .album-info-disclosure > summary { list-style: none; cursor: pointer; min-width: 44px; min-height: 44px; box-sizing: border-box; }
 .album-info-disclosure > summary::-webkit-details-marker { display: none; }
 .album-info-content { position: absolute; top: calc(100% + 8px); left: 0; width: min(280px, 70vw); max-height: 40dvh; overflow: auto; padding: var(--spacing-md); border: 1px solid var(--border-color); border-radius: var(--radius-md); background: var(--bg-elevated); color: var(--text-secondary); z-index: 20; }
 .album-info-content p { margin: 0 0 var(--spacing-sm); }
 .album-info-disclosure > summary:focus-visible { outline: 2px solid var(--accent-primary); }
 .browse-view { height: 100%; min-width: 0; min-height: 0; display: flex; flex-direction: column; overflow: hidden; }
 .browse-view > :global(.virtualized-grid-container) { flex: 1; min-height: 0; }
 .browse-view > p { color: var(--text-secondary); font-size: .8rem; padding: 8px var(--spacing-md); margin: 0; }
 .browse-view > button { min-height: 44px; color: var(--text-primary); background: var(--bg-highlight); border: 1px solid var(--border-subtle); }
 .entity-list { overflow: auto; min-height: 0; }
 .entity-list button { min-height: 44px; text-align: left; color: var(--text-primary); background: var(--bg-base); border: 0; padding: var(--spacing-md); }
 .entity-list small { display: block; color: var(--text-secondary); }
 .album-grid-body { overflow: auto; min-height: 0; }
 .browse-view.album-detail-scroll { display: block; overflow-y: auto; overscroll-behavior-y: contain; }
 .album-play-split { display: inline-flex; align-items: stretch; position: relative; gap: 0; padding: 0; }
 .album-play-split .play-main { background: transparent; color: inherit; display: inline-flex; align-items: center; justify-content: center; gap: var(--spacing-sm); font-weight: 600; padding: var(--spacing-sm) var(--spacing-lg); min-height: 44px; border-radius: var(--radius-full) 0 0 var(--radius-full); }
 :global(.album-header:has(.play-options[open])) { z-index: 30; }
 .album-play-split:has(.play-options[open]) { z-index: 30; }
 .play-options { position: static; }
 .play-options > summary { list-style: none; display: flex; align-items: center; justify-content: center; width: 44px; height: 100%; min-height: 44px; cursor: pointer; border-radius: 0 var(--radius-full) var(--radius-full) 0; }
 .play-options > summary::-webkit-details-marker { display: none; }
 .play-options > summary:focus-visible { outline: 2px solid var(--text-primary); outline-offset: 2px; }
 .play-options-panel { position: absolute; left: 0; top: calc(100% + 8px); width: min(300px, 75vw); max-height: 40dvh; overflow: auto; z-index: 20; padding: 8px; border: 1px solid var(--border-color); border-radius: var(--radius-md); background: var(--bg-elevated); color: var(--text-primary); box-shadow: var(--shadow-lg); }
 .play-options-panel button { display: block; width: 100%; min-height: 44px; text-align: left; padding: 8px 12px; border-radius: var(--radius-sm); }
 .play-options-panel button:hover:not(:disabled) { background: var(--bg-highlight); }
 .play-options-panel button:disabled { opacity: .5; cursor: not-allowed; }
 .play-options-panel p { font-size: .75rem; color: var(--text-secondary); padding: 8px 12px; }
</style>
