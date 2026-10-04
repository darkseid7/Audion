<script lang="ts">
 export let title = "Nothing playing";
 export let artist = "Select music from your PC";
 export let badges: readonly string[] = [];
 export let variant: "bar" | "mini" | "full" = "bar";
 export let onOpen: (()=>void) | undefined = undefined;
 export let onTitle: (()=>void) | undefined = undefined;
 export let onArtist: (()=>void) | undefined = undefined;
</script>
<div class="track-info desktop-track-info shared-player-metadata" class:compact={variant === "mini"} class:fullscreen={variant === "full"}>
 <slot>
  <button class="album-art" on:click={onOpen} aria-label={variant === "full" ? "Close now playing" : "Open now playing"}><slot name="artwork" /></button>
  <div class="track-details">{#if onTitle}<button class="track-title truncate metadata-link" on:click={onTitle}>{title}</button>{:else}<span class="track-title truncate">{title}</span>{/if}{#if onArtist}<button class="track-artist truncate metadata-link" on:click={onArtist}>{artist}</button>{:else}<span class="track-artist truncate">{artist}</span>{/if}{#if badges.length}<div class="player-audio-chips">{#each badges as badge,index}<span class="player-audio-chip" class:format={index===0}>{badge}</span>{/each}</div>{/if}</div>
 </slot>
</div>
<style>
 :global {
.shared-player-metadata{
        display: flex;
        align-items: center;
        gap: 12px;
        min-width: 0;
        overflow: hidden;
    }
.shared-player-metadata.desktop-track-info{
        padding-right: var(--spacing-sm);
    }
.shared-player-metadata .album-art{
        width: 96px;
        height: 96px;
        border-radius: var(--radius-md);
        overflow: hidden;
        flex-shrink: 0;
        background-color: var(--bg-surface);
        transition: transform var(--transition-fast);
        cursor: pointer;
    }
.shared-player-metadata .album-art:hover{
        transform: scale(1.05);
    }
.shared-player-metadata .album-art img{
        width: 100%;
        height: 100%;
        object-fit: cover;
    }
.shared-player-metadata .album-art-placeholder{
        width: 100%;
        height: 100%;
        display: flex;
        align-items: center;
        justify-content: center;
        color: var(--text-subdued);
    }
.shared-player-metadata .track-details{
        display: flex;
        flex-direction: column;
        min-width: 0;
    }
.shared-player-metadata .track-title{
        font-size: 1.32rem;
        font-weight: 500;
    }
.shared-player-metadata .track-title:hover{
        color: var(--text-primary);
        text-decoration: underline;
        cursor: pointer;
    }
.shared-player-metadata .track-artist{
        font-size: 1.12rem;
        color: var(--text-secondary);
    }
.shared-player-metadata .track-artist:hover{
        color: var(--text-primary);
        text-decoration: underline;
        cursor: pointer;
    }
.shared-player-metadata .track-album-link{
        color: var(--text-subdued);
        cursor: pointer;
    }
.shared-player-metadata .track-album-link:hover{
        color: var(--text-primary);
        text-decoration: underline;
    }
.shared-player-metadata .player-audio-chips{
        display: flex;
        flex-wrap: wrap;
        gap: 3px;
        margin-top: 4px;
    }
.shared-player-metadata .player-audio-chip{
        display: inline-flex;
        align-items: center;
        font-size: 0.6rem;
        font-weight: 600;
        line-height: 1;
        letter-spacing: 0.03em;
        padding: 2px 6px;
        border-radius: 999px;
        background: var(--bg-highlight);
        color: var(--text-secondary);
        border: 1px solid var(--border-color);
        white-space: nowrap;
    }
.shared-player-metadata .player-audio-chip.format{
        background: color-mix(in oklab, var(--accent-primary) 15%, transparent);
        color: var(--accent-primary);
        border-color: color-mix(in oklab, var(--accent-primary) 40%, transparent);
    }
.shared-player-metadata .no-track{
        color: var(--text-subdued);
        font-size: 0.875rem;
    }
.shared-player-metadata .like-btn{
        background: none;
        border: none;
        color: var(--text-subdued);
        cursor: pointer;
        padding: 4px;
        display: flex;
        align-items: center;
        justify-content: center;
        border-radius: 50%;
        transition: all 0.2s ease;
        flex-shrink: 0;
        margin-left: 8px;
    }
.shared-player-metadata .like-btn:hover{
        color: var(--text-primary);
        transform: scale(1.15);
    }
.shared-player-metadata .like-btn.liked{
        color: var(--accent-primary, #1db954);
    }
.shared-player-metadata .like-btn.liked:hover{
        transform: scale(1.15);
    }

 }
 .shared-player-metadata { width: 100%; }
 .shared-player-metadata .metadata-link { background: none; border: 0; padding: 0; margin: 0; min-height: 0; font-family: inherit; text-align: inherit; cursor: pointer; }
 .shared-player-metadata .metadata-link:focus-visible { outline: 2px solid var(--accent-primary); outline-offset: 2px; border-radius: var(--radius-sm); }
 .shared-player-metadata .track-title.metadata-link { color: var(--text-primary); }
 .shared-player-metadata button.album-art { padding: 0; border: 0; color: var(--text-primary); }
 .shared-player-metadata.fullscreen { justify-content: center; flex-direction: column; overflow: visible; text-align: center; }
 .shared-player-metadata.fullscreen :global(.album-art) { width: min(280px,65vw); height: min(280px,65vw); }
 .shared-player-metadata.fullscreen :global(.player-audio-chips) { justify-content: center; }
 @media(width < 768px) { .shared-player-metadata:not(.fullscreen) :global(.album-art) { width: 48px; height: 48px; } .shared-player-metadata:not(.fullscreen) :global(.track-title) { font-size: .9rem; } .shared-player-metadata:not(.fullscreen) :global(.track-artist) { font-size: .75rem; } }
</style>
