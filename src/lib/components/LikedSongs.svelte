<script lang="ts">
    import { applicationMode } from "$lib/application/bootstrap";
    import { viewActions } from "$lib/application/view-actions";
    import type { ApplicationIntent, ApplicationQuery } from "$lib/application/types";
    import ControllerBrowse from "./ControllerBrowse.svelte";
    function handleControllerIntent(intent: ApplicationIntent) { return viewActions.execute(intent); }
    function handleControllerQueue(query: ApplicationQuery, placement: "next" | "after_user_queue" | "end") { return viewActions.queueQuery(query, placement); }

    import { getLikedTracks, type Track } from "$lib/api/tauri";
    import { likedTrackIds } from "$lib/stores/liked";
    import { playTracks, setShuffle } from "$lib/stores/player";
    import EntityDetail from "./presentation/EntityDetail.svelte";
    import TrackList from "./TrackList.svelte";

    let tracks: Track[] = [];
    let loading = true;
    let scrollTop = 0;

    $: if ($applicationMode !== "controller") loadTracks($likedTrackIds);

    // Header transition calculations
    $: headerOpacity = Math.max(0, 1 - scrollTop / 150);
    $: headerScale = Math.max(0.85, 1 - scrollTop / 800);
    $: headerTranslateY = -scrollTop * 0.4;
    $: isHeaderSmall = scrollTop > 60;

    async function loadTracks(_ids: Set<number>) {
        try {
            tracks = await getLikedTracks();
        } catch (error) {
            console.error("[LikedSongs] Failed to load:", error);
        } finally {
            loading = false;
        }
    }

    async function handlePlayAll() {
        if (tracks.length > 0) {
            await setShuffle(false);
            playTracks(tracks, 0);
        }
    }

    async function handleShufflePlay() {
        if (tracks.length > 0) {
            await setShuffle(true);
            const randomIndex = Math.floor(Math.random() * tracks.length);
            playTracks(tracks, randomIndex);
        }
    }
    function handleScroll(e: Event) {
        scrollTop = (e.target as HTMLElement).scrollTop;
    }
</script>
{#if $applicationMode === "controller"}
 <ControllerBrowse query={{type:"liked_tracks"}} heading="Liked Songs" context={{type:"liked"}} enqueue={handleControllerQueue} execute={handleControllerIntent} />
{:else}


<div class="liked-songs-view">
    <!-- Header -->
    <EntityDetail kind="liked" className={isHeaderSmall ? "is-small" : ""} style="opacity:{headerOpacity}; transform:translateY({headerTranslateY}px) scale({headerScale})">
        <div class="liked-gradient-bg">
            <svg viewBox="0 0 24 24" width="64" height="64" fill="currentColor">
                <path
                    d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"
                />
            </svg>
        </div>
        <div class="liked-header-info">
            <span class="liked-label">PLAYLIST</span>
            <h1 class="liked-title">Liked Songs</h1>
            <div class="liked-count-container">
                <span class="liked-count">{tracks.length} songs</span>
            </div>
        </div>
    <svelte:fragment slot="controls"><div class="liked-controls">
        <div class="controls-wrapper">
            <button
                class="play-all-btn"
                on:click={handlePlayAll}
                disabled={tracks.length === 0}
                aria-label="Play All"
            >
                <div class="btn-icon">
                    <svg
                        viewBox="0 0 24 24"
                        fill="currentColor"
                        width="24"
                        height="24"
                    >
                        <path d="M8 5v14l11-7z" />
                    </svg>
                </div>
                <span>Play All</span>
            </button>

            <button
                class="shuffle-btn"
                on:click={handleShufflePlay}
                disabled={tracks.length === 0}
                aria-label="Shuffle Play"
            >
                <svg
                    viewBox="0 0 24 24"
                    fill="currentColor"
                    width="20"
                    height="20"
                >
                    <path
                        d="M10.59 9.17L5.41 4L4 5.41l5.17 5.17l1.42-1.41zM14.5 4l2.04 2.04L4 18.59L5.41 20L17.96 7.45L20 9.5V4h-5.5zm.33 9.41l-1.41 1.41l3.13 3.13L14.5 20H20v-5.5l-2.04 2.04l-3.13-3.13z"
                    />
                </svg>
                <span>Shuffle</span>
            </button>
        </div>
    </div></svelte:fragment></EntityDetail>

    <!-- Controls -->


    <!-- Track List -->
    <div class="liked-body" on:scroll={handleScroll}>
        {#if loading}
            <div class="loading">Loading liked songs...</div>
        {:else if tracks.length === 0}
            <div class="empty-state">
                <svg
                    viewBox="0 0 24 24"
                    width="48"
                    height="48"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.5"
                >
                    <path
                        d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"
                    />
                </svg>
                <p>Songs you like will appear here</p>
                <span class="empty-hint"
                    >Find songs and tap the heart icon to save them.</span
                >
            </div>
        {:else}
            <TrackList
                {tracks}
                title="Liked Songs"
                showAlbum={true}
                scrollKey="liked-songs"
            />
        {/if}
    </div>
</div>

{/if}

<style>
    .liked-songs-view {
        height: 100%;
        display: flex;
        flex-direction: column;
        background-color: var(--bg-base);
    }

    /* Header */






    .liked-songs-view:hover .liked-gradient-bg {
        transform: scale(1.02);
    }













    /* Controls */


















    /* Body - fills remaining space */
    .liked-body {
        flex: 1;
        min-height: 0;
        overflow: hidden;
    }

    /* Empty / Loading State */
    .loading,
    .empty-state {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        padding: 64px var(--spacing-xl);
        color: var(--text-subdued);
        gap: var(--spacing-md);
    }

    .empty-state p {
        font-size: 1.2rem;
        font-weight: 600;
        color: var(--text-primary);
        margin: 0;
    }

    .empty-hint {
        font-size: 0.875rem;
        color: var(--text-secondary);
    }

    @media (max-width: 768px) {






        .liked-body {
            padding-bottom: calc(
                var(--mobile-bottom-inset) + var(--spacing-md)
            );
        }
    }
</style>
