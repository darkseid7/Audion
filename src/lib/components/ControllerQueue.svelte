<script lang="ts">
 import { controllerOverlay } from "$lib/application/controller-ui";
 import ControllerFeedback from "./ControllerFeedback.svelte";
 import { onDestroy } from "svelte";
 import { controllerState } from "$lib/application/controller/bootstrap";
 import { createControllerPage } from "$lib/application/controller/views";
 import { canExecute } from "$lib/application/capabilities";
 import { toggleQueue } from "$lib/stores/ui";
 import type { ApplicationIntent, ExecutionResult, QueueEntry } from "$lib/application/types";
 import ControllerArtwork from "./ControllerArtwork.svelte";
 export let execute: (intent: ApplicationIntent) => Promise<ExecutionResult>;
 export let visible = true;
 export let hideheader = false;
 const page = createControllerPage(), state = page.state;
 page.setQuery({type:"queue"});
 $: entries = $state.items as QueueEntry[];
 onDestroy(page.dispose);
</script>
{#if visible}<button class="queue-backdrop" tabindex="-1" aria-label="Dismiss queue" on:click={toggleQueue}></button><div class="queue-panel" role="dialog" aria-modal="true" tabindex="-1" use:controllerOverlay={{close:toggleQueue}} aria-label="Queue">
 {#if !hideheader}<header><h2>Queue</h2><button on:click={toggleQueue} aria-label="Close queue">×</button></header>{/if}
 <button disabled={!canExecute($controllerState,"queue_clear_upcoming")} on:click={() => execute({type:"queue_clear_upcoming"})}>Clear upcoming</button>
 <ControllerFeedback />
 {#each entries as entry, index (entry.entryId)}
  <div class="queue-row" class:current={entry.entryId === $controllerState.snapshot?.queue.currentEntryId}>
   <div class="art"><ControllerArtwork reference={entry.track.artwork} /></div>
   <button class="track" disabled={!canExecute($controllerState,"queue_play")} on:click={() => execute({type:"queue_play",entryId:entry.entryId})}><strong>{entry.track.title ?? "Untitled"}</strong><small>{entry.track.artist}{entry.entryId === $controllerState.snapshot?.queue.currentEntryId ? " · Now playing" : ""}</small></button>
   <button aria-label={`Move ${entry.track.title ?? "track"} up`} disabled={index === 0 || !canExecute($controllerState,"queue_reorder")} on:click={() => execute({type:"queue_reorder",entryId:entry.entryId,beforeEntryId:entries[index-1].entryId})}>↑</button>
   <button aria-label={`Move ${entry.track.title ?? "track"} to end`} disabled={!canExecute($controllerState,"queue_reorder")} on:click={() => execute({type:"queue_reorder",entryId:entry.entryId,beforeEntryId:null})}>↓</button>
   <button aria-label={`Remove ${entry.track.title ?? "track"}`} disabled={!canExecute($controllerState,"queue_remove")} on:click={() => execute({type:"queue_remove",entryId:entry.entryId})}>×</button>
  </div>
 {/each}
 {#if $state.error}<p role="alert">{$state.error}</p><button on:click={page.refresh}>Refresh queue</button>{/if}
 {#if $state.loading}<p role="status">Loading queue…</p>{:else if !entries.length}<p>Queue is empty</p>{/if}
 {#if $state.hasEarlier}<p>Showing the latest 1,000 loaded entries.</p><button on:click={page.refresh}>Back to first page</button>{/if}
 {#if $state.nextCursor}<button on:click={page.more} disabled={$state.loading}>Load more queue entries</button>{/if}
</div>{/if}
<style>.queue-panel{position:fixed;right:0;top:0;bottom:0;z-index:150;width:min(100%,430px);overflow:auto;background:var(--bg-elevated);border-left:1px solid var(--border-subtle);padding:var(--spacing-lg)}header,.queue-row{display:flex;align-items:center;gap:8px}header{justify-content:space-between}.queue-row{min-height:64px;border-bottom:1px solid var(--border-subtle)}.art{width:40px;height:40px;flex:none;border-radius:var(--radius-sm);overflow:hidden}.track{text-align:left;flex:1;min-width:0}strong,small{display:block;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}small,p{color:var(--text-secondary)}.current .track{color:var(--accent-primary)}button{color:var(--text-primary);background:none;border:0;padding:10px;cursor:pointer}button:disabled{opacity:.4;cursor:not-allowed}
 .queue-backdrop { position:fixed; inset:0; z-index:149; background:#0008; border:0; }
 .queue-panel { padding:calc(16px + var(--safe-area-top)) calc(16px + var(--safe-area-right)) calc(16px + var(--safe-area-bottom)) calc(16px + var(--safe-area-left)); overscroll-behavior:contain; }
 button { min-height:44px; min-width:44px; }
 .track { min-width:0; }
 @media(max-width:767px) { .queue-panel { width:100%; } .queue-row { gap:4px; } .art { width:32px; height:32px; } }
</style>
