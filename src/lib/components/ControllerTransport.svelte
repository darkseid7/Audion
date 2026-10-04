<script lang="ts">
 import { controllerOverlay } from "$lib/application/controller-ui";
 import ControllerFeedback from "./ControllerFeedback.svelte";
 import { controllerState } from "$lib/application/controller/bootstrap";
 import { canExecute } from "$lib/application/capabilities";
 import type { ApplicationIntent, ExecutionResult } from "$lib/application/types";
 import { toggleFullScreen, toggleQueue, isFullScreen } from "$lib/stores/ui";
 import ConnectPanel from "./ConnectPanel.svelte";
 import Player from "./presentation/Player.svelte";
 import ControllerArtwork from "./ControllerArtwork.svelte";
 import { goToAlbumDetail, goToArtistDetail } from "$lib/stores/view";
 export let execute: (intent: ApplicationIntent) => Promise<ExecutionResult>;
 export let variant: "bar" | "mini" | "full" = "bar";
 export let visible = true;
 function expandedTransition(_node: HTMLElement) {
  if (variant !== "full" || (typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches)) return {duration:0};
  return {duration: 320, easing:(t:number)=>1-Math.pow(1-t,3), css:(t:number)=>`opacity:${t};transform:translateY(${(1-t)*16}px)`};
 }
 let outputsOpen = false;
 $: playback = $controllerState.snapshot?.playback;
 function openTrackAlbum() {
  const albumId = playback?.track?.albumId;
  if (albumId == null || !Number.isSafeInteger(albumId) || albumId <= 0) return;
  if ($isFullScreen) toggleFullScreen();
  goToAlbumDetail(albumId);
 }
 function openTrackArtist() {
  const artist = playback?.track?.artist;
  if (!artist?.trim()) return;
  if ($isFullScreen) toggleFullScreen();
  goToArtistDetail(artist);
 }
 $: playerBadges = playback?.track?.quality.badges.length ? playback.track.quality.badges : [playback?.track?.quality.format, playback?.track?.quality.bitrate ? `${playback.track.quality.bitrate}kbps` : null].filter((badge): badge is string => !!badge);
 const time = (seconds: number) => `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2,"0")}`;
 function seek(event: Event) { const input=event.target as HTMLInputElement, seconds=Number(input.value); input.value=String(playback?.position ?? 0); return execute({ type: "seek", seconds }); }
 function volume(event: Event) { const input=event.target as HTMLInputElement, volume=Number(input.value); input.value=String(playback?.volume ?? 0); return execute({ type: "set_volume", volume }); }
</script>
{#if visible}
 <div transition:expandedTransition class="player-bar" class:full={variant === "full"} class:mini={variant === "mini"} aria-label={variant === "full" ? "Now playing" : "Player controls"} role={variant === "full" ? "dialog" : "region"} aria-modal={variant === "full" ? true : undefined} tabindex="-1" use:controllerOverlay={{close:toggleFullScreen,enabled:variant === "full"}}>
  {#if variant === "full"}<button class="close" on:click={toggleFullScreen} aria-label="Close now playing"><svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18"/></svg></button>{/if}
  <Player {variant} title={playback?.track?.title ?? "Nothing playing"} artist={playback?.track?.artist ?? "Select music from your PC"} badges={playerBadges} onOpen={toggleFullScreen} onTitle={playback?.track?.albumId != null && playback.track.albumId > 0 ? openTrackAlbum : undefined} onArtist={playback?.track?.artist?.trim() ? openTrackArtist : undefined}><svelte:fragment slot="artwork"><ControllerArtwork reference={playback?.track?.artwork} alt={playback?.track?.album ?? playback?.track?.title ?? "Album art"} /></svelte:fragment></Player>
  <div class="player-center">
   <div class="transport">
    <button aria-label="Shuffle" class:active={playback?.shuffle===true} aria-pressed={playback?.shuffle ?? false} disabled={!canExecute($controllerState,"set_shuffle")} on:click={() => execute({type:"set_shuffle",enabled:!playback?.shuffle})}><svg viewBox="0 0 24 24" fill="currentColor" width="20" height="20" aria-hidden="true"><path d="M10.59 9.17L5.41 4 4 5.41l5.17 5.17 1.42-1.41zM14.5 4l2.04 2.04L4 18.59 5.41 20 17.96 7.46 20 9.5V4h-5.5zm.33 9.41l-1.41 1.41 3.13 3.13L14.5 20H20v-5.5l-2.04 2.04-3.13-3.13z"/></svg></button>
    <button aria-label="Previous" disabled={!canExecute($controllerState,"previous")} on:click={() => execute({type:"previous"})}><svg viewBox="0 0 24 24" fill="currentColor" width="24" height="24" aria-hidden="true"><path d="M6 6h2v12H6zm3.5 6l8.5 6V6z"/></svg></button>
    <button class="play-button" aria-label={playback?.status === "playing" ? "Pause" : "Play"} disabled={!canExecute($controllerState,playback?.status === "playing" ? "pause" : "resume")} on:click={() => execute({type:playback?.status === "playing" ? "pause" : "resume"})}><svg viewBox="0 0 24 24" fill="currentColor" width="24" height="24" aria-hidden="true"><path d={playback?.status === "playing" ? "M6 4h4v16H6zm8 0h4v16h-4z" : "M8 5v14l11-7z"}/></svg></button>
    <button aria-label="Next" disabled={!canExecute($controllerState,"next")} on:click={() => execute({type:"next"})}><svg viewBox="0 0 24 24" fill="currentColor" width="24" height="24" aria-hidden="true"><path d="M6 18l8.5-6L6 6v12zM16 6v12h2V6h-2z"/></svg></button>
    <button aria-label="Repeat" class:active={!!playback && playback.repeat!=="none"} title={`Repeat: ${playback?.repeat ?? "none"}`} aria-pressed={playback?.repeat !== "none"} disabled={!canExecute($controllerState,"set_repeat")} on:click={() => execute({type:"set_repeat",mode:playback?.repeat === "none" ? "all" : playback?.repeat === "all" ? "one" : "none"})}><svg viewBox="0 0 24 24" fill="currentColor" width="20" height="20" aria-hidden="true"><path d="M7 7h10v3l4-4-4-4v3H5v6h2V7zm10 10H7v-3l-4 4 4 4v-3h12v-6h-2v4z"/></svg>{#if playback?.repeat === "one"}<span class="repeat-one">1</span>{/if}</button>
   </div>
   <div class="progress"><span>{time(playback?.position ?? 0)}</span><input type="range" aria-label="Seek" min="0" max={playback?.duration ?? 0} step="1" value={playback?.position ?? 0} disabled={!canExecute($controllerState,"seek") || !playback?.duration} on:change={seek} /><span>{time(playback?.duration ?? 0)}</span></div>
  </div>
  <div class="extras"><label class="volume-control" title={`Volume: ${Math.round((playback?.volume ?? 0) * 100)}%`}><svg viewBox="0 0 24 24" fill="currentColor" width="18" height="18" aria-hidden="true"><path d="M3 9v6h4l5 5V4L7 9H3zm13.5 3c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02zM14 3.23v2.06c2.89.86 5 3.54 5 6.71s-2.11 5.85-5 6.71v2.06c4.01-.91 7-4.49 7-8.77s-2.99-7.86-7-8.77z"/></svg><input class="volume-range" type="range" aria-label="Volume" aria-valuetext={`${Math.round((playback?.volume ?? 0) * 100)}%`} style={`--volume-fill: ${Math.round((playback?.volume ?? 0) * 100)}%`} min="0" max="1" step="0.01" value={playback?.volume ?? 0} disabled={!canExecute($controllerState,"set_volume")} on:change={volume} /></label><button on:click={() => outputsOpen = true} aria-label="Choose output" title="Outputs" class="extra-icon-button"><svg viewBox="0 0 24 24" fill="currentColor" width="18" height="18" aria-hidden="true"><path d="M19,2H5A3,3,0,0,0,2,5V15a3,3,0,0,0,3,3H9.17l-1.42,1.41a1,1,0,0,0,0,1.42,1,1,0,0,0,1.42,0L11,18.99,12.83,20.83a1,1,0,0,0,1.42,0,1,1,0,0,0,0-1.42L12.83,18H19a3,3,0,0,0,3-3V5A3,3,0,0,0,19,2Zm1,13a1,1,0,0,1-1,1H5a1,1,0,0,1-1-1V5A1,1,0,0,1,5,4H19a1,1,0,0,1,1,1Z"/></svg></button><button on:click={toggleQueue} aria-label="Open queue" title="Queue" class="extra-icon-button"><svg viewBox="0 0 24 24" fill="currentColor" width="18" height="18" aria-hidden="true"><path d="M15 6H3v2h12V6zm0 4H3v2h12v-2zM3 16h8v-2H3v2zM17 6v8.18c-.31-.11-.65-.18-1-.18-1.66 0-3 1.34-3 3s1.34 3 3 3 3-1.34 3-3V8h3V6h-5z"/></svg></button></div>
  {#if variant === "full"}<ControllerFeedback />{/if}
  {#if playback?.error}<p role="alert">{playback.error.message}</p>{/if}
 </div>
{/if}
{#if outputsOpen}<ConnectPanel on:close={() => outputsOpen = false} />{/if}
<style>
 .transport button { display: inline-flex; align-items: center; justify-content: center; position: relative; }
 .transport button.active { color: var(--accent-primary); }
 .transport .repeat-one { position: absolute; font-size: .6rem; bottom: 2px; right: 2px; color: var(--accent-primary); }
 .transport .play-button { width: 44px; height: 44px; }
.player-bar{background:var(--bg-elevated);border-top:1px solid var(--border-subtle);display:flex;align-items:center;gap:var(--spacing-lg);padding:12px 20px;flex-wrap:wrap}.track-info{display:flex;align-items:center;gap:12px;min-width:180px;flex:1}.art{width:52px;height:52px;flex:none;padding:0;border-radius:var(--radius-sm);overflow:hidden}strong,span,small{display:block}strong{font-size:.9rem}span,small{font-size:.75rem;color:var(--text-secondary)}.player-center{flex:2;min-width:240px}.transport{display:flex;justify-content:center;align-items:center;gap:12px}.play-button{border-radius:50%;background:var(--text-primary);color:var(--bg-base);width:40px;height:40px}.progress{display:flex;align-items:center;gap:10px;margin-top:8px}input{accent-color:var(--accent-primary);min-width:0}.progress input{flex:1}.extras{display:flex;align-items:center;gap:8px;font-size:.75rem}.extras input{width:85px}button{background:none;border:0;color:var(--text-primary);padding:8px;cursor:pointer}button[aria-pressed=true]{color:var(--accent-primary)}button:disabled,input:disabled{opacity:.4;cursor:not-allowed}.full{position:fixed;inset:0;z-index:100;flex-direction:column;justify-content:center;overflow:auto}.full .track-info{flex:0;flex-direction:column;text-align:center}.full .art{width:min(48vw,320px);height:min(48vw,320px)}.full .player-center{flex:0;width:min(90%,560px)}.close{position:absolute;top:16px;right:16px}.mini .extras{display:none}@media(width < 768px){.player-bar{gap:8px;padding:10px}.player-center{order:2;flex-basis:100%}.extras label{display:none}.full .extras label{display:block}}
 .player-bar { min-width:0; }
 .track-info { min-width:0; }
 .track-info > div { min-width:0; }
 .track-info strong,.track-info span { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
 button,input[type=range] { min-height:44px; }
 button { min-width:44px; }
 .full { padding:calc(60px + var(--safe-area-top)) calc(20px + var(--safe-area-right)) calc(20px + var(--safe-area-bottom)) calc(20px + var(--safe-area-left)); }
 .full .close { top:calc(24px + var(--safe-area-top)); right:calc(20px + var(--safe-area-right)); width:44px; height:44px; display:flex; align-items:center; justify-content:center; border-radius:var(--radius-full); background:var(--bg-elevated); border:1px solid var(--border-subtle); color:var(--text-primary); }
 .full { flex-wrap:nowrap; }
 /* Keep theme gradients, but composite them over an opaque theme surface. */
 .player-bar.full { background-color: var(--bg-base) !important; }
 @media(min-width:768px) {
  .player-bar:not(.full):not(.mini) { height: calc(var(--player-height) + 60px); box-sizing: border-box; padding: 0 calc(var(--spacing-md) + 2px); gap: clamp(20px, 2.2vw, 36px); }
  .player-bar:not(.full) { display:grid; grid-template-columns:minmax(120px,1fr) minmax(250px,2fr) minmax(140px,1fr); gap:12px; }
  .player-bar:not(.full) .extras { flex-wrap:wrap; justify-content:flex-end; gap:4px; }
  .player-bar:not(.full) .extras label { flex-basis:100%; display:flex; align-items:center; justify-content:flex-end; gap:8px; }
 }
 @media(width < 768px) {
  .player-bar:not(.full) { display:grid; grid-template-columns:minmax(0,1fr) auto; gap:4px 8px; padding:8px 12px; }
  .player-bar:not(.full) .player-center { order:initial; min-width:0; }
  .player-bar:not(.full) .transport { gap:0; }
  .player-bar:not(.full) .transport button:first-child,.player-bar:not(.full) .transport button:nth-child(2),.player-bar:not(.full) .transport button:last-child,.player-bar:not(.full) .progress { display:none; }
  .player-bar:not(.full) .extras { grid-column:1/-1; justify-content:flex-end; }
  .player-bar:not(.full) .extras button { font-size:.75rem; padding:4px 12px; }
  .player-bar:not(.full) .art { width:44px; height:44px; }
  .full .extras { flex-wrap:wrap; justify-content:center; }
 }
 @media(max-height:520px) {
  .full { justify-content:flex-start; }
  .full .art { width:96px; height:96px; }
  .player-bar:not(.full) .extras { grid-column:auto; }
 }
 .extras .extra-icon-button { display: inline-flex; align-items: center; justify-content: center; width: 44px; height: 44px; padding: 0; border-radius: var(--radius-sm); }
 .extras .extra-icon-button:hover { background: var(--bg-highlight); }
 .player-bar.full .volume-control { display: inline-flex; align-items: center; gap: 8px; }
 .volume-control svg { flex-shrink: 0; color: var(--text-secondary); }
 .volume-control .volume-range { appearance: none; -webkit-appearance: none; width: 112px; min-height: 44px; margin: 0; padding: 0; border: 0; background: transparent; cursor: pointer; }
 .volume-range::-webkit-slider-runnable-track { height: 4px; border-radius: var(--radius-full); background: linear-gradient(to right, var(--accent-primary) 0 var(--volume-fill), var(--bg-highlight) var(--volume-fill) 100%); }
 .volume-range::-webkit-slider-thumb { appearance: none; -webkit-appearance: none; width: 12px; height: 12px; margin-top: -4px; border: 0; border-radius: 50%; background: var(--accent-primary); }
 .volume-range::-moz-range-track { height: 4px; border-radius: var(--radius-full); background: var(--bg-highlight); }
 .volume-range::-moz-range-progress { height: 4px; border-radius: var(--radius-full); background: var(--accent-primary); }
 .volume-range::-moz-range-thumb { width: 12px; height: 12px; border: 0; border-radius: 50%; background: var(--accent-primary); }
 .volume-range:disabled { opacity: .4; cursor: not-allowed; }
 .volume-range:focus-visible { outline: 2px solid var(--accent-primary); outline-offset: 2px; border-radius: var(--radius-sm); }
 @media (pointer: coarse) {
   .volume-range::-webkit-slider-thumb { width: 14px; height: 14px; margin-top: -5px; }
   .volume-range::-moz-range-thumb { width: 14px; height: 14px; }
 }
</style>
