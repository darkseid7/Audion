<script lang="ts">
 import { tick } from "svelte";
 import { controllerOverlay } from "$lib/application/controller-ui";
 import type { DisplayTrack } from "$lib/application/types";
 import type { ActionAvailability } from "$lib/application/presentation/types";
 export let tracks: readonly DisplayTrack[] = [];
 export let playCounts: ReadonlyMap<number, number | null> = new Map();
 export let countsStale = false;
 function playCount(counts:ReadonlyMap<number, number | null>, id:number) { const count = Number.isSafeInteger(id) && id>0 ? counts.get(id) : null; return typeof count === "number" && Number.isSafeInteger(count) && count>=0 ? count : null; }
 export let showAlbum = true;
 export let disableVirtualScroll = false;
 export let mobileViewMode: "album" | "playlist" | "library" = "library";
 export let playingTrackId: number | null = null;
 export let playing = false;
 export let playAvailability: ActionAvailability = {enabled:false,reason:"Playback is not available."};
 export let queueAvailability: (placement:"next"|"after_user_queue"|"end")=>ActionAvailability = ()=>({enabled:false,reason:"Queue changes are not available."});
 export let onPlay: (id:number)=>void = ()=>{};
 export let onQueue: (id:number,placement:"next"|"after_user_queue"|"end")=>void = ()=>{};
 export let onArtist: ((name:string)=>void) | undefined = undefined;
 export let onAlbum: ((id:number)=>void) | undefined = undefined;
 export let selectedTrack: DisplayTrack | null = null;
 let actionsDialog: HTMLDialogElement;
 const queueActions = [
   { placement: "next", label: "Play Next", icon: "M5 4v16l10-8zM19 4v16" },
   { placement: "after_user_queue", label: "Add to Queue", icon: "M4 6h12M4 11h12M4 16h6M18 14v8M14 18h8" },
   { placement: "end", label: "Add to End", icon: "M4 5h16M4 10h16M4 15h8M17 15v7M14 19l3 3 3-3" }
 ] as const;
 $: unavailableReasons = [...new Set(queueActions.map(action => queueAvailability(action.placement)).filter(state => !state.enabled).map(state => state.reason).filter(Boolean))];
 function dismissBackdrop(event: MouseEvent) {
   if (event.target !== actionsDialog) return;
   const bounds = actionsDialog.getBoundingClientRect();
   if (event.clientX < bounds.left || event.clientX > bounds.right || event.clientY < bounds.top || event.clientY > bounds.bottom) closeActions();
 }
 function queueSelection(id: number, placement: "next" | "after_user_queue" | "end") {
   if (!queueAvailability(placement).enabled) return;
   queue(id, placement);
   closeActions();
 }
 async function showActions(track: DisplayTrack) { selectedTrack = track; await tick(); actionsDialog?.showModal(); }
 function closeActions() { actionsDialog?.close(); selectedTrack = null; }
 function play(id:number) { if(playAvailability.enabled)onPlay(id); }
 function queue(id:number,placement:"next"|"after_user_queue"|"end") { if(queueAvailability(placement).enabled)onQueue(id,placement); }
 function duration(value:number|null) { if(value===null)return "—";const seconds=Math.max(0,Math.floor(value));return `${Math.floor(seconds/60)}:${String(seconds%60).padStart(2,"0")}`; }
</script>
<div class="track-list" class:no-scroll={disableVirtualScroll}>
 <slot name="header"><header class="list-header" class:no-album={!showAlbum}><span class="col-header col-num">#</span><span class="col-header col-cover" aria-hidden="true"></span><span class="col-header col-artist">Title</span>{#if showAlbum}<span class="col-header col-album">Album</span>{/if}<span class="col-header col-duration">Duration</span><span class="col-header col-like">Liked</span><span class="col-header col-plays">Plays</span></header></slot>
 {#if $$slots.default}<slot />{:else}
 <div class="list-body" class:no-scroll={disableVirtualScroll} class:no-album={!showAlbum} class:mobile-album={mobileViewMode==="album"} class:mobile-playlist={mobileViewMode==="playlist"} class:mobile-library={mobileViewMode==="library"}>
 {#each tracks as track,index (index)}
 <div class="track-row" class:playing={playingTrackId===track.id} on:contextmenu={(event)=>{event.preventDefault();void showActions(track);}}>
 <span class="col-num">{#if playingTrackId===track.id && playing}<svg class="playing-icon" viewBox="0 0 24 24" fill="currentColor" width="18" height="18"><path d="M12 3v10.55c-.59-.34-1.27-.55-2-.55-2.21 0-4 1.79-4 4s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z"/></svg>{:else}<span class="track-index">{index+1}</span><button class="hover-play" on:click={()=>play(track.id)} disabled={!playAvailability.enabled} aria-label="Play">&#9654;</button>{/if}</span>
 <span class="col-cover"><div class="cover-wrapper"><slot name="artwork" {track} /></div></span>
 <div class="col-title"><div class="title-row"><button class="track-name truncate" class:album-title-target={mobileViewMode === "album"} on:click={()=>play(track.id)} disabled={!playAvailability.enabled}><span class="truncate">{track.title || "Unknown Title"}</span>{#if mobileViewMode === "album"}<span class="track-quality-row">{#each track.quality.badges as badge}<span class="quality-tag">{badge}</span>{/each}</span>{/if}</button><button class="row-menu-button" aria-label={`Actions for ${track.title || "Unknown Title"}`} on:click={()=>showActions(track)}>⋮</button></div>{#if mobileViewMode !== "album"}<div class="artist-with-format"><button class="track-artist truncate" on:click={()=>track.artist && onArtist?.(track.artist)} disabled={!track.artist || !onArtist}>{track.artist || "Unknown Artist"}</button><div class="track-quality-row">{#each track.quality.badges as badge}<span class="quality-tag">{badge}</span>{/each}</div></div>{/if}</div>
 {#if showAlbum}<button class="col-album-cell truncate" disabled={track.albumId===null || !onAlbum} on:click={()=>track.albumId!==null && onAlbum?.(track.albumId)}>{track.album || "—"}</button>{/if}
 <span class="col-duration">{duration(track.duration)}</span><span class="col-like" class:liked={track.liked===true} role="img" aria-label={track.liked===undefined ? "Like state not supplied by the PC" : track.liked ? "Liked; changes managed on the PC" : "Not liked; changes managed on the PC"}>{#if track.liked===undefined}—{:else}<svg viewBox="0 0 24 24" width="16" height="16" fill={track.liked ? "currentColor" : "none"} stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"/></svg>{/if}</span><span class="col-plays" title={playCount(playCounts, track.id) === null ? "Not supplied by the PC" : countsStale ? "Last known play count; PC metadata could not be refreshed." : "Play count"} aria-label={playCount(playCounts, track.id) === null ? "Play count not supplied by the PC" : `${countsStale ? "Last known play count" : "Play count"}: ${playCount(playCounts, track.id)}`}>{playCount(playCounts, track.id) ?? "—"}</span>
 </div>

 {/each}
 <p class="projection-gap">{#if tracks.some(track=>playCount(playCounts, track.id) === null)}Play counts: Not supplied by the PC. {/if}Track sorting is managed by the PC.</p>
 </div>
 {/if}
</div>
 {#if selectedTrack}{@const actionTrack = selectedTrack}
 <dialog class="track-actions-dialog" bind:this={actionsDialog} use:controllerOverlay={{ close: closeActions }} aria-label={`Actions for ${actionTrack.title || "Unknown Title"}`} on:close={()=>selectedTrack=null} on:cancel={closeActions} on:click={dismissBackdrop}>
   <header class="track-actions-heading">
     <div class="track-actions-symbol" aria-hidden="true"><svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="2"><path d="M9 18V5l11-2v13M9 8l11-2"/><ellipse cx="6" cy="18" rx="3" ry="2"/><ellipse cx="17" cy="16" rx="3" ry="2"/></svg></div>
     <div class="track-actions-context"><h2>{actionTrack.title || "Unknown Title"}</h2><p>{actionTrack.artist || "Unknown Artist"}</p></div>
     <button class="track-actions-close" aria-label="Close track actions" on:click={closeActions}><svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18"/></svg></button>
   </header>
   <div class="track-actions-list">
     {#each queueActions as action}
       <button class="track-action-item" disabled={!queueAvailability(action.placement).enabled} on:click={()=>queueSelection(actionTrack.id,action.placement)}><svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d={action.icon}/></svg><span>{action.label}</span></button>
     {/each}
   </div>
   <footer class="track-actions-notes">
     {#if unavailableReasons.length}<details><summary>Unavailable actions</summary>{#each unavailableReasons as reason}<p>{reason}</p>{/each}</details>{/if}
     <p>Metadata, file operations and like changes are managed on the PC.</p>
   </footer>
 </dialog>
 {/if}
<style>
 .track-actions-dialog { position: fixed; inset: 0; margin: auto; width: min(420px, calc(100vw - 32px)); max-height: min(70dvh, 560px); overflow: auto; padding: 0; color: var(--text-primary); background: var(--bg-elevated); border: 1px solid var(--border-color); border-radius: var(--radius-lg); box-shadow: var(--shadow-lg); }
 .track-actions-heading { display: flex; align-items: center; gap: 12px; padding: 16px; border-bottom: 1px solid var(--border-subtle); }
 .track-actions-symbol { flex-shrink: 0; width: 40px; height: 40px; display: flex; align-items: center; justify-content: center; border-radius: var(--radius-sm); color: var(--accent-primary); background: var(--accent-subtle); }
 .track-actions-context { flex: 1; min-width: 0; }
 .track-actions-context h2 { margin: 0; font-size: 1rem; font-weight: 600; line-height: 1.4; overflow-wrap: anywhere; }
 .track-actions-context p { margin: 2px 0 0; font-size: .8125rem; color: var(--text-secondary); }
 .track-actions-dialog .track-actions-close { flex-shrink: 0; width: 44px; height: 44px; display: flex; align-items: center; justify-content: center; border: 0; padding: 0; border-radius: var(--radius-full); background: transparent; color: var(--text-secondary); }
 .track-actions-list { padding: 8px; }
 .track-actions-dialog .track-action-item { display: flex; align-items: center; gap: 16px; width: 100%; min-height: 48px; padding: 12px 16px; border: 0; border-radius: var(--radius-sm); background: transparent; text-align: left; font-size: .9375rem; font-weight: 500; }
 .track-action-item svg { flex-shrink: 0; color: var(--accent-primary); }
 .track-actions-dialog .track-action-item:hover:not(:disabled), .track-actions-dialog .track-actions-close:hover { background: var(--bg-highlight); }
 .track-actions-dialog .track-action-item:disabled { opacity: .45; cursor: not-allowed; }
 .track-actions-notes { padding: 12px 16px 16px; border-top: 1px solid var(--border-subtle); font-size: .75rem; line-height: 1.5; color: var(--text-subdued); }
 .track-actions-notes p { margin: 8px 0 0; }
 .track-actions-notes summary { min-height: 44px; display: flex; align-items: center; cursor: pointer; color: var(--text-secondary); }
 .track-actions-notes summary::before { content: "+"; margin-right: 8px; color: var(--accent-primary); }
 .track-actions-notes details[open] summary::before { content: "−"; }
 @media (max-width: 600px) {
   .track-actions-dialog { inset: auto 0 0; margin: 0 auto; width: 100%; max-height: 75dvh; border-radius: var(--radius-lg) var(--radius-lg) 0 0; }
   .track-actions-notes { padding-bottom: max(16px, env(safe-area-inset-bottom, 0px)); }
 }
 .track-actions-dialog::backdrop { background: rgba(0,0,0,.65); }
 .col-title { position: relative; padding-right: 44px; }
 button.track-name { background: none; border: 0; padding: 0; text-align: left; font-family: inherit; cursor: pointer; }
 .row-menu-button { position: absolute; right: 0; top: 50%; transform: translateY(-50%); min-width: 44px; min-height: 44px; color: var(--text-secondary); background: none; border: 0; }

 .track-actions-dialog button { min-height: 44px; cursor: pointer; }
 .track-actions-dialog button { color: var(--text-primary); background: var(--bg-highlight); border: 1px solid var(--border-subtle); padding: 8px; }
 .projection-gap { color: var(--text-secondary); font-size: .75rem; padding: var(--spacing-md); }
 summary:focus-visible,button:focus-visible { outline: 2px solid var(--accent-primary); }
 :global {
.track-list {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }
.track-list.no-scroll {
    height: auto;
    overflow: visible;
  }
.track-list .list-header {
    display: grid;
    grid-template-columns: 40px 56px 1fr 1fr 80px 36px 100px;
    gap: var(--spacing-md);
    padding: var(--spacing-sm) var(--spacing-md);
    padding-right: calc(var(--spacing-md) + var(--scrollbar-width, 0px));
    padding-left: var(--spacing-lg);
    margin-bottom: 20px;
    border-bottom: 1px solid var(--border-color);
    font-size: 0.78rem;
    font-weight: 500;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    line-height: 1.1;
    color: var(--text-subdued);
    background-color: var(--bg-base);
    z-index: 10;
    flex-shrink: 0;
  }
.track-list .list-header.with-drag {
    grid-template-columns: 32px 40px 56px 1fr 1fr 80px 36px 100px;
  }
.track-list .list-header.no-album {
    grid-template-columns: 40px 56px 1fr 80px 36px 100px;
  }
.track-list .list-header.no-album.with-drag {
    grid-template-columns: 32px 40px 56px 1fr 80px 36px 100px;
  }
.track-list .col-header {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    color: inherit;
    text-transform: inherit;
    letter-spacing: inherit;
    cursor: default;
    display: flex;
    align-items: center;
    gap: 4px;
    transition: color var(--transition-fast);
    user-select: none;
    justify-self: stretch;
    width: 100%;
    font-size: inherit;
    font-weight: inherit;
    line-height: inherit;
  }
.track-list .col-header.sortable {
    cursor: pointer;
  }
.track-list .col-header.sortable:hover {
    color: var(--text-primary);
  }
.track-list .col-header.col-drag {
    cursor: default;
  }
.track-list .col-header.col-num {
    justify-content: center;
  }
.track-list .col-header.col-artist {
    justify-content: flex-start;
  }
.track-list .col-header.col-album {
    justify-content: flex-start;
  }
.track-list .col-header.col-duration {
    justify-content: center;
  }
.track-list .col-header.col-like {
    justify-content: center;
    color: var(--text-subdued);
  }
.track-list .col-header.col-plays {
    justify-content: center;
  }
.track-list .sort-icon {
    color: var(--accent-primary);
    font-size: 0.75rem;
  }
.track-list .list-body {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    position: relative;
    overscroll-behavior-y: contain;
  }
.track-list .list-body.no-scroll {
    overflow-y: visible;
    overflow-x: visible;
    flex: none;
    overscroll-behavior-y: auto;
  }
.track-list .virtual-spacer {
    position: relative;
    width: 100%;
  }
.track-list .virtual-content {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    will-change: transform;
  }
.track-list .track-row {
    display: grid;
    grid-template-columns: 40px 56px 1fr 1fr 80px 36px 100px;
    gap: var(--spacing-md);
    padding: 6px var(--spacing-md);
    padding-left: var(--spacing-lg);
    align-items: center;
    border-radius: var(--radius-md);
    transition: background-color var(--transition-fast);
    width: 100%;
    text-align: left;
    height: 58px; /* Fixed height for virtual scrolling */
    box-sizing: border-box;
    overflow: hidden;
  }
.track-list .list-body.with-drag .track-row {
    grid-template-columns: 32px 40px 56px 1fr 1fr 80px 36px 100px;
  }
.track-list .list-body.no-album .track-row {
    grid-template-columns: 40px 56px 1fr 80px 36px 100px;
  }
.track-list .list-body.no-album.with-drag .track-row {
    grid-template-columns: 32px 40px 56px 1fr 80px 36px 100px;
  }
.track-list .list-body.multiselect .track-row {
    grid-template-columns: 40px 40px 56px 1fr 1fr 80px 36px 100px;
  }
.track-list .list-body.multiselect.no-album .track-row {
    grid-template-columns: 40px 40px 56px 1fr 80px 36px 100px;
  }
.track-list .track-row.selected {
    background-color: rgba(var(--accent-primary-rgb, 29, 185, 84), 0.12);
  }
.track-list .track-row.selected:hover {
    background-color: rgba(var(--accent-primary-rgb, 29, 185, 84), 0.18);
  }
.track-list .track-row:hover {
    background-color: rgba(255, 255, 255, 0.1);
    cursor: pointer;
  }
.track-list .track-row.playing {
    background-color: var(--bg-surface);
  }
.track-list .track-row.playing .track-name {
    color: var(--accent-primary);
  }
.track-list .track-name {
    opacity: 0.5;
    transition: opacity 0.15s ease;
  }
.track-list .track-row.playing .track-name,
.track-list .track-row:hover .track-name,
.track-list .track-row.selected .track-name {
    opacity: 1;
  }
.track-list .track-row.dragging {
    opacity: 0.5;
    background-color: var(--bg-highlight);
  }
.track-list .track-row.drag-over {
    border-top: 2px solid var(--accent-primary);
    margin-top: -2px;
  }
.track-list .drag-handle {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    color: var(--text-subdued);
    cursor: grab;
    opacity: 0;
    transition: all var(--transition-fast);
    flex-shrink: 0;
    user-select: none;
    -webkit-user-select: none;
    touch-action: none; /* Prevent default touch behaviors */
  }
.track-list .track-row:hover .drag-handle {
    opacity: 1;
  }
.track-list .drag-handle:hover {
    color: var(--text-primary);
    background-color: rgba(255, 255, 255, 0.1);
    border-radius: var(--radius-sm);
  }
.track-list .drag-handle:active {
    cursor: grabbing;
    background-color: rgba(255, 255, 255, 0.15);
  }
.track-list .col-num {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    text-align: center;
    color: var(--text-subdued);
    font-size: 0.875rem;
  }
.track-list .track-row:hover .col-num:not(:has(.playing-icon)) {
    color: var(--text-primary);
  }
.track-list .track-index,
.track-list .hover-play {
    transition: opacity var(--transition-fast);
  }
.track-list .hover-play {
    all: unset;
    position: absolute;
    opacity: 0;
    color: var(--text-primary);
    font-size: 1.1rem;
    line-height: 1;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
  }
.track-list .track-row:hover .track-index {
    opacity: 0;
  }
.track-list .track-row:hover .hover-play {
    opacity: 1;
  }
.track-list .track-row.unavailable:hover .hover-play,
.track-list .track-row.unavailable:hover .track-index {
    opacity: 1;
  }
.track-list .col-cover {
    display: flex;
    align-items: center;
    justify-content: center;
  }
.track-list .cover-image {
    width: 40px;
    height: 40px;
    border-radius: var(--radius-sm);
    object-fit: cover;
  }
.track-list .cover-placeholder {
    width: 40px;
    height: 40px;
    border-radius: var(--radius-sm);
    background-color: var(--bg-highlight);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-subdued);
  }
.track-list .cover-wrapper {
    position: relative;
    width: 40px;
    height: 40px;
  }
.track-list .cover-play-overlay {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background-color: rgba(0, 0, 0, 0.6);
    border-radius: var(--radius-sm);
    opacity: 0;
    transition: opacity var(--transition-fast);
    color: var(--text-primary);
  }
.track-list .track-row:hover .cover-play-overlay {
    opacity: 1;
  }
.track-list .track-row.playing .cover-play-overlay {
    opacity: 0;
  }
.track-list .playing-icon {
    color: var(--accent-primary);
    animation: pulse 1.5s ease-in-out infinite;
  }
@keyframes pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.5;
    }
  }
.track-list .col-title {
    display: flex;
    flex-direction: column;
    min-width: 0;
    justify-content: center;
    gap: 1px;
    height: 100%;
    padding-top: 1.5px;
    overflow: hidden;
  }
.track-list .col-artist {
    display: flex;
    align-items: center;
    min-width: 0;
    gap: 8px;
    overflow: hidden;
  }
.track-list .artist-thumb {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    flex-shrink: 0;
  }
.track-list .cover-image-small {
    width: 40px;
    height: 40px;
    border-radius: 6px;
    object-fit: cover;
  }
.track-list .cover-placeholder-small {
    width: 40px;
    height: 40px;
    border-radius: 6px;
    background-color: var(--bg-highlight);
    color: var(--text-subdued);
    display: flex;
    align-items: center;
    justify-content: center;
  }
.track-list .artist-meta {
    display: flex;
    flex-direction: column;
    justify-content: center;
    min-width: 0;
    gap: 1px;
    overflow: hidden;
  }
.track-list .title-row {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    min-width: 0;
  }
.track-list .track-name {
    font-size: 1rem;
    font-weight: 500;
    color: var(--text-primary);
    line-height: 1.2;
    margin: 0;
  }
.track-list .quality-tag {
    font-size: 0.6rem;
    font-weight: 700;
    padding: 2px 6px;
    border-radius: var(--radius-sm);
    background-color: var(--bg-highlight);
    color: var(--text-secondary);
    border: 1px solid var(--border-color);
    white-space: nowrap;
    flex-shrink: 0;
    opacity: 0.7;
    transition: opacity var(--transition-fast);
  }
.track-list .track-row:hover .quality-tag {
    opacity: 1;
  }
.track-list .track-quality-row {
    display: flex;
    align-items: center;
    gap: 3px;
    flex-wrap: nowrap;
    min-width: 0;
    overflow: hidden;
  }
.track-list .quality-tag.high-quality {
    color: var(--accent-primary);
    border-color: var(--accent-primary);
    background-color: color-mix(in srgb, var(--accent-primary), transparent 85%);
  }
.track-list .track-artist {
    font-size: 0.8125rem;
    color: var(--text-secondary);
    background: none;
    border: none;
    padding: 0;
    margin: 0;
    text-align: left;
    max-width: fit-content;
    line-height: 1.2;
    min-height: 0;
  }
.track-list .track-artist:hover:not(:disabled) {
    color: var(--text-primary);
    text-decoration: underline;
    cursor: pointer;
  }
.track-list .media-metadata {
    font-size: 0.7rem;
    color: var(--text-subdued);
    opacity: 0.9;
  }
.track-list .col-album-cell {
    font-size: 0.875rem;
    color: var(--text-secondary);
    background: none;
    border: none;
    padding: 0;
    width: 100%;
    justify-self: stretch;
    text-align: left;
    line-height: 1.2;
  }
.track-list .col-album-cell:hover:not(:disabled) {
    color: var(--text-primary);
    text-decoration: underline;
    cursor: pointer;
  }
.track-list .col-duration {
    text-align: center;
    font-size: 0.875rem;
    color: var(--text-subdued);
    display: flex;
    align-items: center;
    justify-content: center;
  }
.track-list .col-like {
    display: flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
    color: var(--text-subdued);
    opacity: 0;
    transition: opacity var(--transition-fast), color var(--transition-fast);
  }
.track-list .col-like.liked {
    opacity: 1;
    color: var(--accent-primary);
  }
.track-list .track-row:hover .col-like {
    opacity: 1;
  }
.track-list .col-like:hover {
    color: var(--accent-primary);
  }
.track-list .col-plays {
    text-align: center;
    font-size: 0.8125rem;
    color: var(--text-subdued);
    display: flex;
    align-items: center;
    justify-content: center;
  }
.track-list .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: var(--spacing-xl);
    color: var(--text-subdued);
    text-align: center;
    gap: var(--spacing-sm);
    height: 100%;
  }
.track-list .empty-state h3 {
    font-size: 1.25rem;
    font-weight: 600;
    color: var(--text-primary);
  }
.track-list .empty-state p {
    font-size: 0.875rem;
  }
.track-list .track-row.unavailable {
    opacity: 0.5;
    cursor: not-allowed;
  }
.track-list .track-row.unavailable:hover {
    background-color: transparent;
  }
.track-list .downloaded-icon {
    color: var(--accent-primary);
    display: flex;
    align-items: center;
    margin-left: var(--spacing-xs);
    flex-shrink: 0;
  }
.track-list .truncate {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
.track-list .col-checkbox {
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
  }
.track-list .custom-checkbox {
    width: 20px;
    height: 20px;
    border: 2px solid var(--border-color);
    border-radius: var(--radius-sm);
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all var(--transition-fast);
    background-color: transparent;
    position: relative;
  }
.track-list .custom-checkbox:hover {
    border-color: var(--accent-primary);
    background-color: rgba(var(--accent-primary-rgb, 29, 185, 84), 0.1);
  }
.track-list .custom-checkbox.checked {
    background-color: var(--accent-primary);
    border-color: var(--accent-primary);
  }
.track-list .custom-checkbox svg {
    color: var(--bg-base);
  }
.track-list .col-header.col-checkbox {
    display: flex;
    align-items: center;
    justify-content: center;
  }
.track-list .list-header.multiselect {
    grid-template-columns: 40px 40px 56px 1fr 1fr 80px 36px 100px;
  }
.track-list .list-header.multiselect.no-album {
    grid-template-columns: 40px 40px 56px 1fr 80px 36px 100px;
  }
.track-list .equalizer-bars {
    display: flex;
    align-items: flex-end;
    justify-content: center;
    gap: 2px;
    height: 16px;
    width: 16px;
  }
.track-list .eq-bar {
    width: 3px;
    background-color: var(--accent-primary);
    border-radius: 1px;
    animation: eq-bounce 1.2s ease-in-out infinite;
  }
.track-list .eq-bar:nth-child(1) { height: 60%;  animation-delay: 0s;   }
.track-list .eq-bar:nth-child(2) { height: 100%; animation-delay: 0.2s; }
.track-list .eq-bar:nth-child(3) { height: 40%;  animation-delay: 0.4s; }
.track-list .eq-bar:nth-child(4) { height: 80%;  animation-delay: 0.6s; }
@keyframes eq-bounce {
    0%, 100% { height: 20%; }
    50%      { height: 100%; }
  }
@media (min-width: 769px) {.track-list .playing-icon {
      display: none;
    }}
@media (max-width: 768px) {.track-list .list-header {
      display: none;
    }
.track-list .quality-tag {
      display: none;
    }
.track-list .cover-play-overlay {
      display: none;
    }
.track-list .drag-handle {
      opacity: 1;
    }
.track-list .track-row {
      gap: var(--spacing-sm);
      padding: var(--spacing-xs) var(--spacing-sm);
      height: 60px;
      min-height: 60px;
    }
.track-list .list-body.mobile-album .track-row {
      grid-template-columns: 32px 1fr 48px;
      padding-left: var(--spacing-sm);
    }
.track-list .list-body.mobile-album .col-num {
      display: flex;
      align-items: center;
      justify-content: center;
      font-size: 0.9375rem;
      color: var(--text-subdued);
    }
.track-list .list-body.mobile-album .track-row.playing .col-num {
      color: var(--accent-primary);
    }
.track-list .list-body.mobile-album .col-cover {
      display: none;
    }
.track-list .list-body.mobile-album .col-album-cell {
      display: none;
    }
.track-list .list-body.mobile-album .playing-icon {
      display: none;
    }
.track-list .list-body:not(.mobile-album) .equalizer-bars {
      display: none;
    }
.track-list .list-body.mobile-album .track-name {
      font-size: 0.9375rem;
      font-weight: 600;
      color: var(--text-primary);
    }
.track-list .list-body.mobile-album .track-artist {
      font-size: 0.75rem;
      color: var(--text-secondary);
    }
.track-list .list-body.mobile-album .col-duration {
      font-size: 0.75rem;
      color: var(--text-subdued);
    }
.track-list .list-body.mobile-album.with-drag .track-row {
      grid-template-columns: 28px 32px 1fr 48px;
    }
.track-list .list-body.mobile-album.multiselect .track-row {
      grid-template-columns: 36px 32px 1fr 48px;
    }
.track-list .list-body.mobile-playlist .track-row {
      grid-template-columns: 48px 1fr 48px;
      padding-left: var(--spacing-sm);
    }
.track-list .list-body.mobile-playlist .col-num {
      display: none;
    }
.track-list .list-body.mobile-playlist .col-album-cell {
      display: none;
    }
.track-list .list-body.mobile-playlist .cover-wrapper,
.track-list .list-body.mobile-playlist .cover-image,
.track-list .list-body.mobile-playlist .cover-placeholder {
      width: 48px;
      height: 48px;
      border-radius: var(--radius-sm);
    }
.track-list .list-body.mobile-playlist .col-cover {
      justify-content: flex-start;
      align-items: center;
    }
.track-list .list-body.mobile-playlist .col-title {
      padding-top: 0;
      justify-content: center;
    }
.track-list .list-body.mobile-playlist .track-name {
      font-size: 0.9375rem;
      font-weight: 600;
      color: var(--text-primary);
    }
.track-list .list-body.mobile-playlist .track-artist {
      font-size: 0.75rem;
      color: var(--text-secondary);
      margin-top: 0;
    }
.track-list .list-body.mobile-playlist .col-duration {
      font-size: 0.75rem;
      color: var(--text-subdued);
    }
.track-list .list-body.mobile-playlist.with-drag .track-row {
      grid-template-columns: 28px 48px 1fr 48px;
    }
.track-list .list-body.mobile-playlist.multiselect .track-row {
      grid-template-columns: 36px 48px 1fr 48px;
    }
.track-list .list-body.mobile-library .track-row {
      grid-template-columns: 48px 1fr 48px;
      padding-left: var(--spacing-sm);
    }
.track-list .list-body.mobile-library .col-num {
      display: none;
    }
.track-list .list-body.mobile-library .col-album-cell {
      display: none;
    }
.track-list .list-body.mobile-library .cover-wrapper,
.track-list .list-body.mobile-library .cover-image,
.track-list .list-body.mobile-library .cover-placeholder {
      width: 48px;
      height: 48px;
      border-radius: var(--radius-sm);
    }
.track-list .list-body.mobile-library .col-cover {
      justify-content: flex-start;
      align-items: center;
    }
.track-list .list-body.mobile-library .col-title {
      padding-top: 0;
      justify-content: center;
    }
.track-list .list-body.mobile-library .track-name {
      font-size: 0.9375rem;
      font-weight: 600;
      color: var(--text-primary);
    }
.track-list .list-body.mobile-library .track-artist {
      font-size: 0.75rem;
      color: var(--text-secondary);
      margin-top: 2px;
    }
.track-list .list-body.mobile-library .col-duration {
      font-size: 0.75rem;
      color: var(--text-subdued);
    }
.track-list .list-body.mobile-library.with-drag .track-row {
      grid-template-columns: 28px 48px 1fr 48px;
    }
.track-list .list-body.mobile-library.multiselect .track-row,
.track-list .list-body.mobile-library.multiselect.no-album .track-row {
      grid-template-columns: 36px 48px 1fr 48px;
    }
.track-list .track-row.playing .track-name {
      color: var(--accent-primary);
    }
.track-list .track-row.playing .col-num {
      color: var(--accent-primary);
    }
.track-list .downloaded-icon {
      margin-left: 2px;
    }
.track-list .downloaded-icon svg {
      width: 12px;
      height: 12px;
    }
.track-list .track-row {
      position: relative;
      will-change: transform;
    }
.track-list .track-row::before {
      content: "";
      position: absolute;
      inset: 0;
      border-radius: var(--radius-md);
      background-color: transparent;
      transition: background-color 0.15s ease;
      z-index: -1;
      pointer-events: none;
    }
.track-list .track-row.swipe-queue-ready::before {
      background-color: color-mix(in srgb, var(--accent-primary), transparent 80%);
    }
.track-list .track-row.swipe-queue-added::before {
      background-color: color-mix(in srgb, var(--accent-primary), transparent 65%);
    }
.track-list .track-row::after {
      content: "+";
      position: absolute;
      left: 8px;
      top: 50%;
      transform: translateY(-50%);
      font-size: 1.25rem;
      font-weight: 700;
      color: var(--accent-primary);
      opacity: 0;
      transition: opacity 0.15s ease;
      pointer-events: none;
      z-index: -1;
    }
.track-list .track-row.swipe-queue-ready::after {
      opacity: 1;
    }
.track-list .track-row.swipe-queue-added::after {
      content: "âœ“";
      opacity: 1;
    }}
 }
 .track-list .album-title-target { display: flex; flex-direction: column; align-items: flex-start; justify-content: center; gap: 2px; min-height: 44px; padding: 0; text-align: left; width: 100%; }
 .track-list .album-title-target > .truncate { max-width: 100%; }
</style>
