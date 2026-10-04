<script lang="ts">
 import { createEventDispatcher } from "svelte";
 import MediaCard from "../MediaCard.svelte";
 import type { ActionAvailability, BrowseContextAction } from "$lib/application/presentation/types";
 export let page = false;
 export let albumMetadata = false;
 export let actions: readonly BrowseContextAction[] = [];
 export let playAvailability: ActionAvailability = {enabled:true};
 export let pauseAvailability: ActionAvailability = {enabled:true};
 export let onPlay: (()=>void) | undefined = undefined;
 export let onPause: (()=>void) | undefined = undefined;
 const dispatch = createEventDispatcher<{play:void;pause:void;click:MouseEvent}>();
 function play() { if(playAvailability.enabled) { onPlay?.(); dispatch("play"); } }
 function pause() { if(pauseAvailability.enabled) { onPause?.(); dispatch("pause"); } }
 function action(row: BrowseContextAction) { if(row.availability.enabled) row.run?.(); }
</script>
{#if page}<div class="playlist-view"><slot /></div>{:else}
<div class="entity-card" class:album-metadata={albumMetadata} class:list-view={$$restProps.layout === "list"}>
 <MediaCard {...$$restProps} on:play={play} on:pause={pause} on:click={(event)=>dispatch('click',event.detail)}>
  <svelte:fragment slot="cover"><slot name="cover" /></svelte:fragment>
  <svelte:fragment slot="extra-info"><slot name="extra-info" /></svelte:fragment>
 </MediaCard>
 {#if actions.length || !playAvailability.enabled}<details class="context-actions" on:click|stopPropagation on:dblclick|stopPropagation><summary aria-label="Entity actions"><svg class="context-menu-icon" viewBox="0 0 24 24" width="20" height="20" fill="currentColor" aria-hidden="true"><circle cx="12" cy="5" r="2"/><circle cx="12" cy="12" r="2"/><circle cx="12" cy="19" r="2"/></svg><span class="visually-hidden">Actions</span></summary><ul>{#if !playAvailability.enabled}<li><button disabled>Play</button><p>{playAvailability.reason}</p></li>{/if}{#each actions as row (row.id)}<li><button disabled={!row.availability.enabled} on:click={()=>action(row)}>{row.label}</button>{#if !row.availability.enabled}<p>{row.availability.reason}</p>{/if}</li>{/each}</ul></details>{/if}
</div>
{/if}
<style>
 .album-metadata:not(.list-view) { --album-metadata-reserve: calc(2.4rem + 54px); }
 .album-metadata:not(.list-view) :global(.media-card .cover) { max-height: calc(100% - var(--album-metadata-reserve)); }
 .album-metadata:not(.list-view) :global(.media-card .info) { flex-shrink: 0; gap: var(--spacing-xs); }
 .album-metadata:not(.list-view) :global(.media-card .text-track), .album-metadata:not(.list-view) :global(.audio-chips) { flex-shrink: 0; }
 .entity-card { position: relative; height: 100%; min-width: 0; }
 .context-actions { position: absolute; top: 8px; right: 8px; max-width: calc(100% - 16px); max-height: calc(100% - 16px); overflow: auto; background: var(--bg-base); border: 1px solid var(--border-subtle); border-radius: var(--radius-sm); z-index: 2; }
 .visually-hidden { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; border: 0; }
 .context-actions:not([open]) { background: transparent; border-color: transparent; }
 summary::-webkit-details-marker { display: none; }
 summary { list-style: none; min-width: 44px; box-sizing: border-box; justify-content: center; cursor: pointer; min-height: 44px; display: flex; align-items: center; padding: 0 8px; color: var(--text-secondary); font-size: .75rem; }
 ul { list-style: none; padding: 8px; margin: 0; }
 button { min-height: 44px; background: none; border: 0; color: var(--text-primary); text-align: left; }
 button:disabled { opacity: .5; }
 p { color: var(--text-secondary); font-size: .75rem; margin: 0 0 8px; }
 summary:focus-visible,button:focus-visible { outline: 2px solid var(--accent-primary); }
 :global {
.playlist-view {
        display: flex;
        flex-direction: column;
        height: 100%;
        padding: var(--spacing-md);
        padding-bottom: 0;
    }
.playlist-view .view-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        margin-bottom: var(--spacing-lg);
        flex-shrink: 0;
    }
.playlist-view .view-header h1 {
        font-size: 2rem;
        font-weight: 700;
    }
.playlist-view .header-actions {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
    }
.playlist-view .create-form {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        margin-bottom: var(--spacing-lg);
        padding: var(--spacing-md);
        background-color: var(--bg-elevated);
        border-radius: var(--radius-md);
        flex-shrink: 0;
    }
.playlist-view .create-form input {
        flex: 1;
        padding: var(--spacing-sm) var(--spacing-md);
        background-color: var(--bg-surface);
        border-radius: var(--radius-sm);
        border: 1px solid var(--border-color);
        color: var(--text-primary);
    }
.playlist-view .create-form input:focus {
        outline: none;
        border-color: var(--accent-primary);
    }
.playlist-view .rename-label {
        font-size: 0.875rem;
        color: var(--text-secondary);
        white-space: nowrap;
        flex-shrink: 0;
    }
@media (max-width: 768px) {.playlist-view {
            padding-bottom: calc(
                var(--mobile-bottom-inset) + var(--spacing-md)
            );
        }
.playlist-view .rename-label {
            display: none;
        }}
 }

.entity-card :global(.placeholder) {
        width: 100%;
        height: 100%;
        display: flex;
        align-items: center;
        justify-content: center;
        color: var(--text-subdued);
        background: linear-gradient(
            135deg,
            var(--bg-surface) 0%,
            var(--bg-highlight) 100%
        );
    }
.entity-card :global(.audio-chips) {
        display: flex;
        flex-wrap: wrap;
        gap: 4px;
        margin-top: 8px;
    }
.entity-card.list-view :global(.audio-chips) {
        flex-wrap: nowrap;
        margin-top: 0;
        overflow: hidden;
    }
.entity-card :global(.audio-chip) {
        display: inline-flex;
        align-items: center;
        font-size: 0.65rem;
        font-weight: 600;
        line-height: 1;
        letter-spacing: 0.03em;
        padding: 3px 7px;
        border-radius: 999px;
        background: var(--bg-highlight);
        color: var(--text-secondary);
        border: 1px solid var(--border-color);
        white-space: nowrap;
    }
.entity-card :global(.audio-chip.format) {
        background: color-mix(in oklab, var(--accent-primary) 15%, transparent);
        color: var(--accent-primary);
        border-color: color-mix(in oklab, var(--accent-primary) 40%, transparent);
    }

</style>
