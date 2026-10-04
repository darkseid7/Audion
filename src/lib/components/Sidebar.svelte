<script lang="ts">
    import Navigation from "./presentation/Navigation.svelte";
    import type { NavigationRow, NavigationSection } from "$lib/application/presentation/types";
    import { desktopEffectsEnabled } from "$lib/application/bootstrap";
    import { onMount, createEventDispatcher } from "svelte";
    import {
        playlists,
        loadPlaylists,
        trackCount,
        albumCount,
        artistCount,
        loadAlbumsAndArtists,
        getTrackAlbumCover,
        getAlbumCoverFromTracks,
    } from "$lib/stores/library";
    import { getAlbum } from "$lib/api/tauri";
    import {
        currentView,
        goToHome,
        goToTracks,
        goToAlbums,
        goToArtists,
        goToPlaylists,
        goToPlaylistDetail,
        goToPlugins,
        goToSettings,
        goToLikedSongs,
        goToListenLater,
        goToListenBrainz,
        goToDiscover,
        goToRecentlyPlayed,
    } from "$lib/stores/view";
    import {
        isSettingsOpen as isSettingsOpenUI,
        toggleSettings as toggleSettingsUI,
        contextMenu,
    } from "$lib/stores/ui";
    import { appSettings } from "$lib/stores/settings";
    import { likedCount } from "$lib/stores/liked";
    import { listenLaterCount } from "$lib/stores/listen-later";
    import {
        selectMusicFolder,
        addFolder,
        rescanMusic,
        deletePlaylist,
        type Playlist,
        getPlaylistTracks,
        renamePlaylist,
    } from "$lib/api/tauri";
    import { playlistCovers, setPlaylistCover } from "$lib/stores/playlistCovers";
    import { progressiveScan } from "$lib/stores/progressiveScan";
    import { confirm, prompt } from "$lib/stores/dialogs";
    import {
        playTracks,
        addToQueue,
        playNext,
        currentTrack,
        isPlaying,
        queue,
    } from "$lib/stores/player";
    import { uiSlotManager } from "$lib/plugins/ui-slots";

    import { updates } from "$lib/stores/updates";
    import UpdatePopup from "./UpdatePopup.svelte";
    import SyncStatus from "./SyncStatus.svelte";

    import { currentPlaylistId } from "$lib/stores/player";
    import {
        pinnedItems,
        pinItem,
        unpinItem,
        isPinned,
    } from "$lib/stores/pinned";
    import { setCustomArtwork } from "$lib/stores/customArtwork";
    import { _ } from "svelte-i18n";

    const dispatch = createEventDispatcher();

    function navigateAndClose(fn: () => void) {
        fn();
        dispatch("navigate");
    }

    let isScanning = false;
    let scanStatus = "Scanning...";
    let scanError: string | null = null;
    let showUpdatePopup = false;

    // Slot containers
    let slotTop: HTMLDivElement;
    let slotBottom: HTMLDivElement;

    import { addToast } from "$lib/stores/toast";

    // Track counts for each playlist
    let playlistTrackCounts = new Map<number, number>();

    // Extract store values at top level for reactivity
    $: currentPlaylistIdValue = $currentPlaylistId;
    $: isPlayingValue = $isPlaying;

    // Helper function to check if a playlist is currently playing
    function isPlaylistPlaying(
        playlistId: number,
        currentId: number | null,
        playing: boolean,
    ): boolean {
        return currentId === playlistId && playing;
    }

    function initialsFromName(name: string): string {
        if (!name) return "PL";
        const parts = name.trim().split(/\s+/);
        return (
            parts
                .slice(0, 2)
                .map((p) => p[0]?.toUpperCase() ?? "")
                .join("") || name.slice(0, 2).toUpperCase()
        );
    }

    function hashToColor(str: string): string {
        let h = 0;
        for (let i = 0; i < str.length; i++)
            h = (h << 5) - h + str.charCodeAt(i);
        return `hsl(${Math.abs(h) % 360} 30% 30%)`;
    }

    function generateSvgCover(name: string, size = 512): string {
        const initials = initialsFromName(name);
        const bg = hashToColor(name || "playlist");
        const svg =
            `<svg xmlns='http://www.w3.org/2000/svg' width='${size}' height='${size}' viewBox='0 0 ${size} ${size}'>` +
            `<rect width='100%' height='100%' fill='${bg}'/>` +
            `<text x='50%' y='50%' dominant-baseline='middle' text-anchor='middle' font-family='Inter, system-ui, sans-serif' font-size='${Math.floor(size / 3)}' fill='white' font-weight='700'>${initials}</text>` +
            `</svg>`;
        return `data:image/svg+xml;base64,${btoa(unescape(encodeURIComponent(svg)))}`;
    }

    function handleImageError(e: Event, playlist: Playlist) {
        const img = e.target as HTMLImageElement;
        img.src = generateSvgCover(playlist.name || "Playlist", 512);
    }

    // Load track counts for all playlists
    async function loadPlaylistTrackCounts() {
        const counts = new Map<number, number>();
        for (const playlist of $playlists) {
            try {
                const tracks = await getPlaylistTracks(playlist.id);
                counts.set(playlist.id, tracks.length);
            } catch (error) {
                console.error(
                    `Failed to get track count for playlist ${playlist.id}:`,
                    error,
                );
                counts.set(playlist.id, 0);
            }
        }
        playlistTrackCounts = counts;
    }

    // Reload track counts when playlists change
    $: if ($desktopEffectsEnabled && $playlists.length > 0) {
        loadPlaylistTrackCounts();
    }

    async function handleAddFolder() {
        try {
            const path = await selectMusicFolder();
            if (path) {
                isScanning = true;
                scanStatus = "Scanning...";
                scanError = null;

                // Progressive scan: clear existing, stream new tracks in batches
                await progressiveScan.startScan(true);

                // Add folder then full rescan
                await addFolder(path);
                const result = await rescanMusic();

                if (result.errors.length > 0) {
                    console.warn("Scan errors:", result.errors);
                }

                console.log(
                    `Scan complete: ${result.tracks_added} added, ${result.tracks_updated} updated, ${result.tracks_deleted} deleted`,
                );

                // Tracks already loaded progressively — just fetch albums/artists
                await loadAlbumsAndArtists();
                await loadPlaylists();

                // success toast
                const parts = [];
                if (result.tracks_added > 0)
                    parts.push(`${result.tracks_added} added`);
                if (result.tracks_updated > 0)
                    parts.push(`${result.tracks_updated} updated`);
                if (result.tracks_deleted > 0)
                    parts.push(`${result.tracks_deleted} deleted`);

                const message =
                    parts.length > 0
                        ? `Library scan complete: ${parts.join(", ")}`
                        : "Library scan complete";

                addToast(message, "success", 4000);
            }
        } catch (error) {
            scanError = error instanceof Error ? error.message : String(error);
            console.error("Scan failed:", error);
            addToast("Failed to scan music folder", "error");
        } finally {
            isScanning = false;
            progressiveScan.reset();
        }
    }

    async function handlePlayPlaylist(id: number) {
        try {
            const tracks = await getPlaylistTracks(id);
            if (tracks.length > 0) {
                const playlist = $playlists.find((p) => p.id === id);
                playTracks(tracks, 0, {
                    type: "playlist",
                    playlistId: id,
                    displayName: playlist?.name ?? "Playlist",
                });
            }
        } catch (error) {
            console.error("Failed to play playlist:", error);
        }
    }

    async function handleAddToQueue(id: number) {
        try {
            const tracks = await getPlaylistTracks(id);
            if (tracks.length > 0) {
                addToQueue(tracks);
            }
        } catch (error) {
            console.error("Failed to add playlist to queue:", error);
        }
    }

    async function handlePlayNext(id: number) {
        try {
            const tracks = await getPlaylistTracks(id);
            if (tracks.length > 0) {
                playNext(tracks);
            }
        } catch (error) {
            console.error("Failed to play playlist next:", error);
        }
    }

    async function handleDeletePlaylist(id: number, name: string) {
        if (
            !(await confirm(`Delete playlist "${name}"?`, {
                title: "Delete Playlist",
                confirmLabel: "Delete",
                danger: true,
            }))
        )
            return;

        try {
            await deletePlaylist(id);
            await loadPlaylists();
            if (
                $currentView.type === "playlist-detail" &&
                $currentView.id === id
            ) {
                goToTracks(); // Navigate away if deleted
            }
        } catch (error) {
            console.error("Failed to delete playlist:", error);
        }
    }

    function handlePlaylistContextMenu(e: MouseEvent, playlist: Playlist) {
        e.preventDefault();
        const pinned = isPinned("playlist", playlist.id, $pinnedItems);
        contextMenu.set({
            visible: true,
            x: e.clientX,
            y: e.clientY,
            items: [
                {
                    label: "Play",
                    action: () => handlePlayPlaylist(playlist.id),
                },
                {
                    label: $_('contextMenu.playNext'),
                    action: () => handlePlayNext(playlist.id),
                },
                {
                    label: "Add to Queue",
                    action: () => handleAddToQueue(playlist.id),
                },
                {
                    label: pinned ? "Unpin from Top" : "Pin to Top",
                    icon: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18"><path d="M12 2L4.5 9L9 9L9 22L15 22L15 9L19.5 9L12 2Z"/></svg>`,
                    action: () => {
                        if (pinned) {
                            unpinItem("playlist", playlist.id);
                        } else {
                            pinItem("playlist", playlist.id);
                        }
                    },
                },
                { type: "separator" },
                {
                    label: "Rename",
                    action: async () => {
                        const newName = await prompt("Enter new name:", {
                            initialValue: playlist.name,
                            title: "Rename Playlist",
                        });
                        if (
                            newName &&
                            newName.trim() &&
                            newName !== playlist.name
                        ) {
                            try {
                                await renamePlaylist(
                                    playlist.id,
                                    newName.trim(),
                                );
                                await loadPlaylists();
                            } catch (error) {
                                console.error(
                                    "Failed to rename playlist:",
                                    error,
                                );
                            }
                        }
                    },
                },
                {
                    label: "Change Cover",
                    action: () => {
                        const input = document.createElement("input");
                        input.type = "file";
                        input.accept = "image/*";
                        input.onchange = (e) => {
                            const file = (e.target as HTMLInputElement)
                                .files?.[0];
                            if (file) {
                                const reader = new FileReader();
                                reader.onload = () => {
                                    const result = reader.result as string;
                                    setPlaylistCover(playlist.id, result);
                                };
                                reader.readAsDataURL(file);
                            }
                        };
                        input.click();
                    },
                },
                { type: "separator" },
                {
                    label: "Delete Playlist",
                    action: () =>
                        handleDeletePlaylist(playlist.id, playlist.name),
                },
            ],
        });
    }

    // Playlist pinning logic
    $: sortedPlaylists = [...$playlists].sort((a, b) => {
        const aPinned = isPinned("playlist", a.id, $pinnedItems);
        const bPinned = isPinned("playlist", b.id, $pinnedItems);
        if (aPinned && !bPinned) return -1;
        if (!aPinned && bPinned) return 1;
        return 0;
    });

    function isActive(viewType: string): boolean {
        return (
            $currentView.type === viewType ||
            ($currentView.type === "album-detail" && viewType === "albums") ||
            ($currentView.type === "artist-detail" && viewType === "artists")
        );
    }

    onMount(() => {
        if (!$desktopEffectsEnabled) return;
        loadPlaylists();
        updates.checkUpdate();

        // Register UI slots
        if (slotTop) uiSlotManager.registerContainer("sidebar:top", slotTop);
        if (slotBottom)
            uiSlotManager.registerContainer("sidebar:bottom", slotBottom);

        return () => {
            uiSlotManager.unregisterContainer("sidebar:top");
            uiSlotManager.unregisterContainer("sidebar:bottom");
        };
    });

    function navigationRow(id: string, label: string, count?: number): NavigationRow {
        return { id, label, icon: id as NavigationRow["icon"], active: isActive(id), ...(count === undefined ? {} : { count }), availability: { enabled: true } };
    }
    let navigationSections: NavigationSection[];
 $: { void $currentView; navigationSections = [
        { id: "library", label: $_('sidebar.library', { default: 'Library' }), rows: [navigationRow("home", $_('sidebar.home', { default: "Home" })),navigationRow("albums", $_('sidebar.albums', { default: "Albums" }), $albumCount),navigationRow("liked-songs", $_('sidebar.likedSongs', { default: "Liked Songs" }), $likedCount),navigationRow("listen-later", $_('sidebar.listenLater', { default: "Escuchar más tarde" }), $listenLaterCount),navigationRow("recently-played", $_('sidebar.thisWeek', { default: "This Week" })),navigationRow("discover", $_('sidebar.discover', { default: "Discover" })), ...($appSettings.listenBrainzEnabled && $appSettings.listenBrainzTokenSet ? [{ id: "listenbrainz", label: $_('sidebar.recommendations', { default: 'Recommendations' }), icon: "listenbrainz" as const, active: isActive("listenbrainz"), availability: { enabled: true as const } }] : []), navigationRow("tracks", $_('sidebar.allTracks', { default: "All Tracks" }), $trackCount), navigationRow("artists", $_('sidebar.artists', { default: "Artists" }), $artistCount)] },
        { id: "playlists", label: $_('sidebar.playlists', { default: 'Playlists' }), rows: [navigationRow("playlists", $_('sidebar.allPlaylists', { default: "All Playlists" }), $playlists.length)] },
        { id: "settings", label: $_('sidebar.settings', { default: 'Settings' }), rows: [navigationRow("plugins", $_('sidebar.plugins', { default: "Plugins" })), navigationRow("settings", $_('sidebar.settings', { default: "Settings" }))] }
    ]; }
    function navigateShared(id: string): void {
        if (!$desktopEffectsEnabled) return;
        const actions: Record<string, () => void> = { home: goToHome, albums: goToAlbums, "liked-songs": goToLikedSongs, "listen-later": goToListenLater, "recently-played": goToRecentlyPlayed, discover: goToDiscover, listenbrainz: goToListenBrainz, tracks: goToTracks, artists: goToArtists, playlists: goToPlaylists, plugins: goToPlugins, settings: goToSettings };
        if (actions[id]) navigateAndClose(actions[id]);
    }
</script>
{#if $desktopEffectsEnabled}

<Navigation sections={navigationSections} onNavigate={navigateShared}>
    <svelte:fragment slot="header">            <SyncStatus />
            {#if $updates.hasUpdate}
                <div
                    class="update-badge"
                    title="View update details"
                    on:click={() => (showUpdatePopup = true)}
                    role="button"
                    tabindex="0"
                    on:keydown={(e) =>
                        e.key === "Enter" && (showUpdatePopup = true)}
                >
                    Update
                </div>
            {/if}
</svelte:fragment>
    <div slot="top" class="plugin-slot" bind:this={slotTop}></div>
    <svelte:fragment slot="playlists">                {#each sortedPlaylists as playlist (playlist.id)}
                    <li>
                        <button
                            class="nav-item playlist-item"
                            class:active={$currentView.type ===
                                "playlist-detail" &&
                                $currentView.id !== undefined &&
                                playlist.id !== undefined &&
                                $currentView.id === playlist.id}
                            class:playing={isPlaylistPlaying(
                                playlist.id,
                                currentPlaylistIdValue,
                                isPlayingValue,
                            )}
                            on:click={() =>
                                navigateAndClose(() =>
                                    goToPlaylistDetail(playlist.id, playlist.name),
                                )}
                            on:contextmenu={(e) =>
                                handlePlaylistContextMenu(e, playlist)}
                        >
                            {#if isPlaylistPlaying(playlist.id, currentPlaylistIdValue, isPlayingValue)}
                                <div class="playing-indicator">
                                    <span class="bar"></span>
                                    <span class="bar"></span>
                                    <span class="bar"></span>
                                </div>
                            {:else}
                                <div class="playlist-icon-container">
                                    {#if $playlistCovers && $playlistCovers[playlist.id]}
                                        <img
                                            src={$playlistCovers[playlist.id]}
                                            alt=""
                                            class="sidebar-playlist-art"
                                            on:error={(e) =>
                                                handleImageError(e, playlist)}
                                        />
                                    {:else if playlist.cover_url}
                                        <img
                                            src={playlist.cover_url}
                                            alt=""
                                            class="sidebar-playlist-art"
                                            on:error={(e) =>
                                                handleImageError(e, playlist)}
                                        />
                                    {:else}
                                        <svg
                                            viewBox="0 0 24 24"
                                            fill="currentColor"
                                            width="24"
                                            height="24"
                                        >
                                            <path
                                                d="M15 6H3v2h12V6zm0 4H3v2h12v-2zM3 16h8v-2H3v2zM17 6v8.18c-.31-.11-.65-.18-1-.18-1.66 0-3 1.34-3 3s1.34 3 3 3 3-1.34 3-3V8h3V6h-5z"
                                            />
                                        </svg>
                                    {/if}
                                </div>
                            {/if}
                            <span class="playlist-name">
                                {playlist.name}
                            </span>
                            {#if isPinned("playlist", playlist.id, $pinnedItems)}
                                <span
                                    class="pinned-indicator-sidebar"
                                    title="Pinned to top"
                                >
                                    <svg
                                        viewBox="0 0 24 24"
                                        fill="currentColor"
                                        width="12"
                                        height="12"
                                    >
                                        <path
                                            d="M16 9V4l1 0V2H7v2l1 0v5c0 1.66-1.34 3-3 3v2h5.97v7l1 1 1-1v-7H19v-2c-1.66 0-3-1.34-3-3z"
                                        />
                                    </svg>
                                </span>
                            {/if}
                            {#if playlistTrackCounts.has(playlist.id)}
                                <span class="nav-count"
                                    >{playlistTrackCounts.get(
                                        playlist.id,
                                    )}</span
                                >
                            {/if}
                        </button>
                    </li>
                {/each}</svelte:fragment>
    <svelte:fragment slot="community">        <section class="nav-section">
            <h3 class="nav-section-title">{$_('sidebar.community', { default: 'Community' })}</h3>
            <ul class="nav-list">
                {#if $appSettings.showDiscord}
                    <li>
                        <a
                            href="https://discord.gg/27XRVQsBd9"
                            target="_blank"
                            class="nav-item discord-item"
                        >
                            <svg
                                viewBox="0 0 24 24"
                                fill="currentColor"
                                width="24"
                                height="24"
                            >
                                <path
                                    d="M20.317 4.37a19.791 19.791 0 0 0-4.885-1.515.074.074 0 0 0-.079.037c-.21.375-.444.864-.608 1.25a18.27 18.27 0 0 0-5.487 0 12.64 12.64 0 0 0-.617-1.25.077.077 0 0 0-.079-.037A19.736 19.736 0 0 0 3.677 4.37a.07.07 0 0 0-.032.027C.533 9.046-.32 13.58.099 18.057a.082.082 0 0 0 .031.057 19.9 19.9 0 0 0 5.993 3.03.078.078 0 0 0 .084-.028 14.09 14.09 0 0 0 1.226-1.994.076.076 0 0 0-.041-.106 13.107 13.107 0 0 1-1.872-.892.077.077 0 0 1-.008-.128 10.2 10.2 0 0 0 .372-.292.074.074 0 0 1 .077-.01c3.928 1.793 8.18 1.793 12.062 0 a.074.074 0 0 1 .078.01c.118.098.246.198.373.292a.077.077 0 0 1-.006.127 12.299 12.299 0 0 1-1.873.892.077.077 0 0 0-.041.107c.36.698.772 1.362 1.225 1.993a.076.076 0 0 0 .084.028 19.839 19.839 0 0 0 6.002-3.03.077.077 0 0 0 .032-.054c.5-5.177-.838-9.674-3.549-13.66a.061.061 0 0 0-.031-.03zM8.02 15.33c-1.183 0-2.157-1.085-2.157-2.419 0-1.333.956-2.419 2.157-2.419 1.21 0 2.176 1.086 2.157 2.419 0 1.334-.956 2.419-2.157 2.419zm7.975 0c-1.183 0-2.157-1.085-2.157-2.419 0-1.333.955-2.419 2.157-2.419 1.21 0 2.176 1.086 2.157 2.419 0 1.334-.946 2.419-2.157 2.419z"
                                />
                            </svg>
                            <span>{$_('sidebar.joinDiscord', { default: 'Join Discord' })}</span>
                        </a>
                    </li>
                {/if}
                {#if $appSettings.showResonate}
                    <li>
                        <a
                            href="https://resonate.audionplayer.com?ref=audion"
                            target="_blank"
                            class="nav-item resonate-item"
                        >
                            <img src="/resonate.png" alt="Resonate" class="resonate-icon" />
                            <span>{$_('sidebar.resonate', { default: 'Resonate Playlists' })}</span>
                        </a>
                    </li>
                {/if}
            </ul>
        </section>
</svelte:fragment>
    <svelte:fragment slot="footer">
        <!-- Plugin slot: Bottom -->
        <div class="plugin-slot" bind:this={slotBottom}></div>

        <button
            class="add-folder-btn"
            on:click={handleAddFolder}
            disabled={isScanning}
        >
            {#if isScanning}
                <svg
                    class="animate-spin"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    width="20"
                    height="20"
                >
                    <circle
                        cx="12"
                        cy="12"
                        r="10"
                        stroke-width="2"
                        opacity="0.25"
                    />
                    <path
                        d="M12 2a10 10 0 0 1 10 10"
                        stroke-width="2"
                        stroke-linecap="round"
                    />
                </svg>
                <span>{scanStatus}</span>
            {:else}
                <svg
                    viewBox="0 0 24 24"
                    fill="currentColor"
                    width="20"
                    height="20"
                >
                    <path d="M19 13h-6v6h-2v-6H5v-2h6V5h2v6h6v2z" />
                </svg>
                <span>{$_('sidebar.addMusicFolder', { default: 'Add Music Folder' })}</span>
            {/if}
        </button>
        {#if scanError}
            <p class="scan-error">{scanError}</p>
        {/if}
</svelte:fragment>
</Navigation>

{#if showUpdatePopup && $updates.latestRelease}
    <UpdatePopup
        release={$updates.latestRelease}
        on:close={() => (showUpdatePopup = false)}
    />
{/if}

{/if}
