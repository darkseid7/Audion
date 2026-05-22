<script lang="ts">
    import type { Album } from "$lib/api/tauri";
    import { goToAlbumDetail, goToArtistDetail } from "$lib/stores/view";
    import {
        loadLibrary,
        getAlbumCoverFromTracks,
        loadMoreAlbums,
        tracks,
    } from "$lib/stores/library";
    import { contextMenu } from "$lib/stores/ui";
    import { deleteAlbum, getTracksByAlbum } from "$lib/api/tauri";
    import {
        playTracks,
        currentAlbumId,
        isPlaying,
        togglePlay,
    } from "$lib/stores/player";
    import VirtualizedGrid from "./Virtualizedgrid.svelte";
    import MediaCard from "./MediaCard.svelte";
    import { confirm, prompt } from "$lib/stores/dialogs";
    import { onDestroy } from "svelte";
    import { saveScroll, getScroll } from "$lib/stores/scrollMemory";
    import {
        pinnedItems,
        pinItem,
        unpinItem,
        isPinned,
    } from "$lib/stores/pinned";
    import { setCustomArtwork } from "$lib/stores/customArtwork";
    import { addToast } from "$lib/stores/toast";
    import { isInListenLater, toggleListenLater } from "$lib/stores/listen-later";
    import { _ } from "svelte-i18n";

    let currentScrollTop = getScroll("albums");

    onDestroy(() => {
        saveScroll("albums", currentScrollTop);
    });

    export let albums: Album[] = [];

    // Playback state
    $: playingAlbumId = $currentAlbumId;
    $: playing = $isPlaying;
    $: pausedAlbumId = !playing ? playingAlbumId : null;

    type AlbumAudioInfo = {
        format: string | null;
        bitrate: number | null;
        sampleRate: number | null;
        bitDepth: number | null;
    };

    function normalizeFormat(format: string | null | undefined): string | null {
        if (!format) return null;
        const upper = format.toUpperCase();
        if (upper.includes("MPEG") || upper.includes("MP3")) return "MP3";
        if (upper.includes("HI_RES") || upper.includes("HIRES")) return "HI-RES";
        if (upper.includes("LOSSLESS")) return "LOSSLESS";
        return upper;
    }

    function parseSampleRateFromMetadata(metadataJson: string | null | undefined): number | null {
        if (!metadataJson) return null;

        try {
            const parsed = JSON.parse(metadataJson) as Record<string, unknown>;

            const directSampleRate = parsed.__sample_rate_hz;
            if (typeof directSampleRate === "number") return directSampleRate;

            const values = Object.values(parsed);

            for (const value of values) {
                if (!value) continue;

                if (typeof value === "number") {
                    if (value >= 4000 && value <= 768000) return value;
                    continue;
                }

                const text = String(value);

                const khzMatch = text.match(/(\d+(?:\.\d+)?)\s*k\s*hz/i);
                if (khzMatch) {
                    return Math.round(parseFloat(khzMatch[1]) * 1000);
                }

                const hzMatch = text.match(/\b(\d{4,6})\s*hz\b/i);
                if (hzMatch) {
                    return Number(hzMatch[1]);
                }
            }
        } catch {
            return null;
        }

        return null;
    }

    function parseBitDepthFromMetadata(metadataJson: string | null | undefined): number | null {
        if (!metadataJson) return null;

        try {
            const parsed = JSON.parse(metadataJson) as Record<string, unknown>;

            const directBitDepth = parsed.__bit_depth;
            if (typeof directBitDepth === "number") return directBitDepth;

            for (const value of Object.values(parsed)) {
                if (!value) continue;
                if (typeof value === "number") continue;

                const text = String(value);
                const bitMatch = text.match(/\b(16|24|32)\s*bit\b/i);
                if (bitMatch) {
                    return Number(bitMatch[1]);
                }
            }
        } catch {
            return null;
        }

        return null;
    }

    function formatSampleRate(sampleRate: number | null): string {
        if (!sampleRate) return "--";
        return `${(sampleRate / 1000).toFixed(sampleRate % 1000 === 0 ? 0 : 1)}kHz`;
    }

    function formatAlbumAudioLine(audioInfo: AlbumAudioInfo | undefined): string {
        if (!audioInfo) return "--";

        const parts: string[] = [];
        if (audioInfo.format) parts.push(audioInfo.format);
        if (audioInfo.sampleRate) parts.push(formatSampleRate(audioInfo.sampleRate));
        if (audioInfo.bitDepth) {
            parts.push(`${audioInfo.bitDepth}bit`);
        } else if (audioInfo.bitrate) {
            parts.push(`${audioInfo.bitrate}kbps`);
        }

        return parts.length ? parts.join(" ") : "--";
    }

    function buildAlbumAudioInfoMap(libraryTracks: typeof $tracks): Map<number, AlbumAudioInfo> {
        const map = new Map<number, AlbumAudioInfo>();
        const perAlbum = new Map<
            number,
            {
                formatCounts: Map<string, number>;
                bestBitrate: number | null;
                bestSampleRate: number | null;
                bestBitDepth: number | null;
            }
        >();

        for (const track of libraryTracks) {
            if (!track.album_id) continue;

            if (!perAlbum.has(track.album_id)) {
                perAlbum.set(track.album_id, {
                    formatCounts: new Map<string, number>(),
                    bestBitrate: null,
                    bestSampleRate: null,
                    bestBitDepth: null,
                });
            }

            const albumStats = perAlbum.get(track.album_id)!;

            const normalizedFormat = normalizeFormat(track.format);
            if (normalizedFormat) {
                albumStats.formatCounts.set(
                    normalizedFormat,
                    (albumStats.formatCounts.get(normalizedFormat) || 0) + 1,
                );
            }

            if (typeof track.bitrate === "number") {
                albumStats.bestBitrate = Math.max(albumStats.bestBitrate || 0, track.bitrate);
            }

            const parsedSampleRate = parseSampleRateFromMetadata(track.metadata_json);
            if (typeof parsedSampleRate === "number") {
                albumStats.bestSampleRate = Math.max(
                    albumStats.bestSampleRate || 0,
                    parsedSampleRate,
                );
            }

            const parsedBitDepth = parseBitDepthFromMetadata(track.metadata_json);
            if (typeof parsedBitDepth === "number") {
                albumStats.bestBitDepth = Math.max(
                    albumStats.bestBitDepth || 0,
                    parsedBitDepth,
                );
            }
        }

        for (const [albumId, stats] of perAlbum.entries()) {
            let primaryFormat: string | null = null;
            let maxCount = -1;

            for (const [format, count] of stats.formatCounts.entries()) {
                if (count > maxCount) {
                    primaryFormat = format;
                    maxCount = count;
                }
            }

            map.set(albumId, {
                format: primaryFormat,
                bitrate: stats.bestBitrate,
                sampleRate: stats.bestSampleRate,
                bitDepth: stats.bestBitDepth,
            });
        }

        return map;
    }

    $: albumAudioInfoById = buildAlbumAudioInfoMap($tracks);

    // Sorting/Pinning logic
    $: sortedAlbums = [...albums].sort((a, b) => {
        const aPinned = isPinned("album", a.id, $pinnedItems);
        const bPinned = isPinned("album", b.id, $pinnedItems);
        if (aPinned && !bPinned) return -1;
        if (!aPinned && bPinned) return 1;
        return 0; // Maintain original order if both same
    });

    // Image error cache
    let failedImages = new Set<string>();
    const MAX_FAILED_IMAGES = 200;

    function handleImageError(e: Event) {
        const img = e.target as HTMLImageElement;
        if (failedImages.size >= MAX_FAILED_IMAGES) {
            const toKeep = Array.from(failedImages).slice(
                -MAX_FAILED_IMAGES / 2,
            );
            failedImages.clear();
            toKeep.forEach((s) => failedImages.add(s));
        }
        failedImages.add(img.src);
        failedImages = failedImages;
    }

    // Playback
    async function playAlbum(album: Album) {
        if (pausedAlbumId === album.id) {
            togglePlay();
            return;
        }
        if (playingAlbumId === album.id && playing) return;
        try {
            const tracks = await getTracksByAlbum(album.id);
            if (tracks.length > 0) {
                playTracks(tracks, 0, {
                    type: "album",
                    albumId: album.id,
                    displayName: album.name,
                });
            }
        } catch (err) {
            console.error("Failed to load tracks for album:", err);
        }
    }

    // Navigation
    function handleAlbumClick(album: Album, e: MouseEvent) {
        if ((e.target as HTMLElement).closest("[data-mediacard-play]")) return;
        goToAlbumDetail(album.id);
    }

    async function handleAlbumContextMenu(album: Album, e: MouseEvent) {
        const pinned = isPinned("album", album.id, $pinnedItems);
        const savedForLater = isInListenLater(album.id);
        contextMenu.set({
            visible: true,
            x: e.clientX,
            y: e.clientY,
            items: [
                {
                    label: "Play",
                    action: () => playAlbum(album),
                },
                {
                    label: savedForLater
                        ? $_("contextMenu.removeFromListenLater", { default: "Quitar de Escuchar más tarde" })
                        : $_("contextMenu.listenLater", { default: "Escuchar más tarde" }),
                    action: async () => {
                        const wasSaved = isInListenLater(album.id);
                        await toggleListenLater(album.id);
                        addToast(
                            wasSaved
                                ? $_("listenLater.removedToast", { default: "Álbum eliminado de Escuchar más tarde" })
                                : $_("listenLater.addedToast", { default: "Álbum añadido a Escuchar más tarde" }),
                            "success",
                        );
                    },
                },
                {
                    label: pinned ? "Unpin from Top" : "Pin to Top",
                    icon: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18"><path d="M12 2L4.5 9L9 9L9 22L15 22L15 9L19.5 9L12 2Z"/></svg>`,
                    action: () => {
                        if (pinned) {
                            unpinItem("album", album.id);
                        } else {
                            pinItem("album", album.id);
                        }
                    },
                },
                { type: "separator" },
                {
                    label: "Change Artwork",
                    submenu: [
                        {
                            label: "From File",
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
                                                album.id,
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
                            label: "From URL",
                            action: async () => {
                                const url = await prompt("Enter image URL:", {
                                    title: "Change Artwork",
                                    placeholder:
                                        "https://example.com/image.jpg",
                                });
                                if (url && url.trim()) {
                                    setCustomArtwork(
                                        "album",
                                        album.id,
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
                            `Are you sure you want to delete the album "${album.name}"? This will delete all songs in this album from your computer.`,
                            {
                                title: "Delete Album",
                                confirmLabel: "Delete",
                                danger: true,
                            },
                        );
                        if (!confirmed) return;
                        try {
                            await deleteAlbum(album.id);
                            await loadLibrary();
                        } catch (err) {
                            console.error("Failed to delete album:", err);
                        }
                    },
                },
            ],
        });
    }

    async function handleLoadMore(): Promise<boolean> {
        return await loadMoreAlbums();
    }

    const emptyState = {
        icon: `<svg viewBox="0 0 24 24" fill="currentColor"><path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 14.5c-2.49 0-4.5-2.01-4.5-4.5S9.51 7.5 12 7.5s4.5 2.01 4.5 4.5-2.01 4.5-4.5 4.5zm0-5.5c-.55 0-1 .45-1 1s.45 1 1 1 1-.45 1-1-.45-1-1-1z"/></svg>`,
        title: "No albums found",
        description: "Add a music folder to see your albums",
    };
</script>

<VirtualizedGrid
    items={sortedAlbums}
    bind:currentScrollTop
    initialScrollTop={currentScrollTop}
    onItemClick={handleAlbumClick}
    onItemContextMenu={handleAlbumContextMenu}
    onLoadMore={handleLoadMore}
    emptyStateConfig={emptyState}
    cardHeightDesktop={285}
    cardHeightMobile={235}
    let:item={album}
>
    {@const cover = getAlbumCoverFromTracks(album.id)}
    {@const isNowPlaying = playingAlbumId === album.id && playing}
    {@const isPaused = pausedAlbumId === album.id}
    {@const audioInfo = albumAudioInfoById.get(album.id)}

    <MediaCard
        {isNowPlaying}
        {isPaused}
        isPinned={isPinned("album", album.id, $pinnedItems)}
        playTooltip="Play album"
        resumeTooltip="Resume album"
        pauseTooltip="Pause"
        ariaLabel={album.name}
        primaryText={album.name}
        secondaryText={album.artist || "Unknown Artist"}
        secondaryAction={album.artist
            ? () => goToArtistDetail(album.artist!)
            : null}
        on:play={() => playAlbum(album)}
        on:pause={togglePlay}
    >
        <svelte:fragment slot="cover">
            {#if cover && !failedImages.has(cover)}
                <img
                    src={cover}
                    alt={album.name}
                    loading="lazy"
                    decoding="async"
                    on:error={handleImageError}
                />
            {:else}
                <div class="placeholder">
                    <svg
                        viewBox="0 0 24 24"
                        fill="currentColor"
                        width="48"
                        height="48"
                        aria-hidden="true"
                    >
                        <path
                            d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 14.5c-2.49 0-4.5-2.01-4.5-4.5S9.51 7.5 12 7.5s4.5 2.01 4.5 4.5-2.01 4.5-4.5 4.5zm0-5.5c-.55 0-1 .45-1 1s.45 1 1 1 1-.45 1-1-.45-1-1-1z"
                        />
                    </svg>
                </div>
            {/if}
        </svelte:fragment>

        <svelte:fragment slot="extra-info">
            {#if audioInfo?.format || audioInfo?.sampleRate || audioInfo?.bitDepth || audioInfo?.bitrate}
                <div class="audio-chips">
                    {#if audioInfo?.format}
                        <span class="audio-chip format">{audioInfo.format}</span>
                    {/if}
                    {#if audioInfo?.sampleRate}
                        <span class="audio-chip">{formatSampleRate(audioInfo.sampleRate)}</span>
                    {/if}
                    {#if audioInfo?.bitDepth}
                        <span class="audio-chip">{audioInfo.bitDepth}bit</span>
                    {:else if audioInfo?.bitrate}
                        <span class="audio-chip">{audioInfo.bitrate}kbps</span>
                    {/if}
                </div>
            {/if}
        </svelte:fragment>
    </MediaCard>
</VirtualizedGrid>

<style>
    .placeholder {
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

    .audio-chips {
        display: flex;
        flex-wrap: wrap;
        gap: 4px;
        margin-top: 8px;
    }

    .audio-chip {
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

    .audio-chip.format {
        background: color-mix(in oklab, var(--accent-primary) 15%, transparent);
        color: var(--accent-primary);
        border-color: color-mix(in oklab, var(--accent-primary) 40%, transparent);
    }
</style>
