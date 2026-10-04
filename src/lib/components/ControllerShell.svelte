<script lang="ts">
 import AppShell from "./presentation/AppShell.svelte";
 import Navigation from "./presentation/Navigation.svelte";
 import AppearanceSettings from "./presentation/AppearanceSettings.svelte";
 import { theme } from "$lib/stores/theme";
 import { _, locale } from "svelte-i18n";
 import type { ApplicationQuery } from "$lib/application/types";
 import type { NavigationRow, NavigationSection } from "$lib/application/presentation/types";
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
 let navigationSections: NavigationSection[];
 let librarySections: NavigationSection[];
 let bottomSections: NavigationSection[];
 let search = "";
 let connectionOpen = !$controllerState.ready;
 let wasReady = $controllerState.ready;
 $: if ($controllerState.ready !== wasReady) { wasReady = $controllerState.ready; connectionOpen = !wasReady; }
 function translated(key: string, fallback: string): string { return $locale ? $_(key, { default: fallback }) : fallback; }
 function navigationRow(id: string, label: string): NavigationRow {
  const query: Partial<Record<string, ApplicationQuery["type"]>> = { albums: "albums", "liked-songs": "liked_tracks", tracks: "tracks", artists: "artists", playlists: "playlists" };
  const capability = query[id];
  const supported = id === "settings" || !!capability && !!$controllerState.snapshot?.capabilities.queries.includes(capability);
  return { id, label, icon: id as NavigationRow["icon"], active: active(id as ViewType) && !search, availability: supported ? { enabled: true } : { enabled: false, reason: "This view is not available from the connected PC." } };
 }
 $: { void $currentView; void $controllerState; void search; void $_; void $locale; navigationSections = [
  { id: "library", label: translated('sidebar.library', 'Library'), rows: [navigationRow("home", translated('sidebar.home', "Home")),navigationRow("albums", translated('sidebar.albums', "Albums")),navigationRow("liked-songs", translated('sidebar.likedSongs', "Liked Songs")),navigationRow("listen-later", translated('sidebar.listenLater', "Escuchar más tarde")),navigationRow("recently-played", translated('sidebar.thisWeek', "This Week")),navigationRow("discover", translated('sidebar.discover', "Discover")),navigationRow("tracks", translated('sidebar.allTracks', "All Tracks")),navigationRow("artists", translated('sidebar.artists', "Artists"))] },
  { id: "playlists", label: translated('sidebar.playlists', 'Playlists'), rows: [navigationRow("playlists", translated('sidebar.allPlaylists', 'All Playlists'))] },
  { id: "settings", label: translated('sidebar.settings', 'Settings'), rows: [navigationRow("plugins", translated('sidebar.plugins', 'Plugins')), navigationRow("settings", translated('sidebar.settings', 'Settings'))] }
 ]; }
 $: { void $currentView; void $controllerState; librarySections = [{ id: "library", label: "", rows: [navigationRow("tracks", "Songs"), navigationRow("albums", "Albums"), navigationRow("artists", "Artists"), navigationRow("playlists", "Playlists"), navigationRow("liked-songs", "Liked Songs")] }]; }
 $: { void $currentView; void $controllerState; bottomSections = [{ id: "tabs", label: "", rows: [
  { ...navigationRow("home", "Home"), active: false },
  { ...navigationRow("albums", "Library"), id: "library", icon: "library" as const, active: $currentView.type !== "settings" },
  { ...navigationRow("plugins", "Plugins"), active: $currentView.type === "settings" },
  { id: "actions", label: "Actions", icon: "actions" as const, active: false, availability: { enabled: false as const, reason: "Plugin actions require desktop support." } }
 ] }]; }
 function navigateShared(id: string): void {
  if (id === "library") id = "albums";
  const row = navigationSections.flatMap(section => section.rows).find(row => row.id === id);
  if (!row?.availability.enabled) return;
  search = ""; currentView.set({ type: id as ViewType });
 }
 function addCustomColor(color: string): void { theme.addCustomColor(color); theme.setAccentColor(color); }
 function active(type: ViewType) {
  return $currentView.type === type || (type === "albums" && $currentView.type === "album-detail") ||
   (type === "artists" && $currentView.type === "artist-detail") || (type === "playlists" && $currentView.type === "playlist-detail");
 }
 function back() {
  if ($currentView.type === "settings" && connectionOpen) { connectionOpen = false; return true; }
  if (search) { search = ""; return true; }
  if ($navigationHistory.canGoBack) { goBack(); return true; }
  return false;
 }
 function keydown(event: KeyboardEvent) {
  if (event.defaultPrevented) return;
  if (event.key === "Escape" || (event.altKey && event.key === "ArrowLeft")) {
   if (handleControllerBack()) event.preventDefault();
  }
 }
 onMount(() => registerControllerBack(back));
 $: pcName=$controllerState.snapshot?.outputs.find(o=>o.output.kind==="pc")?.name??"Your PC";
</script>
<svelte:window on:keydown={keydown} />
<AppShell layout={$isMobile ? "compact" : "expanded"} safeArea={true} reserveBottomNavigation={true}>
 <Navigation collapsible slot="sidebar" sections={navigationSections} onNavigate={navigateShared}>
  <button slot="footer" class="add-folder-btn" aria-label="Add Music Folder" disabled title="Music folders are managed on the PC."><svg viewBox="0 0 24 24" fill="currentColor" width="20" height="20" aria-hidden="true"><path d="M19 13h-6v6h-2v-6H5v-2h6V5h2v6h6v2z" /></svg><span>{translated('sidebar.addMusicFolder', 'Add Music Folder')}</span></button>
 </Navigation>
 <header slot="header" class="shell-header">
  <button class="back" on:click={back} disabled={!connectionOpen && !search && !$navigationHistory.canGoBack} aria-label="Back">←</button>
  <span class="pc-name">{pcName}</span>

 </header>
 <svelte:fragment slot="status">
  {#if !$controllerState.ready && $currentView.type !== "settings"}<div class="connection-notice" role="status"><span>PC connection unavailable.</span><button on:click={()=>navigateShared("settings")}>Connection settings</button></div>
  {:else if $controllerState.ready && !$controllerState.grants?.control && $currentView.type !== "settings"}<p class="notice" role="status">You can browse this PC. Playback permission is required to control it.</p>{/if}
 </svelte:fragment>
 <main class="controller-content">
 {#if $isMobile && !search.trim() && ["tracks", "albums", "artists", "playlists", "liked-songs"].includes($currentView.type)}<Navigation variant="library" sections={librarySections} onNavigate={navigateShared} />{/if}
 {#if $currentView.type === "settings" && !search.trim()}
   <div class="controller-appearance"><h1>Settings</h1><section class="connection-settings" aria-label="PC connection settings"><h2>PC connection</h2><details bind:open={connectionOpen}><summary>{$controllerState.ready ? "Connected · PC settings" : "PC connection"}</summary><ControllerConnection /></details>{#if $controllerState.ready && !$controllerState.grants?.control}<p class="notice" role="status">You can browse this PC. Playback permission is required to control it.</p>{/if}</section><p class="preference-owner">This controller</p><AppearanceSettings state={$theme} onModeChange={theme.setMode} onAccentChange={theme.setAccentColor} onCustomAccent={addCustomColor} /></div>
 {:else if search.trim()}<ControllerBrowse query={{type:"search",text:search.trim()}} heading="Search results" onNavigate={()=>search=""} execute={viewActions.execute} enqueue={viewActions.queueQuery} />
 {:else if $currentView.type==="album-detail" && $currentView.id!==undefined}<AlbumDetail albumId={$currentView.id} />
 {:else if $currentView.type==="artist-detail" && $currentView.name}<ArtistDetail artistName={$currentView.name} />
 {:else if $currentView.type==="playlist-detail" && $currentView.id!==undefined}<PlaylistDetail playlistId={$currentView.id} controllerTitle={$currentView.name ?? "Playlist"} />
 {:else if $currentView.type==="tracks"}<TrackList />
 {:else if $currentView.type==="liked-songs"}<LikedSongs />
 {:else if $currentView.type==="artists"}<ControllerBrowse query={{type:"artists"}} heading="Artists" execute={viewActions.execute} enqueue={viewActions.queueQuery} />
 {:else if $currentView.type==="playlists"}<ControllerBrowse query={{type:"playlists"}} heading="Playlists" execute={viewActions.execute} enqueue={viewActions.queueQuery} />
 {:else}<AlbumGrid />{/if}

 </main>
 <ControllerFeedback slot="feedback" />
 <svelte:fragment slot="player">{#if $isMiniPlayer}<MiniPlayer />{:else}<PlayerBar />{/if}</svelte:fragment>
 <svelte:fragment slot="panels">{#if $isFullScreen}<FullScreenPlayer />{/if}{#if $isQueueVisible}<QueuePanel />{/if}</svelte:fragment>
 <Navigation slot="bottom-navigation" variant="compact" sections={bottomSections} hasPlayer={!!$controllerState.snapshot?.playback.track} onNavigate={navigateShared} />
</AppShell>
<style>
 .shell-header { display:flex; align-items:center; gap:16px; padding:12px 24px; border-bottom:1px solid var(--border-subtle); min-width:0; }
 .pc-name { color:var(--text-secondary); font-size:.8rem; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
 input { margin-left:auto; min-width:0; max-width:400px; flex:1; background:var(--bg-highlight); color:var(--text-primary); border:1px solid var(--border-subtle); padding:10px 12px; border-radius:var(--radius-md); min-height:44px; }
 button { color:var(--text-secondary); background:none; border:0; cursor:pointer; min-width:44px; min-height:44px; }
 button:disabled { opacity:.4; cursor:default; }
 .connection-settings { margin: var(--spacing-lg) 0; border: 1px solid var(--border-subtle); border-radius: var(--radius-md); background: var(--bg-elevated); }
 .connection-settings h2 { margin: 0; padding: var(--spacing-md); font-size: 1rem; }
 .connection-notice { display: flex; align-items: center; justify-content: space-between; gap: var(--spacing-sm); padding: 0 var(--spacing-md); color: var(--text-secondary); font-size: .8rem; }
 summary { padding:12px 24px; min-height:44px; font-size:.8rem; color:var(--text-secondary); cursor:pointer; }
 .controller-content { display:flex; flex-direction:column; flex:1; min-height:0; min-width:0; overflow:hidden; }
 .controller-appearance { overflow:auto; min-height:0; padding:var(--spacing-lg); }
 .controller-appearance h1 { font-size:2rem; margin-bottom:var(--spacing-sm); }
 .preference-owner { color:var(--text-secondary); margin-bottom:var(--spacing-lg); }
 .notice { margin:0; padding:8px 24px; font-size:.8rem; color:var(--text-secondary); background:var(--bg-elevated); }
 @media(width < 768px) {
  .shell-header { padding:8px 12px; gap:8px; }
  .pc-name { display:none; }
  input { font-size:16px; }
  summary { padding:8px 16px; }

 }
</style>
