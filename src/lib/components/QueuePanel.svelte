<script lang="ts">
    import { fade, fly } from "svelte/transition";
    import { isQueueVisible, toggleQueue } from "$lib/stores/ui";
    import { isMobile } from "$lib/stores/mobile";
    import {
        queue,
        queueIndex,
        currentTrack,
        isPlaying,
        playFromQueue,
        removeFromQueue,
        clearUpcoming,
        reorderQueue,
        userQueueCount,
        shuffle,
        shuffledIndices,
        shuffledIndex,
    } from "$lib/stores/player";
    import { albums } from "$lib/stores/library";
    import { formatDuration, getAlbumArtSrc, getTrackCoverSrc, getAlbumCoverSrc } from "$lib/api/tauri";
    import { onMount, onDestroy } from "svelte";
    import { dndzone, dragHandle, TRIGGERS } from "svelte-dnd-action";
    
    export let hideheader: boolean = false;
    export let forceVisible: boolean = false;

    import type { Track } from "$lib/api/tauri";

    // Virtual scrolling configuration
    const TRACK_ROW_HEIGHT = 56; // pixels
    const OVERSCAN = 5; // Extra rows to render above/below viewport

    let historyContainerHeight = 300;
    let historyScrollTop = 0;
    let historyContainerElement: HTMLDivElement;
    let isAndroid = false;

    // Create album map for art lookup
    $: albumMap = new Map($albums.map((a) => [a.id, a]));

    function getTrackArt(track: Track): string | null {
        if (!track) return null;
        
        // Priority 1: Track's file-based cover
        if (track.track_cover_path) return getTrackCoverSrc(track);
        
        // Priority 2: Track's base64 cover - old
        if (track.track_cover) return getAlbumArtSrc(track.track_cover);
        
        // Priority 3: Streaming cover URL
        if (track.cover_url) return track.cover_url;
        
        // Priority 4 & 5: Album art (file-based or base64)
        if (!track.album_id) return null;
        const album = albumMap.get(track.album_id);
        if (!album) return null;
        
        // Priority 4: Album's file-based art
        if (album.art_path) return getAlbumCoverSrc(album);
        
        // Priority 5: Album's base64 art - old
        return album.art_data ? getAlbumArtSrc(album.art_data) : null;
    }

    // Derived queue state for display
    let historyTracks: Array<{ track: Track; index: number }> = [];
    let upcomingTracks: Array<{
        track: Track;
        index: number;
        isPriority: boolean;
    }> = [];

    // Simple reactive statement to rebuild lists when any dependency changes
    $: {
        const q = $queue;
        const qIdx = $queueIndex;
        const uCount = $userQueueCount;
        const isShuffle = $shuffle;
        const sIndices = $shuffledIndices;
        const sIdx = $shuffledIndex;

        // History: always what's before current in absolute playback history?
        // Or based on mode?
        // Traditionally, history is just "what played before".
        // In shuffle mode, "Previous" button goes back in shuffle history.
        // So we should show shuffle history?

        if (isShuffle) {
            historyTracks = sIndices
                .slice(0, sIdx)
                .map((idx) => ({ track: q[idx], index: idx }));
        } else {
            historyTracks = q
                .slice(0, qIdx)
                .map((t, i) => ({ track: t, index: i }));
        }

        // Upcoming
        if (isShuffle) {
            upcomingTracks = [];

            // 1. Priority Tracks (User Queue)
            // These are strictly q[qIdx+1 ... qIdx+uCount]
            for (let i = 1; i <= uCount; i++) {
                const idx = qIdx + i;
                if (idx < q.length) {
                    upcomingTracks.push({
                        track: q[idx],
                        index: idx,
                        isPriority: true,
                    });
                }
            }

            // 2. Shuffled Tracks
            // Start from sIdx + 1
            const remainingShuffled = sIndices.slice(sIdx + 1);

            // We need to filter out tracks that are ALREADY in Priority list.
            // Priority indices are [qIdx+1 ... qIdx+uCount].
            const priorityIndices = new Set<number>();
            for (let i = 1; i <= uCount; i++) priorityIndices.add(qIdx + i);

            for (const idx of remainingShuffled) {
                if (!priorityIndices.has(idx)) {
                    upcomingTracks.push({
                        track: q[idx],
                        index: idx,
                        isPriority: false,
                    });
                }
            }
        } else {
            // Linear Upcoming
            upcomingTracks = q.slice(qIdx + 1).map((t, i) => ({
                track: t,
                index: qIdx + 1 + i,
                isPriority: i < uCount,
            }));
        }
    }

    $: hasUpcoming = upcomingTracks.length > 0;

    // Virtual scroll state for history tracks
    let historyVirtualState = {
        totalHeight: 0,
        startIndex: 0,
        endIndex: 0,
        offsetY: 0,
        visibleTracks: [] as typeof historyTracks,
    };

    $: {
        const totalHeight = historyTracks.length * TRACK_ROW_HEIGHT;
        const startIndex = Math.max(0, Math.floor(historyScrollTop / TRACK_ROW_HEIGHT) - OVERSCAN);
        const endIndex = Math.min(
            historyTracks.length,
            Math.ceil((historyScrollTop + historyContainerHeight) / TRACK_ROW_HEIGHT) + OVERSCAN
        );
        const visibleTracks = historyTracks.slice(startIndex, endIndex);
        const offsetY = startIndex * TRACK_ROW_HEIGHT;
        historyVirtualState = { totalHeight, startIndex, endIndex, offsetY, visibleTracks };
    }

    function handleHistoryScroll(e: Event) {
        historyScrollTop = (e.target as HTMLElement).scrollTop;
    }

    onMount(() => {
        isAndroid =
            typeof navigator !== 'undefined' && /android/i.test(navigator.userAgent);

        const observers: ResizeObserver[] = [];
        
        // Set up ResizeObserver for history container
        if (historyContainerElement) {
            const updateHistoryHeight = () => {
                historyContainerHeight = historyContainerElement.clientHeight;
            };
            updateHistoryHeight();
            const historyObserver = new ResizeObserver(updateHistoryHeight);
            historyObserver.observe(historyContainerElement);
            observers.push(historyObserver);
        }
        
        return () => {
            observers.forEach(observer => observer.disconnect());
        };
    });

    function handlePlayTrack(index: number) {
        playFromQueue(index);
    }

    function handleRemove(index: number) {
        removeFromQueue(index);
    }

    // ── svelte-dnd-action integration ──
    // The library requires a local mutable items array with a top-level
    // `id` property. upcomingTracks has `track.id` nested — we map it.
    $: dndItems = upcomingTracks.map((t) => ({ ...t, id: t.track.id }));

    // Track the dragged item's absolute queue index at drag start so we
    // can call reorderQueue(from, to) with correct indices on finalize.
    let dragStartIndex: number | null = null;

    // Track drag-active state and the original DOM position of the dragged
    // clone so we can clamp the library's transform to stay within the
    // queue panel. Without this, the clone floats freely across the app.
    let dragActive = false;
    let dragOriginalRect: { top: number; left: number; width: number; height: number } | null = null;

    const FLIP_MS = 180;
    const DRAGGED_EL_ID = "dnd-action-dragged-el";

    function handleConsider(
        e: CustomEvent<{
            items: typeof dndItems;
            info: { trigger: TRIGGERS; id?: string };
        }>,
    ) {
        const info = e.detail.info;
        // Capture the absolute queue index when drag starts.
        if (info.trigger === TRIGGERS.DRAG_STARTED && dragStartIndex === null && info.id) {
            const item = upcomingTracks.find((t) => String(t.track.id) === info.id);
            if (item) dragStartIndex = item.index;

            // svelte-dnd-action has now appended its `position: fixed`
            // clone to document.body. Grab its initial rect and start
            // clamping. Registering the mousemove listener here (rather
            // than at mount) ensures it runs AFTER the library's own
            // mousemove handler (which was registered earlier in the
            // drag-start sequence), so we read and clamp the freshly
            // updated transform on every frame.
            const draggedEl = document.getElementById(DRAGGED_EL_ID);
            if (draggedEl) {
                dragOriginalRect = {
                    top: parseFloat(draggedEl.style.top),
                    left: parseFloat(draggedEl.style.left),
                    width: draggedEl.offsetWidth,
                    height: draggedEl.offsetHeight,
                };
                dragActive = true;
                window.addEventListener("mousemove", clampDraggedToPanel);
                window.addEventListener("touchmove", clampDraggedToPanel);
            }
        }
        dndItems = e.detail.items;
    }

    function handleFinalize(
        e: CustomEvent<{
            items: typeof dndItems;
            info: { trigger: TRIGGERS; id?: string };
        }>,
    ) {
        dndItems = e.detail.items;
        const info = e.detail.info;

        // Sync the actual queue store with the new visual order.
        if (dragStartIndex !== null && info.id) {
            const qIdx = $queueIndex;
            // Find the dragged item in the reordered array.
            const newVisualPos = dndItems.findIndex(
                (it) => String(it.track.id) === info.id,
            );
            if (newVisualPos !== -1) {
                // In linear mode upcomingTracks[0] = queue[qIdx+1], so
                // new visual position maps to qIdx + 1 + newVisualPos.
                const newQueueIndex = qIdx + 1 + newVisualPos;
                if (dragStartIndex !== newQueueIndex) {
                    reorderQueue(dragStartIndex, newQueueIndex);
                }
            }
        }

        // Clean up clamp listener and state.
        if (dragActive) {
            window.removeEventListener("mousemove", clampDraggedToPanel);
            window.removeEventListener("touchmove", clampDraggedToPanel);
            dragActive = false;
            dragOriginalRect = null;
        }
        dragStartIndex = null;
    }

    // Panel ref for bounding the drag.
    let queuePanelElement: HTMLElement;

    // Lock the library's floating drag clone to vertical-only motion within
    // the queue column. The library sets `transform: translate3d(dx, dy, 0)`
    // from cursor delta; we override it so X is always 0 (no horizontal
    // drift across the app) and Y is clamped to the panel bounds.
    function clampDraggedToPanel(e: MouseEvent | TouchEvent) {
        if (!dragActive || !queuePanelElement || !dragOriginalRect) return;
        const draggedEl = document.getElementById(DRAGGED_EL_ID);
        if (!draggedEl) return;

        const panelRect = queuePanelElement.getBoundingClientRect();
        const match = draggedEl.style.transform.match(
            /translate3d\(([-\d.]+)px,\s*([-\d.]+)px/,
        );
        if (!match) return;

        const ty = parseFloat(match[2]);

        // X is always 0 — drag is vertical only.
        const lockedTx = 0;

        // Y is clamped to panel bounds so the clone never escapes vertically.
        const effTop = dragOriginalRect.top + ty;
        const effBottom = effTop + dragOriginalRect.height;
        let dy = 0;
        if (effTop < panelRect.top) dy = panelRect.top - effTop;
        else if (effBottom > panelRect.bottom) dy = panelRect.bottom - effBottom;

        const finalTy = ty + dy;

        // Always rewrite the transform: locks X to 0 and applies Y clamping.
        // We rewrite every frame (even when dy is 0) because the library sets
        // X based on cursor delta and we need to override that consistently.
        draggedEl.style.transform = `translate3d(${lockedTx}px, ${finalTy}px, 0)`;
    }
</script>

{#if $isQueueVisible || forceVisible}
    <aside
        class="queue-panel"
        class:mobile={$isMobile}
        class:android-lite={isAndroid && $isMobile}
        bind:this={queuePanelElement}
        transition:fly={{
            x: $isMobile ? 0 : 300,
            y: $isMobile ? (isAndroid ? 0 : 100) : 0,
            duration: $isMobile && isAndroid ? 180 : 300,
        }}
    >
    {#if !hideheader}
        <header class="queue-header">
            <h3>Queue</h3>
            <div class="header-actions">
                {#if hasUpcoming}
                    <button
                        class="clear-btn"
                        on:click={clearUpcoming}
                        title="Clear upcoming"
                    >
                        Clear
                    </button>
                {/if}
                <button
                    class="close-btn"
                    on:click={toggleQueue}
                    title="Close queue"
                    aria-label="Close queue"
                >
                    <svg
                        viewBox="0 0 24 24"
                        width="20"
                        height="20"
                        fill="currentColor"
                    >
                        <path
                            d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z"
                        />
                    </svg>
                </button>
            </div>
        </header>
    {/if}

        <div class="queue-content">
            {#if upcomingTracks.length > 0}
                <section class="queue-section">
                    <h4 class="section-title">
                        Next Up
                        <span class="count">{upcomingTracks.length}</span>
                    </h4>

                    <div
                        class="queue-list upcoming-dnd"
                        class:shuffle-disabled={$shuffle}
                        use:dndzone={{
                            items: dndItems,
                            flipDurationMs: FLIP_MS,
                            dragDisabled: $shuffle,
                            type: 'queue-upcoming',
                            // svelte-dnd-action's default dropTargetStyle is a yellow
                            // outline around the entire zone — ugly and clashes with
                            // the dark theme. Empty object suppresses it; the per-item
                            // FLIP animation already gives enough feedback.
                            dropTargetStyle: {},
                            dropTargetClasses: [],
                        }}
                        on:consider={handleConsider}
                        on:finalize={handleFinalize}
                    >
                        {#each dndItems as item (item.id)}
                            <div
                                class="queue-track"
                                class:priority={item.isPriority}
                            >
                                {#if item.index !== $queueIndex}
                                    <div
                                        class="drag-handle"
                                        use:dragHandle
                                        title="Drag to reorder"
                                        role="button"
                                        tabindex="-1"
                                    >
                                        <svg
                                            viewBox="0 0 24 24"
                                            fill="currentColor"
                                            width="16"
                                            height="16"
                                        >
                                            <path
                                                d="M3 15h18v-2H3v2zm0 4h18v-2H3v2zm0-8h18V9H3v2zm0-6v2h18V5H3z"
                                            />
                                        </svg>
                                    </div>
                                {:else}
                                    <!-- Spacer so the track-btn doesn't shift when drag-handle is hidden -->
                                    <div class="drag-handle-spacer"></div>
                                {/if}
                                <button
                                    class="track-btn"
                                    on:click={() => handlePlayTrack(item.index)}
                                >
                                    <div class="track-art">
                                        {#if getTrackArt(item.track)}
                                            <img
                                                src={getTrackArt(item.track)}
                                                alt=""
                                                loading="lazy"
                                                decoding="async"
                                            />
                                        {:else}
                                            <div class="art-placeholder">
                                                <svg
                                                    viewBox="0 0 24 24"
                                                    fill="currentColor"
                                                    width="16"
                                                    height="16"
                                                >
                                                    <path
                                                        d="M12 3v10.55c-.59-.34-1.27-.55-2-.55-2.21 0-4 1.79-4 4s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z"
                                                    />
                                                </svg>
                                            </div>
                                        {/if}
                                    </div>
                                    <div class="track-info">
                                        <span class="track-title truncate"
                                            >{item.track.title ||
                                                "Unknown Title"}</span
                                        >
                                        <span class="track-artist truncate"
                                            >{item.track.artist ||
                                                "Unknown Artist"}</span
                                        >
                                    </div>
                                </button>
                                <span class="track-duration"
                                    >{formatDuration(item.track.duration)}</span
                                >
                                <button
                                    class="remove-btn"
                                    on:click={() => handleRemove(item.index)}
                                    title="Remove from queue"
                                >
                                    <svg
                                        viewBox="0 0 24 24"
                                        fill="currentColor"
                                        width="16"
                                        height="16"
                                    >
                                        <path
                                            d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z"
                                        />
                                    </svg>
                                </button>
                            </div>
                        {/each}
                    </div>

                    {#if $shuffle}
                        <p class="dnd-hint">Reorder is disabled in shuffle mode</p>
                    {/if}
                </section>
            {/if}

            {#if historyTracks.length > 0}
                <section class="queue-section history">
                    <h4 class="section-title">
                        Recently Played
                        <span class="count">{historyTracks.length}</span>
                    </h4>
                    <div 
                        class="queue-list virtualized"
                        on:scroll={handleHistoryScroll}
                        bind:this={historyContainerElement}
                    >
                        <div class="virtual-spacer" style="height: {historyVirtualState.totalHeight}px;">
                            <div 
                                class="virtual-content"
                                style="transform: translateY({historyVirtualState.offsetY}px);"
                            >
                                {#each historyVirtualState.visibleTracks as item, i (item.track.id + "-history-" + item.index)}
                                    <div 
                                        class="queue-track past"
                                        style="height: {TRACK_ROW_HEIGHT}px;"
                                    >
                                        <button
                                            class="track-btn"
                                            on:click={() => handlePlayTrack(item.index)}
                                        >
                                            <div class="track-art">
                                                {#if getTrackArt(item.track)}
                                                    <img
                                                        src={getTrackArt(item.track)}
                                                        alt=""
                                                        loading="lazy"
                                                        decoding="async"
                                                    />
                                                {:else}
                                                    <div class="art-placeholder">
                                                        <svg
                                                            viewBox="0 0 24 24"
                                                            fill="currentColor"
                                                            width="16"
                                                            height="16"
                                                        >
                                                            <path
                                                                d="M12 3v10.55c-.59-.34-1.27-.55-2-.55-2.21 0-4 1.79-4 4s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z"
                                                            />
                                                        </svg>
                                                    </div>
                                                {/if}
                                            </div>
                                            <div class="track-info">
                                                <span class="track-title truncate"
                                                    >{item.track.title ||
                                                        "Unknown Title"}</span
                                                >
                                                <span class="track-artist truncate"
                                                    >{item.track.artist ||
                                                        "Unknown Artist"}</span
                                                >
                                            </div>
                                        </button>
                                        <span class="track-duration"
                                            >{formatDuration(item.track.duration)}</span
                                        >
                                    </div>
                                {/each}
                            </div>
                        </div>
                    </div>
                </section>
            {/if}

            {#if $queue.length === 0}
                <div class="empty-state">
                    <svg
                        viewBox="0 0 24 24"
                        fill="currentColor"
                        width="48"
                        height="48"
                    >
                        <path
                            d="M15 6H3v2h12V6zm0 4H3v2h12v-2zM3 16h8v-2H3v2zM17 6v8.18c-.31-.11-.65-.18-1-.18-1.66 0-3 1.34-3 3s1.34 3 3 3 3-1.34 3-3V8h3V6h-5z"
                        />
                    </svg>
                    <p>Queue is empty</p>
                    <span>Play some tracks to fill the queue</span>
                </div>
            {/if}
        </div>
    </aside>
{/if}

<style>
    .queue-panel {
        width: 350px;
        min-width: 300px;
        max-width: 400px;
        height: 100%;
        min-height: 0;
        background: linear-gradient(
            180deg,
            var(--bg-elevated) 0%,
            var(--bg-base) 100%
        );
        border-left: 1px solid var(--border-color);
        display: flex;
        flex-direction: column;
    }

    .queue-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: var(--spacing-md);
        border-bottom: 1px solid var(--border-color);
        flex-shrink: 0;
    }

    .queue-header h3 {
        font-size: 1rem;
        font-weight: 600;
        color: var(--text-primary);
    }

    .header-actions {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
    }

    .clear-btn {
        font-size: 0.75rem;
        color: var(--text-secondary);
        padding: 4px 8px;
        border-radius: var(--radius-sm);
        transition: all var(--transition-fast);
    }

    .clear-btn:hover {
        color: var(--text-primary);
        background-color: rgba(255, 255, 255, 0.1);
    }

    .close-btn {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 36px;
        height: 36px;
        border-radius: var(--radius-full);
        color: var(--text-secondary);
        transition: all var(--transition-fast);
    }

    .close-btn:hover {
        color: var(--text-primary);
        background-color: rgba(255, 255, 255, 0.1);
    }

    .queue-content {
        flex: 1;
        overflow-y: auto;
        padding: var(--spacing-md);
        overscroll-behavior-y: contain;
    }

    .queue-section {
        margin-bottom: var(--spacing-lg);
    }

    .section-title {
        font-size: 0.6875rem;
        font-weight: 700;
        text-transform: uppercase;
        letter-spacing: 0.1em;
        color: var(--text-subdued);
        margin-bottom: var(--spacing-sm);
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
    }

    .section-title .count {
        background-color: var(--bg-surface);
        padding: 2px 6px;
        border-radius: var(--radius-full);
        font-size: 0.625rem;
    }

    .queue-list {
        display: flex;
        flex-direction: column;
        gap: 2px;
    }

    /* Virtualization - flexible height (history only now) */
    .queue-list.virtualized {
        /* height for desktop can change */
        max-height: min(400px, 40vh);
        overflow-y: auto;
        overflow-x: hidden;
        position: relative;
        overscroll-behavior-y: contain;
    }

    /* svelte-dnd-action upcoming list */
    .queue-list.upcoming-dnd {
        display: flex;
        flex-direction: column;
        gap: 2px;
        max-height: min(400px, 40vh);
        overflow-y: auto;
        overflow-x: hidden;
        overscroll-behavior-y: contain;
    }

    .queue-list.upcoming-dnd.shuffle-disabled {
        opacity: 0.6;
        pointer-events: none;
    }

    .dnd-hint {
        font-size: 0.75rem;
        color: var(--text-subdued);
        text-align: center;
        margin-top: var(--spacing-xs);
        font-style: italic;
    }

    .drag-handle-spacer {
        width: 24px;
        height: 24px;
        flex-shrink: 0;
    }

    .virtual-spacer {
        position: relative;
        width: 100%;
    }

    .virtual-content {
        position: absolute;
        top: 0;
        left: 0;
        right: 0;
        will-change: transform;
        display: flex;
        flex-direction: column;
        gap: 2px;
    }

    .queue-track {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        padding: var(--spacing-xs);
        border-radius: var(--radius-md);
        transition: background-color var(--transition-fast);
        box-sizing: border-box;
    }

    .queue-track:hover {
        background-color: rgba(255, 255, 255, 0.1);
    }

    .queue-track.current {
        background-color: var(--accent-subtle);
        padding: var(--spacing-sm);
    }

    .queue-track.past {
        opacity: 0.6;
    }

    .queue-track.past:hover {
        opacity: 1;
    }

    .drag-handle {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 24px;
        height: 24px;
        color: var(--text-subdued);
        opacity: 0;
        transition: all var(--transition-fast);
        flex-shrink: 0;
        user-select: none;
        -webkit-user-select: none;
        touch-action: none;
    }

    .queue-track:hover .drag-handle {
        opacity: 1;
    }

    .drag-handle:hover {
        color: var(--text-primary);
    }

    .track-btn {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        flex: 1;
        min-width: 0;
        text-align: left;
    }

    .track-art {
        position: relative;
        width: 40px;
        height: 40px;
        border-radius: var(--radius-sm);
        overflow: hidden;
        flex-shrink: 0;
    }

    .track-art img {
        width: 100%;
        height: 100%;
        object-fit: cover;
    }

    .art-placeholder {
        width: 100%;
        height: 100%;
        background-color: var(--bg-highlight);
        display: flex;
        align-items: center;
        justify-content: center;
        color: var(--text-subdued);
    }

    .playing-indicator {
        position: absolute;
        inset: 0;
        background-color: rgba(0, 0, 0, 0.6);
        display: flex;
        align-items: center;
        justify-content: center;
        gap: 2px;
    }

    .playing-indicator .bar {
        width: 3px;
        height: 12px;
        background-color: var(--accent-primary);
        animation: equalizer 0.8s ease-in-out infinite;
    }

    .playing-indicator .bar:nth-child(2) {
        animation-delay: 0.2s;
    }

    .playing-indicator .bar:nth-child(3) {
        animation-delay: 0.4s;
    }

    @keyframes equalizer {
        0%,
        100% {
            height: 4px;
        }
        50% {
            height: 14px;
        }
    }

    .track-info {
        display: flex;
        flex-direction: column;
        min-width: 0;
        flex: 1;
    }

    .track-title {
        font-size: 0.875rem;
        font-weight: 500;
        color: var(--text-primary);
    }

    .queue-track.current .track-title {
        color: var(--accent-primary);
    }

    .track-artist {
        font-size: 0.75rem;
        color: var(--text-secondary);
    }

    .track-duration {
        font-size: 0.75rem;
        color: var(--text-subdued);
        flex-shrink: 0;
    }

    .remove-btn {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 28px;
        height: 28px;
        border-radius: var(--radius-full);
        color: var(--text-subdued);
        opacity: 0;
        transition: all var(--transition-fast);
    }

    .queue-track:hover .remove-btn {
        opacity: 1;
    }

    .remove-btn:hover {
        color: var(--error-color);
        background-color: rgba(241, 94, 108, 0.1);
    }

    .empty-state {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        height: 200px;
        color: var(--text-subdued);
        text-align: center;
        gap: var(--spacing-sm);
    }

    .empty-state p {
        font-size: 1rem;
        color: var(--text-secondary);
    }

    .empty-state span {
        font-size: 0.8125rem;
    }

    .history {
        opacity: 0.7;
    }

    .history:hover {
        opacity: 1;
    }

    .truncate {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    /* Mobile: full-screen overlay (z-index above FullScreenPlayer at 2000) */
    .queue-panel.mobile {
        position: fixed;
        top: 0;
        left: 0;
        right: 0;
        bottom: 0;
        width: 100%;
        max-width: 100%;
        min-width: 0;
        z-index: 2100;
        border-left: none;
        border-radius: 0;
    }

    .queue-panel.mobile.android-lite {
        transform: translateZ(0);
        backface-visibility: hidden;
        -webkit-backface-visibility: hidden;
    }

    .queue-panel.mobile .queue-header {
        padding: var(--spacing-md) var(--spacing-md);
        padding-top: calc(var(--spacing-md) + var(--safe-area-top));
    }

    .queue-panel.mobile .close-btn {
        width: 44px;
        height: 44px;
    }

    /* On mobile, always show drag handles since hover dosen't exist */
    .queue-panel.mobile .drag-handle {
        opacity: 1;
    }

    /* On mobile virtualized listsuse more space */
    .queue-panel.mobile .queue-list.virtualized {
        max-height: min(50vh, 500px);
    }
</style>