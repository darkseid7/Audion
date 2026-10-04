<script lang="ts">
 export let kind: "album" | "artist" | "playlist" | "liked" = "album";
 export let className = "";
 export let compact = false;
 export let role: "banner" | "region" = "banner";
 export let ariaLabel: string | undefined = undefined;
 export let onContextMenu: ((event: MouseEvent)=>void) | undefined = undefined;
</script>
<header class={kind+"-header "+className+(compact ? " controller-compact" : "")} {...$$restProps} {role} aria-label={ariaLabel} on:contextmenu={onContextMenu}><slot /></header>
<slot name="controls" />
<style>
 :global {
.album-header {
        display: flex;
        gap: var(--spacing-lg);
        padding: var(--spacing-lg);
        background: linear-gradient(
            180deg,
            var(--bg-surface) 0%,
            var(--bg-base) 100%
        );
    }
.album-header .back-btn {
        position: absolute;
        top: var(--spacing-md);
        left: var(--spacing-md);
        width: 32px;
        height: 32px;
        border-radius: var(--radius-full);
        background-color: rgba(0, 0, 0, 0.5);
        color: var(--text-primary);
        display: flex;
        align-items: center;
        justify-content: center;
        transition: all var(--transition-fast);
    }
.album-header .back-btn:hover {
        background-color: rgba(0, 0, 0, 0.7);
        transform: scale(1.1);
    }
.album-header .album-cover {
        width: 232px;
        height: 232px;
        border-radius: var(--radius-sm);
        overflow: hidden;
        flex-shrink: 0;
        box-shadow: var(--shadow-lg);
    }
.album-header .album-cover img {
        width: 100%;
        height: 100%;
        object-fit: cover;
    }
.album-header .album-cover img.clickable {
        cursor: pointer;
    }
.album-header .album-cover-placeholder {
        width: 100%;
        height: 100%;
        display: flex;
        align-items: center;
        justify-content: center;
        background: linear-gradient(
            135deg,
            var(--bg-surface) 0%,
            var(--bg-highlight) 100%
        );
        color: var(--text-subdued);
    }
.album-header .album-info {
        display: flex;
        flex-direction: column;
        justify-content: flex-end;
        min-width: 0;
    }
.album-header .album-type {
        font-size: 0.75rem;
        font-weight: 600;
        text-transform: uppercase;
        color: var(--text-primary);
    }
.album-header .album-title {
        font-size: 3rem;
        font-weight: 700;
        line-height: 1.1;
        margin: var(--spacing-sm) 0;
        color: var(--text-primary);
    }
.album-header .album-meta {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        font-size: 0.875rem;
        color: var(--text-secondary);
        margin-bottom: var(--spacing-lg);
    }
.album-header .album-audio-meta {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        flex-wrap: wrap;
    }
.album-header .album-audio-chip {
        display: inline-flex;
        align-items: center;
        padding: 2px 8px;
        border-radius: var(--radius-full);
        border: 1px solid var(--border-color);
        background: var(--bg-highlight);
        color: var(--text-secondary);
        font-size: 0.72rem;
        font-weight: 700;
        line-height: 1;
        white-space: nowrap;
    }
.album-header .album-audio-chip.format-chip {
        color: var(--accent-primary);
        border-color: color-mix(in srgb, var(--accent-primary), transparent 65%);
        background: color-mix(in srgb, var(--accent-primary), transparent 88%);
    }
.album-header .album-artist {
        font-weight: 600;
        color: var(--text-primary);
        background: none;
        border: none;
        padding: 0;
        cursor: pointer;
    }
.album-header .album-artist:hover {
        text-decoration: underline;
    }
.album-header .separator {
        color: var(--text-subdued);
    }
.album-header .album-edition-display {
        color: var(--text-subdued);
        font-size: 0.85em;
        margin-left: 4px;
    }
.album-header .album-actions {
        display: flex;
        gap: var(--spacing-md);
        align-items: center;
    }
.album-header .btn-info {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 40px;
        height: 40px;
        border-radius: 50%;
        background: var(--bg-surface, #282828);
        color: var(--text-secondary, #b3b3b3);
        border: 0;
        cursor: pointer;
        transition: background 0.15s, color 0.15s, transform 0.1s;
        flex-shrink: 0;
    }
.album-header .btn-info:hover {
        background: var(--bg-highlight, #3e3e3e);
        color: var(--accent-primary, #1DB954);
    }
.album-header .btn-info:active { transform: scale(0.92); }
.album-header .btn-like-album {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 40px;
        height: 40px;
        border-radius: 50%;
        border: 1px solid var(--border-color);
        background: transparent;
        color: var(--text-subdued);
        cursor: pointer;
        transition: all var(--transition-fast);
    }
.album-header .btn-like-album:hover {
        color: var(--accent-primary);
        border-color: var(--accent-primary);
    }
.album-header .btn-like-album.liked {
        color: var(--accent-primary);
        border-color: var(--accent-primary);
    }
.album-header .btn-filter-liked {
        display: flex;
        align-items: center;
        justify-content: center;
        gap: 4px;
        height: 36px;
        padding: 0 12px;
        border-radius: var(--radius-full);
        border: 1px solid var(--border-color);
        background: transparent;
        color: var(--text-subdued);
        cursor: pointer;
        font-size: 0.8rem;
        font-weight: 600;
        transition: all var(--transition-fast);
    }
.album-header .btn-filter-liked:hover {
        color: var(--accent-primary);
        border-color: var(--accent-primary);
    }
.album-header .btn-filter-liked.active {
        color: var(--accent-primary);
        border-color: var(--accent-primary);
        background: color-mix(in srgb, var(--accent-primary), transparent 88%);
    }
.album-header .play-all-btn {
        font-size: 1rem;
        padding: var(--spacing-sm) var(--spacing-xl);
    }
.album-header .btn-secondary {
        background-color: transparent;
        border: 1px solid var(--border-color);
        color: var(--text-primary);
        font-weight: 600;
        cursor: pointer;
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        transition: all var(--transition-fast);
        padding: var(--spacing-sm) var(--spacing-xl);
        border-radius: var(--radius-full);
        font-size: 1rem;
    }
.album-header .btn-secondary:hover:not(:disabled) {
        border-color: var(--text-primary);
        transform: scale(1.05);
    }
.album-header .btn-secondary.downloaded {
        border-color: var(--accent-primary);
        color: var(--accent-primary);
        cursor: default;
    }
.album-header .btn-secondary.downloaded:hover {
        transform: none;
    }
.album-header .btn-secondary:disabled {
        opacity: 0.7;
        cursor: not-allowed;
    }
.album-header .spinner-sm {
        width: 16px;
        height: 16px;
        border: 2px solid var(--bg-highlight);
        border-top-color: var(--text-primary);
        border-radius: 50%;
        animation: spin 1s linear infinite;
    }
@media (max-width: 768px) {.album-header {
            flex-direction: column;
            align-items: center;
            text-align: center;
            padding: calc(var(--safe-area-top) + var(--spacing-md))
                var(--spacing-md) var(--spacing-md);
            gap: var(--spacing-md);
        }
.album-header .back-btn {
            top: calc(var(--safe-area-top) + var(--spacing-sm));
            left: var(--spacing-sm);
        }
.album-header .album-cover {
            width: 160px;
            height: 160px;
        }
.album-header .album-info {
            align-items: center;
        }
.album-header .album-title {
            font-size: 1.5rem;
            word-break: break-word;
        }
.album-header .album-meta {
            flex-wrap: wrap;
            justify-content: center;
            margin-bottom: var(--spacing-md);
        }
.album-header .album-actions {
            flex-wrap: wrap;
            justify-content: center;
        }
.album-header .play-all-btn,
.album-header .btn-secondary {
            padding: var(--spacing-sm) var(--spacing-lg);
            font-size: 0.875rem;
            min-height: 44px;
        }
}
.artist-header {
        display: flex;
        gap: var(--spacing-lg);
        padding: var(--spacing-lg);
        background: linear-gradient(
            180deg,
            var(--bg-surface) 0%,
            var(--bg-base) 100%
        );
        position: relative;
    }
.artist-header .back-btn {
        position: absolute;
        top: var(--spacing-md);
        left: var(--spacing-md);
        width: 32px;
        height: 32px;
        border-radius: var(--radius-full);
        background-color: rgba(0, 0, 0, 0.5);
        color: var(--text-primary);
        display: flex;
        align-items: center;
        justify-content: center;
        transition: all var(--transition-fast);
    }
.artist-header .back-btn:hover {
        background-color: rgba(0, 0, 0, 0.7);
        transform: scale(1.1);
    }
.artist-header .artist-avatar {
        width: 200px;
        height: 200px;
        border-radius: var(--radius-full);
        background: linear-gradient(
            135deg,
            var(--accent-primary) 0%,
            #1a1a1a 100%
        );
        display: flex;
        align-items: center;
        justify-content: center;
        flex-shrink: 0;
        box-shadow: var(--shadow-lg);
    }
.artist-header .artist-initial {
        font-size: 4rem;
        font-weight: 700;
        color: var(--text-primary);
    }
.artist-header .artist-picture {
        width: 100%;
        height: 100%;
        object-fit: cover;
        border-radius: var(--radius-full);
    }
.artist-header .artist-info {
        display: flex;
        flex-direction: column;
        justify-content: flex-end;
        min-width: 0;
    }
.artist-header .artist-type {
        font-size: 0.75rem;
        font-weight: 600;
        text-transform: uppercase;
        color: var(--text-primary);
    }
.artist-header .artist-name {
        font-size: 3rem;
        font-weight: 700;
        line-height: 1.1;
        margin: var(--spacing-sm) 0;
        color: var(--text-primary);
    }
.artist-header .artist-meta {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        font-size: 0.875rem;
        color: var(--text-secondary);
        margin-bottom: var(--spacing-lg);
    }
.artist-header .separator {
        color: var(--text-subdued);
    }
.artist-header .artist-actions {
        display: flex;
        gap: var(--spacing-md);
    }
.artist-header .play-all-btn {
        font-size: 1rem;
        padding: var(--spacing-sm) var(--spacing-xl);
    }
.artist-header .btn-secondary {
        background-color: transparent;
        border: 1px solid var(--border-color);
        color: var(--text-primary);
        font-weight: 600;
        cursor: pointer;
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        transition: all var(--transition-fast);
        padding: var(--spacing-sm) var(--spacing-xl);
        border-radius: var(--radius-full);
        font-size: 1rem;
    }
.artist-header .btn-secondary:hover:not(:disabled) {
        border-color: var(--text-primary);
        transform: scale(1.05);
    }
.artist-header .btn-secondary:disabled {
        opacity: 0.7;
        cursor: not-allowed;
    }
.artist-header .spinner-sm {
        width: 16px;
        height: 16px;
        border: 2px solid var(--bg-highlight);
        border-top-color: var(--text-primary);
        border-radius: 50%;
        animation: spin 1s linear infinite;
    }
@media (max-width: 768px) {.artist-header {
            flex-direction: column;
            align-items: center;
            text-align: center;
            padding: calc(var(--safe-area-top) + var(--spacing-md))
                var(--spacing-md) var(--spacing-md);
            gap: var(--spacing-md);
        }
.artist-header .back-btn {
            top: calc(var(--safe-area-top) + var(--spacing-sm));
            left: var(--spacing-sm);
        }
.artist-header .artist-avatar {
            width: 120px;
            height: 120px;
        }
.artist-header .artist-initial {
            font-size: 2.5rem;
        }
.artist-header .artist-info {
            align-items: center;
        }
.artist-header .artist-name {
            font-size: 1.5rem;
            word-break: break-word;
        }
.artist-header .artist-meta {
            flex-wrap: wrap;
            justify-content: center;
            margin-bottom: var(--spacing-md);
        }
.artist-header .artist-actions {
            flex-wrap: wrap;
            justify-content: center;
        }
.artist-header .play-all-btn,
.artist-header .btn-secondary {
            padding: var(--spacing-sm) var(--spacing-lg);
            font-size: 0.875rem;
            min-height: 44px;
        }
}
.playlist-header {
        display: flex;
        gap: var(--spacing-lg);
        padding: var(--spacing-lg);
        background: linear-gradient(
            180deg,
            var(--bg-surface) 0%,
            var(--bg-base) 100%
        );
        position: relative;
    }
.playlist-header .back-btn {
        position: absolute;
        top: var(--spacing-md);
        right: var(--spacing-md);
        width: 40px;
        height: 40px;
        border-radius: var(--radius-full);
        background-color: rgba(0, 0, 0, 0.5);
        backdrop-filter: blur(8px);
        color: var(--text-primary);
        display: flex;
        align-items: center;
        justify-content: center;
        transition: all var(--transition-fast);
        z-index: 10;
        border: 1px solid rgba(255, 255, 255, 0.1);
    }
.playlist-header .back-btn::after {
        content: attr(title);
        position: absolute;
        bottom: 100%;
        left: 50%;
        transform: translateX(-50%);
        padding: 6px 12px;
        background-color: var(--bg-surface);
        color: var(--text-primary);
        font-size: 0.75rem;
        border-radius: var(--radius-sm);
        white-space: nowrap;
        opacity: 0;
        pointer-events: none;
        transition: opacity var(--transition-fast);
        margin-bottom: 8px;
        box-shadow: var(--shadow-md);
    }
.playlist-header .back-btn:hover::after {
        opacity: 1;
    }
.playlist-header .back-btn:hover {
        background-color: rgba(220, 38, 38, 0.9);
        border-color: rgba(220, 38, 38, 0.4);
        transform: scale(1.05);
    }
.playlist-header .back-btn:active {
        transform: scale(0.95);
    }
.playlist-header .playlist-cover {
        width: 232px;
        height: 232px;
        border-radius: var(--radius-sm);
        background: linear-gradient(
            135deg,
            var(--bg-highlight) 0%,
            var(--bg-surface) 100%
        );
        display: flex;
        align-items: center;
        justify-content: center;
        flex-shrink: 0;
        color: var(--text-subdued);
        box-shadow: var(--shadow-lg);
        position: relative;
        overflow: hidden;
        cursor: pointer;
    }
.playlist-header .cover-image {
        width: 100%;
        height: 100%;
        object-fit: cover;
        display: block;
        transition: transform 0.3s ease;
    }
.playlist-header .playlist-cover:hover .cover-image {
        transform: scale(1.05);
    }
.playlist-header .cover-overlay {
        position: absolute;
        inset: 0;
        background: linear-gradient(
            to top,
            rgba(0, 0, 0, 0.7) 0%,
            rgba(0, 0, 0, 0.3) 50%,
            rgba(0, 0, 0, 0) 100%
        );
        display: flex;
        align-items: flex-end;
        justify-content: space-between;
        padding: var(--spacing-sm);
        opacity: 0;
        animation: fadeIn 0.2s ease forwards;
    }
.playlist-header .cover-overlay-btn {
        width: 36px;
        height: 36px;
        border-radius: var(--radius-full);
        background-color: rgba(0, 0, 0, 0.6);
        backdrop-filter: blur(8px);
        display: flex;
        align-items: center;
        justify-content: center;
        color: white;
        transition: all var(--transition-fast);
        border: 1px solid rgba(255, 255, 255, 0.1);
    }
.playlist-header .cover-overlay-btn:hover {
        background-color: rgba(0, 0, 0, 0.8);
        transform: scale(1.1);
        border-color: rgba(255, 255, 255, 0.2);
    }
.playlist-header .cover-delete-btn:hover {
        background-color: rgba(241, 94, 108, 0.8);
        border-color: rgba(241, 94, 108, 0.4);
    }
.playlist-header .cover-add-btn:hover {
        background-color: var(--accent-primary);
        border-color: var(--accent-primary);
    }
.playlist-header .playlist-info {
        display: flex;
        flex-direction: column;
        justify-content: flex-end;
        min-width: 0;
    }
.playlist-header .playlist-type {
        font-size: 0.75rem;
        font-weight: 600;
        text-transform: uppercase;
        color: var(--text-primary);
    }
.playlist-header .playlist-title {
        font-size: 3rem;
        font-weight: 700;
        line-height: 1.1;
        margin: var(--spacing-sm) 0;
        color: var(--text-primary);
        cursor: text;
    }
.playlist-header .edit-input {
        font-size: 3rem;
        font-weight: 700;
        background-color: var(--bg-surface);
        border: 2px solid var(--accent-primary);
        border-radius: var(--radius-sm);
        padding: var(--spacing-xs) var(--spacing-sm);
        margin: var(--spacing-sm) 0;
        width: 100%;
    }
.playlist-header .playlist-meta {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        font-size: 0.875rem;
        color: var(--text-secondary);
        margin-bottom: var(--spacing-lg);
    }
.playlist-header .separator {
        color: var(--text-subdued);
    }
.playlist-header .playlist-actions {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
    }
.playlist-header .play-all-btn,
.playlist-header .add-songs-btn {
        font-size: 1rem;
        padding: var(--spacing-sm) var(--spacing-xl);
    }
.playlist-header .btn-secondary.downloaded {
        border-color: var(--accent-primary);
        color: var(--accent-primary);
        cursor: default;
    }
.playlist-header .btn-secondary.downloaded:hover {
        transform: none;
    }
.playlist-header .spinner-sm {
        width: 16px;
        height: 16px;
        border: 2px solid var(--bg-highlight);
        border-top-color: var(--text-primary);
        border-radius: 50%;
        animation: spin 1s linear infinite;
    }
@media (max-width: 768px) {.playlist-header {
            flex-direction: column;
            align-items: center;
            text-align: center;
            padding: calc(var(--safe-area-top) + var(--spacing-md))
                var(--spacing-md) var(--spacing-md);
            gap: var(--spacing-md);
        }
.playlist-header .back-btn {
            top: calc(var(--safe-area-top) + var(--spacing-sm));
            right: var(--spacing-sm);
        }
.playlist-header .playlist-cover {
            width: 160px;
            height: 160px;
        }
.playlist-header .playlist-info {
            align-items: center;
        }
.playlist-header .playlist-title {
            font-size: 1.5rem;
            word-break: break-word;
        }
.playlist-header .edit-input {
            font-size: 1.5rem;
        }
.playlist-header .playlist-meta {
            flex-wrap: wrap;
            justify-content: center;
            margin-bottom: var(--spacing-md);
        }
.playlist-header .playlist-actions {
            flex-wrap: wrap;
            justify-content: center;
            gap: var(--spacing-sm);
        }
.playlist-header .play-all-btn,
.playlist-header .add-songs-btn,
.playlist-header .btn-secondary {
            padding: var(--spacing-sm) var(--spacing-lg);
            font-size: 0.875rem;
            min-height: 44px;
        }
.playlist-header .icon-btn {
            min-width: 44px;
            min-height: 44px;
        }
}


.liked-header {
        display: flex;
        flex-direction: column; /* Center alignment stack */
        align-items: center;
        text-align: center;
        gap: var(--spacing-md);
        padding: 30px var(--spacing-xl) 15px;
        background: linear-gradient(
            180deg,
            color-mix(in srgb, var(--accent-primary) 20%, transparent) 0%,
            color-mix(in srgb, var(--accent-primary) 5%, transparent) 50%,
            var(--bg-base) 100%
        );
        flex-shrink: 0;
        transition: all 0.4s cubic-bezier(0.4, 0, 0.2, 1);
        transform-origin: center top;
        z-index: 10;
        pointer-events: none;
    }
.liked-header.is-small {
        padding-top: 15px;
        padding-bottom: 5px;
        pointer-events: none;
    }
.liked-header .liked-gradient-bg {
        width: 130px;
        height: 130px;
        background: linear-gradient(135deg, #450af5, #c4efd9);
        border-radius: var(--radius-md);
        display: flex;
        align-items: center;
        justify-content: center;
        flex-shrink: 0;
        box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4);
        color: white;
        transition: transform var(--transition-normal);
    }
.liked-header-info {
        display: flex;
        flex-direction: column;
        align-items: center; /* CENTERED */
        gap: var(--spacing-xs);
        padding-bottom: var(--spacing-sm);
    }
.liked-header .liked-label {
        font-size: 0.75rem;
        font-weight: 700;
        text-transform: uppercase;
        letter-spacing: 0.1em;
        color: var(--text-secondary);
    }
.liked-header .liked-title {
        font-size: 2.8rem;
        font-weight: 900;
        color: var(--text-primary);
        margin: 0;
        line-height: 1;
        letter-spacing: -0.02em;
    }
.liked-header .liked-count-container {
        display: flex;
        justify-content: center;
        width: 100%;
    }
.liked-header .liked-count {
        font-size: 0.9rem;
        font-weight: 500;
        color: var(--text-primary);
        margin-top: var(--spacing-sm);
        display: flex;
        align-items: center;
        gap: 4px;
    }
.liked-header .liked-count::before {
        content: "•";
        color: var(--text-secondary);
    }
@media (max-width: 768px) {.liked-header {
            flex-direction: column;
            align-items: center;
            text-align: center;
            padding: calc(var(--safe-area-top) + var(--spacing-md))
                var(--spacing-md) var(--spacing-md);
        }
.liked-header .liked-gradient-bg {
            width: 120px;
            height: 120px;
        }
.liked-header .liked-title {
            font-size: 2rem;
        }
}

.liked-controls {
        padding: var(--spacing-lg) var(--spacing-xl);
        background-color: var(--bg-base);
        flex-shrink: 0;
        display: flex;
        justify-content: center; /* CENTERED */
    }
.liked-controls .controls-wrapper {
        display: flex;
        align-items: center;
        gap: var(--spacing-lg);
        width: 100%;
        max-width: 1200px;
        justify-content: center;
    }
.liked-controls .play-all-btn {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        gap: var(--spacing-md);
        height: 56px;
        min-width: 170px;
        padding: 0 32px 0 8px;
        background-color: var(--accent-primary);
        color: white;
        font-weight: 700;
        border: none;
        border-radius: var(--radius-full);
        font-size: 1.05rem;
        cursor: pointer;
        transition: all var(--transition-normal);
        box-shadow: 0 4px 12px rgba(0, 0, 0, 0.2);
    }
.liked-controls .btn-icon {
        width: 42px;
        height: 42px;
        border-radius: 50%;
        background: rgba(255, 255, 255, 0.1);
        display: flex;
        align-items: center;
        justify-content: center;
        transition: transform var(--transition-fast);
    }
.liked-controls .play-all-btn:hover:not(:disabled) {
        background-color: var(--accent-hover);
        transform: scale(1.05);
        box-shadow: 0 6px 16px rgba(0, 0, 0, 0.3);
    }
.liked-controls .play-all-btn:hover .btn-icon {
        transform: scale(1.1);
        background: rgba(255, 255, 255, 0.2);
    }
.liked-controls .shuffle-btn {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        gap: var(--spacing-sm);
        height: 56px;
        min-width: 140px;
        padding: 0 28px;
        background-color: transparent;
        color: var(--text-primary);
        font-weight: 600;
        border: 1px solid var(--border-subtle);
        border-radius: var(--radius-full);
        font-size: 1rem;
        cursor: pointer;
        transition: all var(--transition-normal);
    }
.liked-controls .shuffle-btn:hover:not(:disabled) {
        border-color: var(--text-primary);
        background: rgba(255, 255, 255, 0.05);
        transform: translateY(-1px);
    }
.liked-controls .play-all-btn:disabled,
.liked-controls .shuffle-btn:disabled {
        opacity: 0.3;
        cursor: not-allowed;
        filter: grayscale(1);
    }

 }
 :global(.detail-actions-menu) { color: var(--text-secondary); font-size: .8rem; }
 :global(.detail-actions-menu summary) { min-height: 44px; cursor: pointer; }
 :global(.album-header.controller-compact) { padding: 16px; gap: 24px; position: relative; }
 :global(.album-header.controller-compact .album-cover) { width: 168px; height: 168px; }
 :global(.album-header.controller-compact .album-title) { font-size: 2rem; }
 :global(.album-header.controller-compact .album-meta) { flex-wrap: wrap; margin-bottom: 12px; }
 :global(.album-header.controller-compact .album-actions) { flex-wrap: wrap; gap: 12px; }
 @media (max-width: 768px) {
   :global(.album-header.controller-compact) { gap: 12px; }
   :global(.album-header.controller-compact .album-cover) { width: 128px; height: 128px; }
   :global(.album-header.controller-compact .album-title) { font-size: 1.5rem; }
 }
</style>
