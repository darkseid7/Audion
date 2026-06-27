<script context="module" lang="ts">
    let _searchQuery = "";
    let _showOnlyFavorites = false;
</script>

<script lang="ts">
    import type { Album, Track } from "$lib/api/tauri";
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
        appendToQueueEnd,
        playNext,
    } from "$lib/stores/player";
    import VirtualizedGrid from "./Virtualizedgrid.svelte";
    import MediaCard from "./MediaCard.svelte";
    import { confirm, prompt } from "$lib/stores/dialogs";
    import { onDestroy, onMount } from "svelte";
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
    import { likedAlbumIds } from "$lib/stores/liked-albums";
    import { _ } from "svelte-i18n";

    let currentScrollTop = getScroll("albums");

    let searchQuery = _searchQuery;
    let showOnlyFavorites = _showOnlyFavorites;

    onDestroy(() => {
        saveScroll("albums", currentScrollTop);
        _searchQuery = searchQuery;
        _showOnlyFavorites = showOnlyFavorites;
    });

    export let albums: Album[] = [];

    type AlbumSortOption =
        | "artist-asc"
        | "artist-desc"
        | "year-desc"
        | "year-asc"
        | "added-desc"
        | "added-asc"
        | "name-asc"
        | "name-desc";

    const ALBUM_SORT_STORAGE_KEY = "audion_album_sort";

    function loadAlbumSort(): AlbumSortOption {
        if (typeof localStorage === "undefined") return "artist-asc";
        const saved = localStorage.getItem(ALBUM_SORT_STORAGE_KEY);
        if (
            saved === "artist-asc" ||
            saved === "artist-desc" ||
            saved === "year-desc" ||
            saved === "year-asc" ||
            saved === "added-desc" ||
            saved === "added-asc" ||
            saved === "name-asc" ||
            saved === "name-desc"
        ) {
            return saved;
        }
        return "artist-asc";
    }

    let albumSort: AlbumSortOption = loadAlbumSort();
    let isSortMenuOpen = false;

    const sortOptionLabels: Record<AlbumSortOption, string> = {
        "artist-asc": "Artista (A-Z)",
        "artist-desc": "Artista (Z-A)",
        "year-desc": "Año (más reciente)",
        "year-asc": "Año (más antiguo)",
        "added-desc": "Antigüedad (agregados recientemente)",
        "added-asc": "Antigüedad (agregados antes)",
        "name-asc": "Álbum (A-Z)",
        "name-desc": "Álbum (Z-A)",
    };

    const sortOptions: { value: AlbumSortOption; label: string }[] = [
        { value: "artist-asc", label: sortOptionLabels["artist-asc"] },
        { value: "artist-desc", label: sortOptionLabels["artist-desc"] },
        { value: "year-desc", label: sortOptionLabels["year-desc"] },
        { value: "year-asc", label: sortOptionLabels["year-asc"] },
        { value: "added-desc", label: sortOptionLabels["added-desc"] },
        { value: "added-asc", label: sortOptionLabels["added-asc"] },
        { value: "name-asc", label: sortOptionLabels["name-asc"] },
        { value: "name-desc", label: sortOptionLabels["name-desc"] },
    ];

    $: selectedSortLabel = sortOptionLabels[albumSort];

    onMount(() => {
        const handleGlobalPointerDown = (event: PointerEvent) => {
            const target = event.target as HTMLElement | null;
            if (!target) return;

            if (!target.closest(".sort-dropdown")) {
                isSortMenuOpen = false;
            }
        };

        const handleEscape = (event: KeyboardEvent) => {
            if (event.key === "Escape") {
                isSortMenuOpen = false;
            }
        };

        window.addEventListener("pointerdown", handleGlobalPointerDown);
        window.addEventListener("keydown", handleEscape);

        return () => {
            window.removeEventListener("pointerdown", handleGlobalPointerDown);
            window.removeEventListener("keydown", handleEscape);
        };
    });

    $: if (typeof localStorage !== "undefined") {
        localStorage.setItem(ALBUM_SORT_STORAGE_KEY, albumSort);
    }

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

    function extractYearFromMetadata(metadataJson: string | null | undefined): number | null {
        if (!metadataJson) return null;

        try {
            const parsed = JSON.parse(metadataJson) as Record<string, unknown>;

            for (const value of Object.values(parsed)) {
                if (value == null) continue;

                if (typeof value === "number") {
                    if (value >= 1000 && value <= 3000) return value;
                    continue;
                }

                const text = String(value);
                const match = text.match(/\b(19\d{2}|20\d{2}|2100)\b/);
                if (match) return Number(match[1]);
            }
        } catch {
            return null;
        }

        return null;
    }

    function parseDateToMs(dateValue: string | null | undefined): number | null {
        if (!dateValue) return null;
        const ms = Date.parse(dateValue);
        return Number.isNaN(ms) ? null : ms;
    }

    function compareNullableNumber(a: number | null, b: number | null): number {
        if (a == null && b == null) return 0;
        if (a == null) return 1;
        if (b == null) return -1;
        return a - b;
    }

    function compareText(a: string | null | undefined, b: string | null | undefined): number {
        return (a || "").localeCompare(b || "", undefined, {
            sensitivity: "base",
            numeric: true,
        });
    }

    function getAlbumArtistForSort(album: Album): string {
        if (album.artist && album.artist.trim()) return album.artist.trim();
        return "ZZZ_UNKNOWN_ARTIST";
    }

    function buildAlbumSortMetaMap(
        libraryTracks: Track[],
    ): Map<number, { year: number | null; newestAddedMs: number | null; oldestAddedMs: number | null }> {
        const meta = new Map<
            number,
            { year: number | null; newestAddedMs: number | null; oldestAddedMs: number | null }
        >();

        for (const track of libraryTracks) {
            if (!track.album_id) continue;

            if (!meta.has(track.album_id)) {
                meta.set(track.album_id, {
                    year: null,
                    newestAddedMs: null,
                    oldestAddedMs: null,
                });
            }

            const current = meta.get(track.album_id)!;

            const parsedYear = extractYearFromMetadata(track.metadata_json);
            if (parsedYear != null) {
                current.year = current.year == null ? parsedYear : Math.max(current.year, parsedYear);
            }

            const addedMs = parseDateToMs(track.date_added);
            if (addedMs != null) {
                current.newestAddedMs =
                    current.newestAddedMs == null
                        ? addedMs
                        : Math.max(current.newestAddedMs, addedMs);
                current.oldestAddedMs =
                    current.oldestAddedMs == null
                        ? addedMs
                        : Math.min(current.oldestAddedMs, addedMs);
            }
        }

        return meta;
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
    $: albumSortMetaById = buildAlbumSortMetaMap($tracks);

    // Filtering logic
    $: filteredAlbums = (() => {
        let result = albums;
        if (showOnlyFavorites) {
            result = result.filter((a) => $likedAlbumIds.has(a.id));
        }
        if (searchQuery.trim()) {
            const q = searchQuery.trim().toLowerCase();
            result = result.filter(
                (a) =>
                    (a.name && a.name.toLowerCase().includes(q)) ||
                    (a.artist && a.artist.toLowerCase().includes(q)),
            );
        }
        return result;
    })();

    // Sorting/Pinning logic
    $: sortedAlbums = [...filteredAlbums].sort((a, b) => {
        const aPinned = isPinned("album", a.id, $pinnedItems);
        const bPinned = isPinned("album", b.id, $pinnedItems);
        if (aPinned && !bPinned) return -1;
        if (!aPinned && bPinned) return 1;

        const aMeta = albumSortMetaById.get(a.id);
        const bMeta = albumSortMetaById.get(b.id);

        // Albums with no loaded tracks (no sort metadata) always go to the end
        const aHasMeta = aMeta != null;
        const bHasMeta = bMeta != null;
        if (aHasMeta && !bHasMeta) return -1;
        if (!aHasMeta && bHasMeta) return 1;
        if (!aHasMeta && !bHasMeta) return compareText(a.name, b.name);

        let result = 0;

        switch (albumSort) {
            case "artist-asc":
                result = compareText(getAlbumArtistForSort(a), getAlbumArtistForSort(b));
                break;
            case "artist-desc":
                result = compareText(getAlbumArtistForSort(b), getAlbumArtistForSort(a));
                break;
            case "year-desc":
                result = compareNullableNumber(
                    b.original_year ?? b.year ?? bMeta?.year ?? null,
                    a.original_year ?? a.year ?? aMeta?.year ?? null,
                );
                break;
            case "year-asc":
                result = compareNullableNumber(
                    a.original_year ?? a.year ?? aMeta?.year ?? null,
                    b.original_year ?? b.year ?? bMeta?.year ?? null,
                );
                break;
            case "added-desc":
                result = compareNullableNumber(
                    bMeta?.newestAddedMs ?? null,
                    aMeta?.newestAddedMs ?? null,
                );
                break;
            case "added-asc":
                result = compareNullableNumber(
                    aMeta?.oldestAddedMs ?? null,
                    bMeta?.oldestAddedMs ?? null,
                );
                break;
            case "name-desc":
                result = compareText(b.name, a.name);
                break;
            case "name-asc":
            default:
                result = compareText(a.name, b.name);
                break;
        }

        if (result !== 0) return result;

        const byArtist = compareText(a.artist, b.artist);
        if (byArtist !== 0) return byArtist;

        return compareText(a.name, b.name);
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

    function toggleSortMenu() {
        isSortMenuOpen = !isSortMenuOpen;
    }

    function selectSort(value: AlbumSortOption) {
        albumSort = value;
        isSortMenuOpen = false;
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
                    label: $_('contextMenu.playAlbumNext'),
                    action: async () => {
                        try {
                            const tracks = await getTracksByAlbum(album.id);
                            if (tracks.length === 0) {
                                addToast(
                                    $_("queue.noTracksToAdd", {
                                        default: "No tracks found for this album",
                                    }),
                                    "warning",
                                );
                                return;
                            }
                            playNext(tracks);
                            addToast(`Playing "${album.name}" next`, "success");
                        } catch (err) {
                            console.error("Failed to play album next:", err);
                            addToast("Failed to play album next", "error");
                        }
                    },
                },
                {
                    label: $_("contextMenu.addToQueue"),
                    icon: `<svg viewBox="0 0 24 24" fill="currentColor" width="18" height="18"><path d="M3 6h18v2H3V6zm0 5h18v2H3v-2zm0 5h12v2H3v-2zM17 13v6h6v-6h-6zm3 4.5L18 15l1.5-1.5L22 16l-2 2z"/></svg>`,
                    action: async () => {
                        try {
                            const tracks = await getTracksByAlbum(album.id);
                            if (tracks.length === 0) {
                                addToast(
                                    $_("queue.noTracksToAdd", {
                                        default: "No tracks found for this album",
                                    }),
                                    "warning",
                                );
                                return;
                            }
                            appendToQueueEnd(tracks);
                            addToast(
                                $_("queue.albumAddedToEnd", {
                                    values: { count: tracks.length, name: album.name },
                                    default: `Added ${tracks.length} tracks from "${album.name}" to end of queue`,
                                }),
                                "success",
                            );
                        } catch (err) {
                            console.error("Failed to add album to queue:", err);
                            addToast(
                                $_("queue.addToQueueFailed", {
                                    default: "Failed to add album to queue",
                                }),
                                "error",
                            );
                        }
                    },
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

<div class="albums-grid">
    <div class="albums-toolbar-wrap">
        <div class="albums-toolbar">
            <div class="search-filter-group">
                <div class="album-search">
                    <svg class="search-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true">
                        <circle cx="11" cy="11" r="8" />
                        <line x1="21" y1="21" x2="16.65" y2="16.65" />
                    </svg>
                    <input
                        type="text"
                        class="search-input"
                        placeholder="Buscar álbumes..."
                        bind:value={searchQuery}
                    />
                    {#if searchQuery}
                        <button class="search-clear" on:click={() => (searchQuery = "")} aria-label="Limpiar búsqueda">
                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
                        </button>
                    {/if}
                </div>
                <button
                    class="filter-favorites"
                    class:active={showOnlyFavorites}
                    on:click={() => (showOnlyFavorites = !showOnlyFavorites)}
                    title={showOnlyFavorites ? "Mostrar todos" : "Solo favoritos"}
                    aria-pressed={showOnlyFavorites}
                >
                    <svg viewBox="0 0 24 24" width="18" height="18"
                        fill={showOnlyFavorites ? "currentColor" : "none"}
                        stroke="currentColor" stroke-width="2"
                    >
                        <path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"/>
                    </svg>
                </button>
            </div>
            <div class="sort-group">
                <span class="sort-label">Ordenar por</span>
            <div class="sort-dropdown">
                <button
                    class="sort-trigger"
                    type="button"
                    aria-haspopup="menu"
                    aria-expanded={isSortMenuOpen}
                    on:click={toggleSortMenu}
                >
                    <span>{selectedSortLabel}</span>
                    <svg
                        class:open={isSortMenuOpen}
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        width="16"
                        height="16"
                        aria-hidden="true"
                    >
                        <polyline points="6 9 12 15 18 9" />
                    </svg>
                </button>

                {#if isSortMenuOpen}
                    <div class="sort-menu" role="menu" aria-label="Opciones de orden">
                        {#each sortOptions as option (option.value)}
                            <button
                                type="button"
                                role="menuitemradio"
                                aria-checked={albumSort === option.value}
                                class="sort-menu-item"
                                class:active={albumSort === option.value}
                                on:click={() => selectSort(option.value)}
                            >
                                {option.label}
                            </button>
                        {/each}
                    </div>
                {/if}
            </div>
            </div>
        </div>
    </div>

    <div class="albums-grid-body">
        <VirtualizedGrid
            items={sortedAlbums}
            bind:currentScrollTop
            initialScrollTop={currentScrollTop}
            onItemClick={handleAlbumClick}
            onItemContextMenu={handleAlbumContextMenu}
            onLoadMore={handleLoadMore}
            emptyStateConfig={emptyState}
            cardWidthDesktop={240}
            cardWidthMobile={170}
            cardHeightDesktop={380}
            cardHeightMobile={305}
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
                isLiked={$likedAlbumIds.has(album.id)}
                playTooltip="Play album"
                resumeTooltip="Resume album"
                pauseTooltip="Pause"
                ariaLabel={album.name}
                primaryText={album.name}
                secondaryText={(album.artist || "Unknown Artist") + ((album.original_year || album.year) ? ` · ${album.original_year || album.year}` : '')}
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
    </div>
</div>

<style>
    .albums-grid {
        height: 100%;
        display: flex;
        flex-direction: column;
        min-height: 0;
    }

    .albums-grid-body {
        flex: 1;
        min-height: 0;
    }

    .albums-toolbar-wrap {
        padding: 0 var(--spacing-md);
    }

    .albums-toolbar {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        margin-top: 4px;
    }

    .search-filter-group {
        display: flex;
        align-items: center;
        gap: 6px;
        flex: 1;
        min-width: 0;
        max-width: 400px;
    }

    .album-search {
        display: flex;
        align-items: center;
        gap: 6px;
        flex: 1;
        min-width: 0;
        border: 1px solid var(--border-color);
        background: var(--bg-card);
        border-radius: 8px;
        padding: 0 10px;
        height: 34px;
        transition: border-color 0.15s;
    }

    .album-search:focus-within {
        border-color: var(--accent-primary);
        box-shadow: 0 0 0 2px color-mix(in oklab, var(--accent-primary) 20%, transparent);
    }

    .search-icon {
        color: var(--text-secondary);
        flex-shrink: 0;
    }

    .search-input {
        all: unset;
        flex: 1;
        min-width: 0;
        font-size: 0.8rem;
        color: var(--text-primary);
    }

    .search-input::placeholder {
        color: var(--text-secondary);
    }

    .search-clear {
        all: unset;
        cursor: pointer;
        color: var(--text-secondary);
        display: flex;
        align-items: center;
        padding: 2px;
        border-radius: 4px;
    }

    .search-clear:hover {
        color: var(--text-primary);
    }

    .filter-favorites {
        all: unset;
        cursor: pointer;
        display: flex;
        align-items: center;
        justify-content: center;
        width: 34px;
        height: 34px;
        border-radius: 8px;
        border: 1px solid var(--border-color);
        background: var(--bg-card);
        color: var(--text-secondary);
        flex-shrink: 0;
        transition: all 0.15s;
    }

    .filter-favorites:hover {
        color: var(--text-primary);
        border-color: var(--text-secondary);
    }

    .filter-favorites.active {
        color: var(--accent-primary);
        border-color: var(--accent-primary);
        background: color-mix(in oklab, var(--accent-primary) 12%, transparent);
    }

    .sort-group {
        display: flex;
        align-items: center;
        gap: 8px;
        flex-shrink: 0;
    }

    .sort-label {
        font-size: 0.8rem;
        color: var(--text-secondary);
    }

    .sort-dropdown {
        position: relative;
        min-width: 220px;
    }

    .sort-trigger {
        width: 100%;
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        border: 1px solid var(--border-color);
        background: var(--bg-card);
        color: var(--text-primary);
        border-radius: 8px;
        padding: 8px 10px;
        font-size: 0.8rem;
        line-height: 1.2;
        cursor: pointer;
    }

    .sort-trigger:focus {
        outline: none;
        border-color: var(--accent-primary);
        box-shadow: 0 0 0 2px color-mix(in oklab, var(--accent-primary) 20%, transparent);
    }

    .sort-trigger svg {
        color: var(--text-secondary);
        transition: transform 0.18s ease;
    }

    .sort-trigger svg.open {
        transform: rotate(180deg);
    }

    .sort-menu {
        position: absolute;
        top: calc(100% + 6px);
        right: 0;
        min-width: 100%;
        background: var(--bg-surface);
        border: 1px solid var(--border-color);
        border-radius: 10px;
        box-shadow: 0 12px 32px rgba(0, 0, 0, 0.45);
        padding: 6px;
        display: flex;
        flex-direction: column;
        gap: 2px;
        z-index: 40;
        max-height: min(360px, 60vh);
        overflow-y: auto;
        overflow-x: hidden;
        isolation: isolate;
        opacity: 1;
    }

    .sort-menu-item {
        border: 0;
        background: transparent;
        color: var(--text-primary);
        text-align: left;
        border-radius: 8px;
        padding: 8px 10px;
        font-size: 0.8rem;
        cursor: pointer;
    }

    .sort-menu-item:hover {
        background: var(--bg-highlight);
    }

    .sort-menu-item.active {
        background: color-mix(in oklab, var(--accent-primary) 20%, transparent);
        color: var(--accent-primary);
        font-weight: 600;
    }

    @media (max-width: 768px) {
        .albums-toolbar-wrap {
            padding: 0 var(--spacing-sm);
        }

        .albums-toolbar {
            flex-wrap: wrap;
            gap: 8px;
            margin-top: 0;
        }

        .search-filter-group {
            max-width: none;
            width: 100%;
        }

        .sort-label {
            display: none;
        }

        .sort-dropdown {
            width: 100%;
            min-width: 0;
        }

        .sort-group {
            width: 100%;
        }

        .sort-menu {
            left: 0;
            right: 0;
        }
    }

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
