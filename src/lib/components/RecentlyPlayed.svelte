<script lang="ts">
    import { onMount } from "svelte";
    import { _ } from "svelte-i18n";
    import { getPlayedThisWeek } from "$lib/api/tauri";
    import type { Album, Track } from "$lib/api/tauri";
    import { dedupeTracksToAlbums } from "$lib/stores/activity";
    import AlbumGrid from "./AlbumGrid.svelte";
    import TrackList from "./TrackList.svelte";

    type Tab = "albums" | "tracks";
    let activeTab: Tab = "albums";

    let thisWeekAlbums: Album[] = [];
    let thisWeekTracks: Track[] = [];
    let loading = true;

    // Fetch enough this-week tracks that we can dedupe into a meaningful
    // album list. 500 tracks is a comfortable upper bound for one week of
    // listening.
    const FETCH_LIMIT = 500;

    onMount(async () => {
        try {
            const tracksRes = await getPlayedThisWeek(FETCH_LIMIT);
            thisWeekTracks = tracksRes ?? [];
            thisWeekAlbums = dedupeTracksToAlbums(thisWeekTracks);
        } catch (e) {
            console.error("Failed to load recently played", e);
            thisWeekAlbums = [];
            thisWeekTracks = [];
        } finally {
            loading = false;
        }
    });
</script>

<div class="recently-played-view">
    <header class="recently-played-header">
        <h1>{$_("recentlyPlayed.title", { default: "This Week" })}</h1>
        <span class="count"
            >{thisWeekAlbums.length}
            {$_("recentlyPlayed.albums", { default: "albums" })} ·
            {thisWeekTracks.length}
            {$_("recentlyPlayed.tracks", { default: "tracks" })}</span
        >
    </header>

    <div class="tabs">
        <button
            class="tab"
            class:active={activeTab === "albums"}
            on:click={() => (activeTab = "albums")}
        >
            {$_("home.tabAlbums", { default: "Albums" })}
        </button>
        <button
            class="tab"
            class:active={activeTab === "tracks"}
            on:click={() => (activeTab = "tracks")}
        >
            {$_("home.tabTracks", { default: "Tracks" })}
        </button>
    </div>

    <div class="content">
        {#if loading}
            <div class="empty-state">
                <p>Loading…</p>
            </div>
        {:else if activeTab === "albums"}
            {#if thisWeekAlbums.length === 0}
                <div class="empty-state">
                    <p>
                        {$_("recentlyPlayed.emptyAlbums", {
                            default: "No albums played this week",
                        })}
                    </p>
                    <span>
                        {$_("recentlyPlayed.emptyDescription", {
                            default: "Start playing tracks and they'll appear here.",
                        })}
                    </span>
                </div>
            {:else}
                <AlbumGrid albums={thisWeekAlbums} />
            {/if}
        {:else}
            {#if thisWeekTracks.length === 0}
                <div class="empty-state">
                    <p>
                        {$_("recentlyPlayed.emptyTracks", {
                            default: "No tracks played this week",
                        })}
                    </p>
                </div>
            {:else}
                <TrackList
                    tracks={thisWeekTracks}
                    showAlbum={true}
                    playbackContext={{
                        type: "recently-played",
                        displayName: $_("recentlyPlayed.title", {
                            default: "This Week",
                        }),
                        displaySubtitle: "Tracks",
                        tracks: thisWeekTracks,
                    }}
                />
            {/if}
        {/if}
    </div>
</div>

<style>
    .recently-played-view {
        height: 100%;
        display: flex;
        flex-direction: column;
        min-height: 0;
    }

    .recently-played-header {
        padding: 24px 24px 12px;
        display: flex;
        align-items: baseline;
        justify-content: space-between;
        gap: 12px;
    }

    .recently-played-header h1 {
        margin: 0;
        font-size: 1.75rem;
    }

    .count {
        color: var(--text-secondary);
        font-size: 0.9rem;
    }

    .tabs {
        display: flex;
        gap: var(--spacing-md);
        padding: 0 24px;
        border-bottom: 1px solid var(--border-color);
    }

    .tab {
        padding: var(--spacing-md);
        font-size: 0.875rem;
        font-weight: 600;
        color: var(--text-secondary);
        background: none;
        border: none;
        border-bottom: 2px solid transparent;
        margin-bottom: -1px;
        cursor: pointer;
        transition: all var(--transition-fast);
    }

    .tab:hover {
        color: var(--text-primary);
    }

    .tab.active {
        color: var(--text-primary);
        border-bottom-color: var(--accent-primary);
    }

    .content {
        flex: 1;
        overflow-y: auto;
        padding: 16px 24px 24px;
    }

    .empty-state {
        margin: auto;
        text-align: center;
        color: var(--text-secondary);
        padding: 48px 24px;
        max-width: 560px;
    }

    .empty-state p {
        margin: 0 0 8px;
        font-size: 1.1rem;
        color: var(--text-primary);
        font-weight: 600;
    }
</style>
