<script lang="ts">
 import { currentView, goToAlbums, goToTracks, goToArtists, goToPlaylists, goBack } from "$lib/stores/view";
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
 let search="";
 const feedback=viewActions.feedback;
 $: pcName=$controllerState.snapshot?.outputs.find(o=>o.output.kind==="pc")?.name??"Your PC";
</script>
<div class="controller-shell">
 <header class="shell-header"><strong>Audion</strong><span>{pcName}</span><input aria-label="Search PC library" placeholder="Search your PC library" bind:value={search} /></header>
 <details open={!$controllerState.ready}><summary>{$controllerState.ready ? "Connected · PC settings" : "PC connection"}</summary><ControllerConnection /></details>
 {#if $controllerState.ready && !$controllerState.grants?.control}<p class="notice" role="status">You can browse this PC. Playback permission is required to control it.</p>{/if}
 <nav aria-label="Music library"><button on:click={goBack} aria-label="Back">←</button><button aria-current={$currentView.type==="albums"?"page":undefined} on:click={()=>{search="";goToAlbums();}}>Albums</button><button on:click={()=>{search="";goToTracks();}}>Tracks</button><button on:click={()=>{search="";goToArtists();}}>Artists</button><button on:click={()=>{search="";goToPlaylists();}}>Playlists</button><button on:click={()=>{search="";currentView.set({type:"liked-songs"});}}>Liked Songs</button></nav>
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
 {#if $feedback.message}<p class="feedback" class:error={$feedback.status==="error" || $feedback.status==="unknown"} role={$feedback.status==="error" || $feedback.status==="unknown"?"alert":"status"} aria-live="polite">{$feedback.message}</p>{/if}
 {#if $isMiniPlayer}<MiniPlayer />{:else}<PlayerBar />{/if}
 {#if $isFullScreen}<FullScreenPlayer />{/if}
 {#if $isQueueVisible}<QueuePanel />{/if}
</div>
<style>.controller-shell{display:flex;flex-direction:column;width:100%;height:100%;min-height:0;background:var(--bg-base);color:var(--text-primary)}.shell-header{display:flex;align-items:center;gap:16px;padding:16px 24px;border-bottom:1px solid var(--border-subtle)}.shell-header strong{font-size:1.25rem;color:var(--accent-primary)}.shell-header span{color:var(--text-secondary);font-size:.8rem}input{margin-left:auto;min-width:100px;max-width:320px;flex:1;background:var(--bg-highlight);color:var(--text-primary);border:1px solid var(--border-subtle);padding:10px 12px;border-radius:var(--radius-md)}details{max-height:50%;overflow:auto;border-bottom:1px solid var(--border-subtle)}summary{padding:8px 24px;font-size:.8rem;color:var(--text-secondary);cursor:pointer}nav{display:flex;gap:8px;overflow:auto;padding:12px 20px}nav button{background:none;color:var(--text-secondary);border:0;border-radius:var(--radius-full);padding:8px 12px;white-space:nowrap;cursor:pointer}nav button[aria-current=page]{background:var(--bg-highlight);color:var(--text-primary)}main{flex:1;min-height:0;overflow:auto}.feedback,.notice{margin:0;padding:8px 24px;font-size:.8rem;color:var(--text-secondary);background:var(--bg-elevated)}.error{color:var(--error-color)}@media(max-width:650px){.shell-header{padding:12px;gap:10px}.shell-header span{display:none}nav{padding:8px}.feedback{padding:8px 12px}}</style>
