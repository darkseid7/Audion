<script lang="ts">
 import { onMount } from "svelte";
 import { isMobile } from "$lib/stores/mobile";
 import { registerControllerBack, handleControllerBack } from "$lib/application/controller-ui";
 import { navigationHistory, type ViewType } from "$lib/stores/view";
 import ControllerFeedback from "./ControllerFeedback.svelte";
 import { currentView, goBack } from "$lib/stores/view";
 import { controllerState } from "$lib/application/controller/bootstrap";
 import { viewActions } from "$lib/application/view-actions";
 import { isQueueVisible, isFullScreen, isMiniPlayer } from "$lib/stores/ui";
 import ControllerConnection from "./ControllerConnection.svelte";
 import ControllerBrowse from "./ControllerBrowse.svelte";
 import AlbumGrid from "./AlbumGrid.svelte";
 import AlbumDetail from "./AlbumDetail.svelte";
 import ArtistDetail from "./ArtistDetail.svelte";
 import PlaylistDetail from "./PlaylistDetail.svelte";
 import LikedSongs from "./LikedSongs.svelte";
 import TrackList from "./TrackList.svelte";
 import PlayerBar from "./PlayerBar.svelte";
 import MiniPlayer from "./MiniPlayer.svelte";
 import FullScreenPlayer from "./FullScreenPlayer.svelte";
 import QueuePanel from "./QueuePanel.svelte";
 let search = "";
 let searchInput: HTMLInputElement;
 let connectionOpen = !$controllerState.ready;
 let wasReady = $controllerState.ready;
 $: if ($controllerState.ready !== wasReady) { wasReady = $controllerState.ready; connectionOpen = !wasReady; }
 const destinations: { type: ViewType; label: string; icon: string }[] = [
  { type: "albums", label: "Albums", icon: "▦" },
  { type: "tracks", label: "Tracks", icon: "♫" },
  { type: "artists", label: "Artists", icon: "♬" },
  { type: "playlists", label: "Playlists", icon: "☷" },
  { type: "liked-songs", label: "Liked Songs", icon: "♡" },
 ];
 function active(type: ViewType) {
  return $currentView.type === type || (type === "albums" && $currentView.type === "album-detail") ||
   (type === "artists" && $currentView.type === "artist-detail") || (type === "playlists" && $currentView.type === "playlist-detail");
 }
 function back() {
  if (connectionOpen) { connectionOpen = false; return true; }
  if (search) { search = ""; searchInput?.focus(); return true; }
  if ($navigationHistory.canGoBack) { goBack(); return true; }
  return false;
 }
 function keydown(event: KeyboardEvent) {
  if (event.defaultPrevented) return;
  if (event.key === "Escape" || (event.altKey && event.key === "ArrowLeft")) {
   if (handleControllerBack()) event.preventDefault();
  } else if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f") {
   event.preventDefault(); searchInput?.focus();
  }
 }
 onMount(() => registerControllerBack(back));
 $: pcName=$controllerState.snapshot?.outputs.find(o=>o.output.kind==="pc")?.name??"Your PC";
</script>
<svelte:window on:keydown={keydown} />
<div class="controller-shell" data-layout={$isMobile ? "compact" : "expanded"}>
 <header class="shell-header">
  <strong class="brand">Audion</strong><button class="back" on:click={back} disabled={!connectionOpen && !search && !$navigationHistory.canGoBack} aria-label="Back">←</button>
  <span class="pc-name">{pcName}</span><input bind:this={searchInput} aria-label="Search PC library" placeholder="Search your PC library" bind:value={search} />
 </header>
 <div class="connection">
  <details bind:open={connectionOpen}><summary>{$controllerState.ready ? "Connected · PC settings" : "PC connection"}</summary><ControllerConnection /></details>
  {#if $controllerState.ready && !$controllerState.grants?.control}<p class="notice" role="status">You can browse this PC. Playback permission is required to control it.</p>{/if}
 </div>
 <nav aria-label="Music library"><span class="nav-heading">Your library</span>
  {#each destinations as destination}<button aria-current={active(destination.type) && !search ? "page" : undefined} on:click={()=>{search="";currentView.set({type:destination.type});}}><span aria-hidden="true" class="nav-icon">{destination.icon}</span><span>{destination.label}</span></button>{/each}
 </nav>
 <main>
 {#if search.trim()}<ControllerBrowse query={{type:"search",text:search.trim()}} heading="Search results" onNavigate={()=>search=""} execute={viewActions.execute} enqueue={viewActions.queueQuery} />
 {:else if $currentView.type==="album-detail" && $currentView.id!==undefined}<AlbumDetail albumId={$currentView.id} />
 {:else if $currentView.type==="artist-detail" && $currentView.name}<ArtistDetail artistName={$currentView.name} />
 {:else if $currentView.type==="playlist-detail" && $currentView.id!==undefined}<PlaylistDetail playlistId={$currentView.id} controllerTitle={$currentView.name ?? "Playlist"} />
 {:else if $currentView.type==="tracks"}<TrackList />
 {:else if $currentView.type==="liked-songs"}<LikedSongs />
 {:else if $currentView.type==="artists"}<ControllerBrowse query={{type:"artists"}} heading="Artists" execute={viewActions.execute} enqueue={viewActions.queueQuery} />
 {:else if $currentView.type==="playlists"}<ControllerBrowse query={{type:"playlists"}} heading="Playlists" execute={viewActions.execute} enqueue={viewActions.queueQuery} />
 {:else}<AlbumGrid />{/if}
 </main>
 <div class="shell-feedback"><ControllerFeedback /></div>
 <div class="shell-player">{#if $isMiniPlayer}<MiniPlayer />{:else}<PlayerBar />{/if}</div>
 {#if $isFullScreen}<FullScreenPlayer />{/if}
 {#if $isQueueVisible}<QueuePanel />{/if}
</div>
<style>
 .controller-shell { display:grid; grid-template-columns:200px minmax(0,1fr); grid-template-rows:auto auto minmax(0,1fr) auto auto; grid-template-areas:"header header" "nav connection" "nav content" "nav feedback" "player player"; width:100%; height:100%; min-height:0; min-width:0; overflow:hidden; background:var(--bg-base); color:var(--text-primary); padding:var(--safe-area-top) var(--safe-area-right) var(--safe-area-bottom) var(--safe-area-left); }
 .shell-header { grid-area:header; display:flex; align-items:center; gap:16px; padding:12px 24px; border-bottom:1px solid var(--border-subtle); min-width:0; }
 .brand { width:152px; flex-shrink:0; font-size:1.25rem; color:var(--accent-primary); }
 .pc-name { color:var(--text-secondary); font-size:.8rem; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
 input { margin-left:auto; min-width:0; max-width:400px; flex:1; background:var(--bg-highlight); color:var(--text-primary); border:1px solid var(--border-subtle); padding:10px 12px; border-radius:var(--radius-md); min-height:44px; }
 button { color:var(--text-secondary); background:none; border:0; cursor:pointer; min-width:44px; min-height:44px; }
 button:disabled { opacity:.4; cursor:default; }
 .connection { grid-area:connection; min-width:0; max-height:35dvh; overflow:auto; border-bottom:1px solid var(--border-subtle); }
 summary { padding:12px 24px; min-height:44px; font-size:.8rem; color:var(--text-secondary); cursor:pointer; }
 nav { grid-area:nav; display:flex; flex-direction:column; gap:4px; padding:24px 12px; overflow:auto; border-right:1px solid var(--border-subtle); }
 .nav-heading { padding:0 12px 16px; font-size:.75rem; font-weight:600; color:var(--text-secondary); }
 nav button { display:flex; align-items:center; gap:12px; border-radius:var(--radius-sm); padding:8px 12px; text-align:left; white-space:nowrap; font-size:.875rem; }
 .nav-icon { font-size:1.25rem; width:24px; text-align:center; }
 nav button[aria-current=page] { background:var(--bg-highlight); color:var(--accent-primary); }
 main { grid-area:content; min-height:0; min-width:0; overflow:hidden; }
 .shell-feedback { grid-area:feedback; min-width:0; }
 .shell-player { grid-area:player; min-width:0; }
 .notice { margin:0; padding:8px 24px; font-size:.8rem; color:var(--text-secondary); background:var(--bg-elevated); }
 @media(width < 768px) {
  .controller-shell { grid-template-columns:minmax(0,1fr); grid-template-rows:auto auto minmax(0,1fr) auto auto auto; grid-template-areas:"header" "connection" "content" "feedback" "player" "nav"; }
  .shell-header { padding:8px 12px; gap:8px; }
  .brand { width:auto; font-size:1.1rem; }
  .pc-name { display:none; }
  input { font-size:16px; }
  nav { flex-direction:row; justify-content:space-around; gap:0; padding:4px; border-right:0; border-top:1px solid var(--border-subtle); }
  .nav-heading { display:none; }
  nav button { flex:1; flex-direction:column; justify-content:center; gap:2px; min-width:0; padding:4px; font-size:.65rem; }
  .nav-icon { font-size:1.2rem; }
  summary { padding:8px 16px; }
 }
</style>
