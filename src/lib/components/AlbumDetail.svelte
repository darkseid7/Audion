<script lang="ts">
    import { onMount } from "svelte";
    import type { Album, Track } from "$lib/api/tauri";
    import {
        getAlbum,
        getTracksByAlbum,
        getAlbumArtSrc,
        getAlbumCoverSrc,
        getTrackCoverSrc,
        formatDuration,
        getReleaseMbInfo,
        enrichAlbumYear,
        type MbReleaseInfo,
    } from "$lib/api/tauri";
    import { playTracks, currentTrack, isPlaying, appendToQueueEnd } from "$lib/stores/player";
    import { goToAlbums, goToArtistDetail, goBack } from "$lib/stores/view";
    import { loadLibrary, getAlbumCoverFromTracks, albums } from "$lib/stores/library";
    import TrackList from "./TrackList.svelte";
    import AlbumInfoModal from "./AlbumInfoModal.svelte";
    import {
        downloadTracks,
        hasDownloadableTracks,
        needsDownloadLocation,
        showDownloadResult,
        type DownloadProgress,
    } from "$lib/services/downloadService";
    import { addToast } from "$lib/stores/toast";
    import { goto } from "$app/navigation";
    import { confirm, prompt } from "$lib/stores/dialogs";
    import { _, locale } from "svelte-i18n";

    export let albumId: number;

    let album: Album | null = null;
    let tracks: Track[] = [];
    let groupedTracks: { disc: number; tracks: Track[] }[] = [];
    let loading = true;

    // MusicBrainz release info
    let mbRelease: MbReleaseInfo | null = null;
    let mbReleaseLoading = false;

    // Album Info modal
    let infoOpen = false;

    $: totalDuration = tracks.reduce((sum, t) => sum + (t.duration || 0), 0);
    type AlbumAudioInfo = {
        format: string | null;
        sampleRate: number | null;
        bitDepth: number | null;
    };

    function normalizeFormat(format: string | null | undefined): string | null {
        if (!format) return null;
        const formatUpper = format.toUpperCase();
        if (formatUpper.includes("HI_RES") || formatUpper.includes("HIRES")) {
            return "HI-RES";
        }
        if (formatUpper.includes("LOSSLESS")) {
            return "LOSSLESS";
        }
        return formatUpper.replace("MPEG", "MP3");
    }

    function parseTrackAudioMeta(track: Track): {
        sampleRate: number | null;
        bitDepth: number | null;
    } {
        if (!track.metadata_json) {
            return { sampleRate: null, bitDepth: null };
        }
        try {
            const meta = JSON.parse(track.metadata_json);
            return {
                sampleRate: meta["__sample_rate_hz"] ?? null,
                bitDepth: meta["__bit_depth"] ?? null,
            };
        } catch {
            return { sampleRate: null, bitDepth: null };
        }
    }

    function mostCommonValue<T>(values: Array<T | null | undefined>): T | null {
        const counts = new Map<T, number>();
        for (const value of values) {
            if (value === null || value === undefined) continue;
            counts.set(value, (counts.get(value) || 0) + 1);
        }
        let best: T | null = null;
        let bestCount = 0;
        for (const [value, count] of counts.entries()) {
            if (count > bestCount) {
                best = value;
                bestCount = count;
            }
        }
        return best;
    }

    function formatSampleRate(hz: number): string {
        return hz % 1000 === 0 ? `${hz / 1000}kHz` : `${(hz / 1000).toFixed(1)}kHz`;
    }

    function buildAlbumAudioInfo(trackList: Track[]): AlbumAudioInfo {
        const formats = trackList.map((track) => normalizeFormat(track.format));
        const sampleRates = trackList.map((track) => parseTrackAudioMeta(track).sampleRate);
        const bitDepths = trackList.map((track) => parseTrackAudioMeta(track).bitDepth);

        return {
            format: mostCommonValue(formats),
            sampleRate: mostCommonValue(sampleRates),
            bitDepth: mostCommonValue(bitDepths),
        };
    }

    $: albumAudioInfo = buildAlbumAudioInfo(tracks);

    function groupTracksByDisc(tracks: Track[]) {
        const groups = new Map<number, Track[]>();

        tracks.forEach((track) => {
            const disc = track.disc_number || 1;
            if (!groups.has(disc)) {
                groups.set(disc, []);
            }
            groups.get(disc)?.push(track);
        });

        return Array.from(groups.entries())
            .sort((a, b) => a[0] - b[0])
            .map(([disc, tracks]) => ({ disc, tracks }));
    }

    async function loadAlbumData() {
        loading = true;
        mbRelease = null;
        try {
            const [albumData, trackData] = await Promise.all([
                getAlbum(albumId),
                getTracksByAlbum(albumId),
            ]);
            album = albumData;
            tracks = trackData;
            groupedTracks = groupTracksByDisc(tracks);
            // Background MB fetch — don't await so it doesn't block the UI
            if (album) fetchMbRelease(album.name, album.artist || "");
        } catch (error) {
            console.error("Failed to load album:", error);
        } finally {
            loading = false;
        }
    }

    async function fetchMbRelease(name: string, artist: string) {
        if (!name) return;
        mbReleaseLoading = true;
        try {
            mbRelease = await getReleaseMbInfo(name, artist);
            // Persist year to DB if album is missing it
            if (album && mbRelease && (mbRelease.original_year || mbRelease.year)) {
                if (album.original_year == null || album.year == null) {
                    try {
                        const result = await enrichAlbumYear(album.id, name, artist);
                        if (result.original_year != null && album.original_year == null) {
                            album.original_year = result.original_year;
                        }
                        if (result.year != null && album.year == null) {
                            album.year = result.year;
                        }
                        // Update the albums store so the grid reflects the change
                        albums.update(list => list.map(a =>
                            a.id === album!.id
                                ? { ...a, year: album!.year, original_year: album!.original_year }
                                : a
                        ));
                    } catch (e) {
                        console.warn("[AlbumDetail] Failed to persist MB year:", e);
                    }
                }
            }
        } catch (e) {
            console.warn("[AlbumDetail] MB release fetch failed:", e);
        } finally {
            mbReleaseLoading = false;
        }
    }

    // Download state
    let isDownloading = false;
    let downloadProgress = "";

    // Check if we have downloadable tracks that are NOT yet downloaded
    $: downloadableTracks = tracks.filter((t) => {
        // Must be downloadable (streaming source) AND not have a local_src yet
        return hasDownloadableTracks([t]) && !t.local_src;
    });

    $: hasDownloadable = downloadableTracks.length > 0;

    // Check if everything that CAN be downloaded IS downloaded
    $: allDownloaded =
        tracks.length > 0 &&
        tracks.every((t) => {
            // If it's local, it's downloaded.
            if (!t.source_type || t.source_type === "local") return true;
            // If it's streaming, it must have local_src
            return !!t.local_src;
        });

    // Check if Tidal plugin is available (for hiding empty albums)
    import { pluginStore } from "$lib/stores/plugin-store";
    $: isTidalAvailable = $pluginStore.installed.some(
        (p) => p.name === "Tidal Search" && p.enabled,
    );
    $: shouldShowAlbum = tracks.length > 0 || isTidalAvailable;

    function formatBytes(bytes: number): string {
        if (bytes === 0) return "0 B";
        const k = 1024;
        const sizes = ["B", "KB", "MB", "GB"];
        const i = Math.floor(Math.log(bytes) / Math.log(k));
        return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + " " + sizes[i];
    }

    async function handleDownloadAll() {
        if (isDownloading) return;

        if (needsDownloadLocation()) {
            addToast(
                "Please configure a download location in Settings first",
                "error",
            );
            // Optionally redirect to settings
            return;
        }

        isDownloading = true;
        downloadProgress = "Starting...";

        try {
            const result = await downloadTracks(
                tracks,
                (progress: DownloadProgress) => {
                    const current = progress.current;
                    const total = progress.total;

                    if (progress.bytesTotal) {
                        const currentMB = formatBytes(
                            progress.bytesCurrent || 0,
                        );
                        const totalMB = formatBytes(progress.bytesTotal);
                        downloadProgress = `${current}/${total} (${currentMB}/${totalMB})`;
                    } else {
                        downloadProgress = `${current}/${total}`;
                    }
                },
            );

            showDownloadResult(result);
        } catch (error) {
            console.error("Download failed:", error);
            addToast("Download failed unexpectedly", "error");
        } finally {
            isDownloading = false;
            downloadProgress = "";
        }
    }

    function handlePlayAll() {
        const tracksToPlay = showOnlyLiked ? likedTracks : tracks;
        if (tracksToPlay.length > 0 && album) {
            playTracks(tracksToPlay, 0, {
                type: "album",
                albumId: album.id,
                displayName: album.name,
            });
        }
    }

    onMount(() => {
        loadAlbumData();
    });

    // Reload when albumId changes
    $: albumId, loadAlbumData();

    import { contextMenu } from "$lib/stores/ui";
    import { deleteAlbum } from "$lib/api/tauri";
    import {
        pinnedItems,
        pinItem,
        unpinItem,
        isPinned,
    } from "$lib/stores/pinned";
    import { setCustomArtwork } from "$lib/stores/customArtwork";

    let showArtPopup = false;
    import { isInListenLater, toggleListenLater } from "$lib/stores/listen-later";
    import { likedAlbumIds, toggleAlbumLike } from "$lib/stores/liked-albums";
    import { likedTrackIds } from "$lib/stores/liked";

    let showOnlyLiked = false;

    $: likedTracks = tracks.filter(t => $likedTrackIds.has(t.id));
    $: displayTracks = showOnlyLiked ? likedTracks : tracks;
    $: displayGroupedTracks = showOnlyLiked ? groupTracksByDisc(likedTracks) : groupedTracks;

    // Computed years: prefer DB values, fallback to MB response
    $: displayOriginalYear = album?.original_year ?? (mbRelease?.original_year ? parseInt(mbRelease.original_year) : null);
    $: displayEditionYear = album?.year ?? (mbRelease?.year ? parseInt(mbRelease.year) : null);
    $: displayYear = displayOriginalYear || displayEditionYear;

    function handleContextMenu(e: MouseEvent) {
        if (!album) return;
        e.preventDefault();
        const pinned = isPinned("album", album.id, $pinnedItems);
        const savedForLater = isInListenLater(album.id);
        contextMenu.set({
            visible: true,
            x: e.clientX,
            y: e.clientY,
            items: [
                {
                    label: $_('contextMenu.addToQueue'),
                    icon: `<svg viewBox="0 0 24 24" fill="currentColor" width="18" height="18"><path d="M3 6h18v2H3V6zm0 5h18v2H3v-2zm0 5h12v2H3v-2zM17 13v6h6v-6h-6zm3 4.5L18 15l1.5-1.5L22 16l-2 2z"/></svg>`,
                    action: async () => {
                        try {
                            const tracks = await getTracksByAlbum(album!.id);
                            if (tracks.length === 0) {
                                addToast(
                                    $_('queue.noTracksToAdd', { default: 'No tracks found for this album' }),
                                    'warning',
                                );
                                return;
                            }
                            appendToQueueEnd(tracks);
                            addToast(
                                $_('queue.albumAddedToEnd', {
                                    values: { count: tracks.length, name: album!.name },
                                    default: `Added ${tracks.length} tracks from "${album!.name}" to end of queue`,
                                }),
                                'success',
                            );
                        } catch (err) {
                            console.error('Failed to add album to queue:', err);
                            addToast(
                                $_('queue.addToQueueFailed', { default: 'Failed to add album to queue' }),
                                'error',
                            );
                        }
                    },
                },
                {
                    label: pinned ? $_('contextMenu.unpinFromTop') : $_('contextMenu.pinToTop'),
                    icon: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18"><path d="M12 2L4.5 9L9 9L9 22L15 22L15 9L19.5 9L12 2Z"/></svg>`,
                    action: () => {
                        if (pinned) {
                            unpinItem("album", album!.id);
                        } else {
                            pinItem("album", album!.id);
                        }
                    },
                },
                {
                    label: savedForLater
                        ? $_('contextMenu.removeFromListenLater', { default: 'Quitar de Escuchar más tarde' })
                        : $_('contextMenu.listenLater', { default: 'Escuchar más tarde' }),
                    action: async () => {
                        const wasSaved = isInListenLater(album!.id);
                        await toggleListenLater(album!.id);
                        addToast(
                            wasSaved
                                ? $_('listenLater.removedToast', { default: 'Álbum eliminado de Escuchar más tarde' })
                                : $_('listenLater.addedToast', { default: 'Álbum añadido a Escuchar más tarde' }),
                            'success',
                        );
                    },
                },
                { type: "separator" },
                {
                    label: $_('contextMenu.changeArtwork'),
                    submenu: [
                        {
                            label: $_('contextMenu.fromFile'),
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
                                            const result =
                                                reader.result as string;
                                            setCustomArtwork(
                                                "album",
                                                album!.id,
                                                result,
                                            );
                                            addToast(
                                                "Album artwork updated",
                                                "success",
                                            );
                                        };
                                        reader.readAsDataURL(file);
                                    }
                                };
                                input.click();
                            },
                        },
                        {
                            label: $_('contextMenu.fromUrl'),
                            action: async () => {
                                const url = await prompt("Enter image URL:", {
                                    title: "Change Artwork",
                                    placeholder:
                                        "https://example.com/image.jpg",
                                });
                                if (url && url.trim()) {
                                    setCustomArtwork(
                                        "album",
                                        album!.id,
                                        url.trim(),
                                    );
                                    addToast(
                                        "Album artwork updated",
                                        "success",
                                    );
                                }
                            },
                        },
                    ],
                },
                { type: "separator" },
                {
                    label: "Delete Album",
                    danger: true,
                    action: async () => {
                        const confirmed = await confirm(
                            `Are you sure you want to delete the album "${album!.name}"? This will delete all songs in this album from your computer.`,
                            {
                                title: "Delete Album",
                                confirmLabel: "Delete",
                                danger: true,
                            },
                        );

                        if (!confirmed) return;

                        try {
                            await deleteAlbum(album!.id);
                            await loadLibrary(); // Refresh library
                            goToAlbums(); // Go back to albums list
                        } catch (error) {
                            console.error("Failed to delete album:", error);
                        }
                    },
                },
            ],
        });
    }
</script>

<div class="album-detail">
    {#if loading}
        <div class="loading">
            <div class="spinner"></div>
            <span>{$_('album.loading')}</span>
        </div>
    {:else if album && shouldShowAlbum}
        <header
            class="album-header"
            on:contextmenu={handleContextMenu}
            role="banner"
            aria-label="Album Header"
        >
            <button
                class="back-btn"
                on:click={goBack}
                aria-label="Back"
            >
                <svg
                    viewBox="0 0 24 24"
                    fill="currentColor"
                    width="24"
                    height="24"
                >
                    <path
                        d="M20 11H7.83l5.59-5.59L12 4l-8 8 8 8 1.41-1.41L7.83 13H20v-2z"
                    />
                </svg>
            </button>
            <div class="album-cover">
                {#if getAlbumCoverFromTracks(album.id)}
                    <img
                        src={getAlbumCoverFromTracks(album.id)}
                        alt={album.name}
                        decoding="async"
                        on:click={() => (showArtPopup = true)}
                        class="clickable"
                    />
                {:else}
                    <div class="album-cover-placeholder">
                        <svg
                            viewBox="0 0 24 24"
                            fill="currentColor"
                            width="64"
                            height="64"
                        >
                            <path
                                d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 14.5c-2.49 0-4.5-2.01-4.5-4.5S9.51 7.5 12 7.5s4.5 2.01 4.5 4.5-2.01 4.5-4.5 4.5zm0-5.5c-.55 0-1 .45-1 1s.45 1 1 1 1-.45 1-1-.45-1-1-1z"
                            />
                        </svg>
                    </div>
                {/if}
            </div>
            <div class="album-info">
                <span class="album-type">{$_('album.type')}</span>
                <h1 class="album-title">{album.name}</h1>
                <div class="album-meta">
                    <button
                        class="album-artist link"
                        on:click={() => {
                            if (album)
                                goToArtistDetail(
                                    album.artist || "Unknown Artist",
                                );
                        }}
                        title="Go to artist"
                    >
                        {album.artist || "Unknown Artist"}
                    </button>
                    {#if displayYear}
                        <span class="separator">•</span>
                        <span class="album-year-display">{displayOriginalYear || displayEditionYear}</span>
                        {#if displayEditionYear && displayOriginalYear && displayEditionYear !== displayOriginalYear}
                            <span class="album-edition-display">({$_('album.edition', { default: 'Edition' })}: {displayEditionYear})</span>
                        {/if}
                    {/if}
                    <span class="separator">•</span>
                    <span>{$_('album.songs', { values: { count: tracks.length } })}</span>
                    <span class="separator">•</span>
                    <span>{formatDuration(totalDuration)}</span>
                    {#if albumAudioInfo.format || albumAudioInfo.sampleRate || albumAudioInfo.bitDepth}
                        <span class="separator">•</span>
                        <div class="album-audio-meta">
                            {#if albumAudioInfo.format}
                                <span class="album-audio-chip format-chip">{albumAudioInfo.format}</span>
                            {/if}
                            {#if albumAudioInfo.sampleRate}
                                <span class="album-audio-chip">{formatSampleRate(albumAudioInfo.sampleRate)}</span>
                            {/if}
                            {#if albumAudioInfo.bitDepth}
                                <span class="album-audio-chip">{albumAudioInfo.bitDepth}bit</span>
                            {/if}
                        </div>
                    {/if}
                </div>
                <div class="album-actions">
                    <button
                        class="btn-primary play-all-btn"
                        on:click={handlePlayAll}
                    >
                        <svg
                            viewBox="0 0 24 24"
                            fill="currentColor"
                            width="24"
                            height="24"
                        >
                            <path d="M8 5v14l11-7z" />
                        </svg>
                        {$_('album.play')}
                    </button>

                    <button
                        class="btn-info"
                        type="button"
                        on:click={() => (infoOpen = true)}
                        title={$_('album.infoTitle')}
                        aria-label={$_('album.infoTitle')}
                    >
                        <svg viewBox="0 0 24 24" width="22" height="22"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="2"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                        >
                            <circle cx="12" cy="12" r="10"/>
                            <line x1="12" y1="16" x2="12" y2="12"/>
                            <line x1="12" y1="8" x2="12.01" y2="8"/>
                        </svg>
                    </button>

                    <button
                        class="btn-like-album"
                        class:liked={$likedAlbumIds.has(albumId)}
                        on:click={() => toggleAlbumLike(albumId)}
                        title={$likedAlbumIds.has(albumId) ? "Unlike album" : "Like album"}
                    >
                        <svg viewBox="0 0 24 24" width="22" height="22"
                            fill={$likedAlbumIds.has(albumId) ? "currentColor" : "none"}
                            stroke="currentColor" stroke-width="2"
                        >
                            <path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"/>
                        </svg>
                    </button>

                    {#if likedTracks.length > 0 && likedTracks.length < tracks.length}
                        <button
                            class="btn-filter-liked"
                            class:active={showOnlyLiked}
                            on:click={() => showOnlyLiked = !showOnlyLiked}
                            title={showOnlyLiked ? "Show all tracks" : "Show only liked tracks"}
                        >
                            <svg viewBox="0 0 24 24" width="18" height="18"
                                fill="currentColor" stroke="none"
                            >
                                <path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"/>
                            </svg>
                            <span>{likedTracks.length}</span>
                        </button>
                    {/if}

                    {#if hasDownloadable}
                        <button
                            class="btn-secondary download-btn"
                            on:click={handleDownloadAll}
                            disabled={isDownloading ||
                                (!hasDownloadable && !allDownloaded) ||
                                allDownloaded}
                            class:downloaded={allDownloaded}
                        >
                            {#if isDownloading}
                                <div class="spinner-sm"></div>
                                <span>{downloadProgress}</span>
                            {:else if allDownloaded}
                                <svg
                                    viewBox="0 0 24 24"
                                    fill="currentColor"
                                    width="24"
                                    height="24"
                                >
                                    <path
                                        d="M19 9h-4V3H9v6H5l7 7 7-7zM5 18v2h14v-2H5z"
                                    />
                                </svg>
                                <span>{$_('album.downloaded')}</span>
                            {:else}
                                <svg
                                    viewBox="0 0 24 24"
                                    fill="currentColor"
                                    width="24"
                                    height="24"
                                >
                                    <path
                                        d="M19 9h-4V3H9v6H5l7 7 7-7zM5 18v2h14v-2H5z"
                                    />
                                </svg>
                                <span>{$_('album.download')}</span>
                            {/if}
                        </button>
                    {/if}
                </div>
            </div>
        </header>

        <!-- MusicBrainz release info bar -->
        {#if mbReleaseLoading}
            <div class="mb-info-bar mb-info-loading">
                <span class="mb-info-spinner"></span>
                <span class="mb-info-hint">{$_('album.fetchingReleaseInfo')}</span>
            </div>
        {:else if mbRelease && (mbRelease.year || mbRelease.original_year || mbRelease.label || mbRelease.country || mbRelease.release_type)}
            <div class="mb-info-bar">
                {#if mbRelease.release_type}
                    <span class="mb-chip type-chip"
                        >{mbRelease.release_type}</span
                    >
                {/if}
                {#if mbRelease.original_year}
                    <span class="mb-chip">{mbRelease.original_year}</span>
                {/if}
                {#if mbRelease.year && mbRelease.year !== mbRelease.original_year}
                    <span class="mb-chip">{$_('album.edition', { default: 'Edition' })}: {mbRelease.year}</span>
                {/if}
                {#if mbRelease.label}
                    <span class="mb-chip">
                        <svg
                            viewBox="0 0 24 24"
                            fill="currentColor"
                            width="12"
                            height="12"
                            style="opacity:0.6;flex-shrink:0"
                        >
                            <path
                                d="M20 4H4c-1.1 0-2 .9-2 2v12c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V6c0-1.1-.9-2-2-2zm0 4l-8 5-8-5V6l8 5 8-5v2z"
                            />
                        </svg>
                        {mbRelease.label}
                    </span>
                {/if}
                {#if mbRelease.country}
                    <span class="mb-chip">{mbRelease.country}</span>
                {/if}
                <span class="mb-source-label">{$_('album.viaMusicBrainz')}</span>
            </div>
        {/if}

        <section class="track-list-section">
            {#if displayGroupedTracks.length > 1}
                {#each displayGroupedTracks as group}
                    <div class="disc-group">
                        <div class="disc-header">
                            <span class="disc-icon">
                                <svg
                                    viewBox="0 0 24 24"
                                    fill="currentColor"
                                    width="16"
                                    height="16"
                                >
                                    <path
                                        d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 18c-4.41 0-8-3.59-8-8s3.59-8 8-8 8 3.59 8 8-3.59 8-8 8zm0-13c-2.76 0-5 2.24-5 5s2.24 5 5 5 5-2.24 5-5-2.24-5-5-5zm0 8c-1.66 0-3-1.34-3-3s1.34-3 3-3 3 1.34 3 3-1.34 3-3 3z"
                                    />
                                </svg>
                            </span>
                            <h3>{$_('album.disc', { values: { number: group.disc } })}</h3>
                        </div>
                        <TrackList
                            tracks={group.tracks}
                            showAlbum={false}
                            disableVirtualScroll={true}
                            playbackContext={{
                                type: "album",
                                albumId,
                                displayName: album?.name,
                            }}
                            queueTracks={displayTracks}
                        />
                    </div>
                {/each}
            {:else}
                <TrackList
                    tracks={displayTracks}
                    showAlbum={false}
                    playbackContext={{
                        type: "album",
                        albumId,
                        displayName: album?.name,
                    }}
                    queueTracks={displayTracks}
                />
            {/if}
        </section>
    {:else}
        <div class="not-found">
            <h2>{$_('album.notFound')}</h2>
            <button class="btn-secondary" on:click={goToAlbums}>
                {$_('album.backToAlbums')}
            </button>
        </div>
    {/if}
</div>

{#if showArtPopup && album}
    <div
        class="art-popup-overlay"
        on:click={() => (showArtPopup = false)}
        on:keydown={(e) => e.key === 'Escape' && (showArtPopup = false)}
        role="dialog"
        aria-label="Album artwork"
        tabindex="-1"
    >
        <div class="art-popup-content" on:click|stopPropagation>
            <img
                src={getAlbumCoverFromTracks(album.id)}
                alt={album.name}
                class="art-popup-img"
            />
            <button class="art-popup-close" on:click={() => (showArtPopup = false)}>Close</button>
        </div>
    </div>
{/if}

{#if album && infoOpen}
    <AlbumInfoModal
        {album}
        bind:open={infoOpen}
        on:close={() => (infoOpen = false)}
    />
{/if}

<style>
    .album-detail {
        display: flex;
        flex-direction: column;
        height: 100%;
    }

    .loading,
    .not-found {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        height: 100%;
        gap: var(--spacing-md);
        color: var(--text-secondary);
    }

    .spinner {
        width: 32px;
        height: 32px;
        border: 3px solid var(--bg-highlight);
        border-top-color: var(--accent-primary);
        border-radius: 50%;
        animation: spin 1s linear infinite;
    }

    @keyframes spin {
        to {
            transform: rotate(360deg);
        }
    }

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

    .back-btn {
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

    .back-btn:hover {
        background-color: rgba(0, 0, 0, 0.7);
        transform: scale(1.1);
    }

    .album-cover {
        width: 232px;
        height: 232px;
        border-radius: var(--radius-sm);
        overflow: hidden;
        flex-shrink: 0;
        box-shadow: var(--shadow-lg);
    }

    .album-cover img {
        width: 100%;
        height: 100%;
        object-fit: cover;
    }

    .album-cover img.clickable {
        cursor: pointer;
    }

    .art-popup-overlay {
        position: fixed;
        inset: 0;
        z-index: 9999;
        background: rgba(0, 0, 0, 0.85);
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        animation: fadeIn 0.15s ease;
    }

    .art-popup-content {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 16px;
        max-width: 90vw;
        max-height: 90vh;
    }

    .art-popup-img {
        max-width: 80vmin;
        max-height: 80vmin;
        border-radius: var(--radius-sm);
        box-shadow: 0 8px 40px rgba(0, 0, 0, 0.6);
        object-fit: contain;
    }

    .art-popup-close {
        background: none;
        border: none;
        color: var(--text-subdued);
        font-size: 14px;
        cursor: pointer;
        padding: 8px 16px;
    }

    .art-popup-close:hover {
        color: var(--text-primary);
    }

    @keyframes fadeIn {
        from { opacity: 0; }
        to { opacity: 1; }
    }

    .album-cover-placeholder {
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

    .album-info {
        display: flex;
        flex-direction: column;
        justify-content: flex-end;
        min-width: 0;
    }

    .album-type {
        font-size: 0.75rem;
        font-weight: 600;
        text-transform: uppercase;
        color: var(--text-primary);
    }

    .album-title {
        font-size: 3rem;
        font-weight: 700;
        line-height: 1.1;
        margin: var(--spacing-sm) 0;
        color: var(--text-primary);
    }

    .album-meta {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        font-size: 0.875rem;
        color: var(--text-secondary);
        margin-bottom: var(--spacing-lg);
    }

    .album-audio-meta {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        flex-wrap: wrap;
    }

    .album-audio-chip {
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

    .album-audio-chip.format-chip {
        color: var(--accent-primary);
        border-color: color-mix(in srgb, var(--accent-primary), transparent 65%);
        background: color-mix(in srgb, var(--accent-primary), transparent 88%);
    }

    .album-artist {
        font-weight: 600;
        color: var(--text-primary);
        background: none;
        border: none;
        padding: 0;
        cursor: pointer;
    }

    .album-artist:hover {
        text-decoration: underline;
    }

    .separator {
        color: var(--text-subdued);
    }

    .album-edition-display {
        color: var(--text-subdued);
        font-size: 0.85em;
        margin-left: 4px;
    }

    .album-actions {
        display: flex;
        gap: var(--spacing-md);
        align-items: center;
    }

    .btn-info {
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
    .btn-info:hover {
        background: var(--bg-highlight, #3e3e3e);
        color: var(--accent-primary, #1DB954);
    }
    .btn-info:active { transform: scale(0.92); }

    .btn-like-album {
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

    .btn-like-album:hover {
        color: var(--accent-primary);
        border-color: var(--accent-primary);
    }

    .btn-like-album.liked {
        color: var(--accent-primary);
        border-color: var(--accent-primary);
    }

    .btn-filter-liked {
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

    .btn-filter-liked:hover {
        color: var(--accent-primary);
        border-color: var(--accent-primary);
    }

    .btn-filter-liked.active {
        color: var(--accent-primary);
        border-color: var(--accent-primary);
        background: color-mix(in srgb, var(--accent-primary), transparent 88%);
    }

    .play-all-btn {
        font-size: 1rem;
        padding: var(--spacing-sm) var(--spacing-xl);
    }

    .track-list-section {
        flex: 1;
        overflow-y: auto;
        min-height: 0;
    }

    .btn-secondary {
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

    .btn-secondary:hover:not(:disabled) {
        border-color: var(--text-primary);
        transform: scale(1.05);
    }

    .btn-secondary.downloaded {
        border-color: var(--accent-primary);
        color: var(--accent-primary);
        cursor: default;
    }

    .btn-secondary.downloaded:hover {
        transform: none;
    }

    .btn-secondary:disabled {
        opacity: 0.7;
        cursor: not-allowed;
    }

    .spinner-sm {
        width: 16px;
        height: 16px;
        border: 2px solid var(--bg-highlight);
        border-top-color: var(--text-primary);
        border-radius: 50%;
        animation: spin 1s linear infinite;
    }

    .disc-group {
        margin-bottom: var(--spacing-lg);
    }

    .disc-header {
        display: flex;
        align-items: center;
        gap: var(--spacing-md);
        padding: var(--spacing-md) var(--spacing-xl);
        background: transparent;
        color: var(--text-primary);
        font-size: 1rem;
        font-weight: 600;
        text-transform: none;
        letter-spacing: normal;
        border: none;
        margin-top: var(--spacing-lg);
        margin-bottom: var(--spacing-xs);
    }

    .disc-icon {
        display: flex;
        align-items: center;
        justify-content: center;
        color: var(--text-secondary);
        width: 24px; /* Align with track number width roughly */
    }

    .disc-icon {
        display: flex;
        align-items: center;
        opacity: 0.7;
    }

    .disc-header h3 {
        margin: 0;
        font-size: inherit;
        font-weight: inherit;
    }

    /* ── MusicBrainz info bar ── */
    .mb-info-bar {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: 8px;
        padding: 8px var(--spacing-lg);
        border-bottom: 1px solid var(--border-color);
        min-height: 38px;
    }

    .mb-info-loading {
        opacity: 0.5;
    }

    .mb-info-spinner {
        width: 14px;
        height: 14px;
        border: 2px solid rgba(255, 255, 255, 0.15);
        border-top-color: var(--accent-primary);
        border-radius: 50%;
        animation: spin 1s linear infinite;
        flex-shrink: 0;
    }

    .mb-info-hint {
        font-size: 0.75rem;
        color: var(--text-subdued);
    }

    .mb-chip {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        background: rgba(255, 255, 255, 0.06);
        border: 1px solid rgba(255, 255, 255, 0.1);
        border-radius: var(--radius-full);
        padding: 2px 10px;
        font-size: 0.75rem;
        font-weight: 600;
        color: var(--text-secondary);
        white-space: nowrap;
    }

    .type-chip {
        background: rgba(var(--accent-primary-rgb, 30 215 96) / 0.1);
        border-color: rgba(var(--accent-primary-rgb, 30 215 96) / 0.3);
        color: var(--accent-primary);
    }

    .mb-source-label {
        font-size: 0.65rem;
        color: var(--text-subdued);
        opacity: 0.45;
        text-transform: uppercase;
        letter-spacing: 1px;
        margin-left: auto;
    }

    /* ── Mobile ── */
    @media (max-width: 768px) {
        .album-header {
            flex-direction: column;
            align-items: center;
            text-align: center;
            padding: calc(var(--safe-area-top) + var(--spacing-md))
                var(--spacing-md) var(--spacing-md);
            gap: var(--spacing-md);
        }

        .back-btn {
            top: calc(var(--safe-area-top) + var(--spacing-sm));
            left: var(--spacing-sm);
        }

        .album-cover {
            width: 160px;
            height: 160px;
        }

        .album-info {
            align-items: center;
        }

        .album-title {
            font-size: 1.5rem;
            word-break: break-word;
        }

        .album-meta {
            flex-wrap: wrap;
            justify-content: center;
            margin-bottom: var(--spacing-md);
        }

        .album-actions {
            flex-wrap: wrap;
            justify-content: center;
        }

        .play-all-btn,
        .btn-secondary {
            padding: var(--spacing-sm) var(--spacing-lg);
            font-size: 0.875rem;
            min-height: 44px;
        }

        .track-list-section {
            padding-bottom: calc(
                var(--mobile-bottom-inset) + var(--spacing-md)
            );
        }
    }
</style>
