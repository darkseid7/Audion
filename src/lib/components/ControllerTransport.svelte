<script lang="ts">
 import { controllerOverlay } from "$lib/application/controller-ui";
 import ControllerFeedback from "./ControllerFeedback.svelte";
 import { controllerState } from "$lib/application/controller/bootstrap";
 import { canExecute } from "$lib/application/capabilities";
 import type { ApplicationIntent, ExecutionResult } from "$lib/application/types";
 import { toggleFullScreen, toggleQueue, isFullScreen } from "$lib/stores/ui";
 import ConnectPanel from "./ConnectPanel.svelte";
 import ControllerArtwork from "./ControllerArtwork.svelte";
 export let execute: (intent: ApplicationIntent) => Promise<ExecutionResult>;
 export let variant: "bar" | "mini" | "full" = "bar";
 export let visible = true;
 let outputsOpen = false;
 $: playback = $controllerState.snapshot?.playback;
 const time = (seconds: number) => `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2,"0")}`;
 function seek(event: Event) { const input=event.target as HTMLInputElement, seconds=Number(input.value); input.value=String(playback?.position ?? 0); return execute({ type: "seek", seconds }); }
 function volume(event: Event) { const input=event.target as HTMLInputElement, volume=Number(input.value); input.value=String(playback?.volume ?? 0); return execute({ type: "set_volume", volume }); }
</script>
{#if visible}
 <div class="player-bar" class:full={variant === "full"} class:mini={variant === "mini"} aria-label={variant === "full" ? "Now playing" : "Player controls"} role={variant === "full" ? "dialog" : "region"} aria-modal={variant === "full" ? true : undefined} tabindex="-1" use:controllerOverlay={{close:toggleFullScreen,enabled:variant === "full"}}>
  {#if variant === "full"}<button class="close" on:click={toggleFullScreen} aria-label="Close now playing">⌄</button>{/if}
  <div class="track-info"><button class="art" on:click={toggleFullScreen} aria-label={variant === "full" ? "Close now playing" : "Open now playing"}><ControllerArtwork reference={playback?.track?.artwork} /></button><div><strong>{playback?.track?.title ?? "Nothing playing"}</strong><span>{playback?.track?.artist ?? "Select music from your PC"}</span><small>{playback?.track?.quality.badges.join(" · ") ?? ""}</small></div></div>
  <div class="player-center">
   <div class="transport">
    <button aria-label="Shuffle" aria-pressed={playback?.shuffle ?? false} disabled={!canExecute($controllerState,"set_shuffle")} on:click={() => execute({type:"set_shuffle",enabled:!playback?.shuffle})}>⇄</button>
    <button aria-label="Previous" disabled={!canExecute($controllerState,"previous")} on:click={() => execute({type:"previous"})}>|◀</button>
    <button class="play-button" aria-label={playback?.status === "playing" ? "Pause" : "Play"} disabled={!canExecute($controllerState,playback?.status === "playing" ? "pause" : "resume")} on:click={() => execute({type:playback?.status === "playing" ? "pause" : "resume"})}>{playback?.status === "playing" ? "Ⅱ" : "▶"}</button>
    <button aria-label="Next" disabled={!canExecute($controllerState,"next")} on:click={() => execute({type:"next"})}>▶|</button>
    <button aria-label="Repeat" title={`Repeat: ${playback?.repeat ?? "none"}`} aria-pressed={playback?.repeat !== "none"} disabled={!canExecute($controllerState,"set_repeat")} on:click={() => execute({type:"set_repeat",mode:playback?.repeat === "none" ? "all" : playback?.repeat === "all" ? "one" : "none"})}>↻{playback?.repeat === "one" ? "1" : ""}</button>
   </div>
   <div class="progress"><span>{time(playback?.position ?? 0)}</span><input type="range" aria-label="Seek" min="0" max={playback?.duration ?? 0} step="1" value={playback?.position ?? 0} disabled={!canExecute($controllerState,"seek") || !playback?.duration} on:change={seek} /><span>{time(playback?.duration ?? 0)}</span></div>
  </div>
  <div class="extras"><label>Volume <input type="range" aria-label="Volume" min="0" max="1" step="0.01" value={playback?.volume ?? 0} disabled={!canExecute($controllerState,"set_volume")} on:change={volume} /></label><button on:click={() => outputsOpen = true} aria-label="Choose output">Outputs</button><button on:click={toggleQueue} aria-label="Open queue">Queue</button></div>
  {#if variant === "full"}<ControllerFeedback />{/if}
  {#if playback?.error}<p role="alert">{playback.error.message}</p>{/if}
 </div>
{/if}
{#if outputsOpen}<ConnectPanel on:close={() => outputsOpen = false} />{/if}
<style>.player-bar{background:var(--bg-elevated);border-top:1px solid var(--border-subtle);display:flex;align-items:center;gap:var(--spacing-lg);padding:12px 20px;flex-wrap:wrap}.track-info{display:flex;align-items:center;gap:12px;min-width:180px;flex:1}.art{width:52px;height:52px;flex:none;padding:0;border-radius:var(--radius-sm);overflow:hidden}strong,span,small{display:block}strong{font-size:.9rem}span,small{font-size:.75rem;color:var(--text-secondary)}.player-center{flex:2;min-width:240px}.transport{display:flex;justify-content:center;align-items:center;gap:12px}.play-button{border-radius:50%;background:var(--text-primary);color:var(--bg-base);width:40px;height:40px}.progress{display:flex;align-items:center;gap:10px;margin-top:8px}input{accent-color:var(--accent-primary);min-width:0}.progress input{flex:1}.extras{display:flex;align-items:center;gap:8px;font-size:.75rem}.extras input{width:85px}button{background:none;border:0;color:var(--text-primary);padding:8px;cursor:pointer}button[aria-pressed=true]{color:var(--accent-primary)}button:disabled,input:disabled{opacity:.4;cursor:not-allowed}.full{position:fixed;inset:0;z-index:100;flex-direction:column;justify-content:center;overflow:auto}.full .track-info{flex:0;flex-direction:column;text-align:center}.full .art{width:min(48vw,320px);height:min(48vw,320px)}.full .player-center{flex:0;width:min(90%,560px)}.close{position:absolute;top:16px;right:16px}.mini .extras{display:none}@media(max-width:767px){.player-bar{gap:8px;padding:10px}.player-center{order:2;flex-basis:100%}.extras label{display:none}.full .extras label{display:block}}
 .player-bar { min-width:0; }
 .track-info { min-width:0; }
 .track-info > div { min-width:0; }
 .track-info strong,.track-info span { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
 button,input[type=range] { min-height:44px; }
 button { min-width:44px; }
 .full { padding:calc(60px + var(--safe-area-top)) calc(20px + var(--safe-area-right)) calc(20px + var(--safe-area-bottom)) calc(20px + var(--safe-area-left)); }
 .full .close { top:calc(8px + var(--safe-area-top)); right:calc(8px + var(--safe-area-right)); }
 .full { flex-wrap:nowrap; }
 @media(min-width:768px) {
  .player-bar:not(.full) { display:grid; grid-template-columns:minmax(120px,1fr) minmax(250px,2fr) minmax(140px,1fr); gap:12px; }
  .player-bar:not(.full) .extras { flex-wrap:wrap; justify-content:flex-end; gap:4px; }
  .player-bar:not(.full) .extras label { flex-basis:100%; display:flex; align-items:center; justify-content:flex-end; gap:8px; }
 }
 @media(max-width:767px) {
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
</style>
