<script lang="ts">
    import { applicationMode } from "$lib/application/bootstrap";
    import { viewActions } from "$lib/application/view-actions";
    import type { ApplicationIntent, ApplicationQuery } from "$lib/application/types";
    import TrackRows from "./presentation/TrackRows.svelte";
    import ControllerBrowse from "./ControllerBrowse.svelte";
    function handleControllerIntent(intent: ApplicationIntent) { return viewActions.execute(intent); }
    function handleControllerQueue(query: ApplicationQuery, placement: "next" | "after_user_queue" | "end") { return viewActions.queueQuery(query, placement); }

  import type { Track } from "$lib/api/tauri";
  import {
    formatDuration,
    getAlbumArtSrc,
    getTrackCoverSrc,
    getAlbumCoverSrc,
    addTrackToPlaylist,
    removeTrackFromPlaylist,
    deleteTrack,
    reorderPlaylistTracks,
  } from "$lib/api/tauri";
  import {
    playTracks,
    currentTrack,
    isPlaying,
    addToQueue,
    playNext,
    type PlaybackContext,
  } from "$lib/stores/player";
  import { contextMenu } from "$lib/stores/ui";
  import {
    albums,
    playlists,
    loadPlaylists,
    loadLibrary,
    getTrackAlbumCover,
    loadMoreTracks,
    tracks as libraryTracks,
  } from "$lib/stores/library";
  import { pluginStore } from "$lib/stores/plugin-store";
  import { goToAlbumDetail, goToArtistDetail } from "$lib/stores/view";
  import {
    canDownload,
    downloadTrack,
    needsDownloadLocation,
  } from "$lib/services/downloadService";
  import { addToast } from "$lib/stores/toast";
  import { isOnline } from "$lib/stores/network";
  import { onDestroy, onMount } from "svelte";
  import { multiSelect } from "$lib/stores/multiselect";
  import { isMobile } from "$lib/stores/mobile";
  import { confirm, prompt } from "$lib/stores/dialogs";
  import { saveScroll, getScroll } from "$lib/stores/scrollMemory";
  import { setCustomArtwork } from "$lib/stores/customArtwork";
  import { likedTrackIds, toggleLike } from "$lib/stores/liked";
  import MetadataModal from "$lib/components/MetadataModal.svelte";
  import { _, locale } from "svelte-i18n";

  // MetadataModal state
  let metadataModalTrack: Track | null = null;

  // Audio quality metadata parsing
  function parseTrackMeta(track: Track): { sampleRate: number | null; bitDepth: number | null } {
    if (!track.metadata_json) return { sampleRate: null, bitDepth: null };
    try {
      const m = JSON.parse(track.metadata_json);
      return {
        sampleRate: m['__sample_rate_hz'] ?? null,
        bitDepth: m['__bit_depth'] ?? null,
      };
    } catch { return { sampleRate: null, bitDepth: null }; }
  }
  function fmtSr(hz: number): string {
    return hz % 1000 === 0 ? `${hz / 1000}kHz` : `${(hz / 1000).toFixed(1)}kHz`;
  }

  function normalizeTrackFormat(format: string | null | undefined): string | null {
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

  export let scrollKey: string | null = null;

  export let tracks: Track[] = [];
  // export let title = ""; // unused
  export let showAlbum: boolean = true;
  export let isTidalAvailable: boolean = true;
  export let playbackContext: PlaybackContext | undefined = undefined;
  export let playlistId: number | null = null;
  export let multiSelectMode: boolean = false;
  export let queueTracks: Track[] | null = null; // New prop for unified queue context
  export let disableVirtualScroll: boolean = false; // When true, render all tracks without virtual scrolling

  // Virtual scrolling configuration
  const TRACK_ROW_HEIGHT = 58; // pixels (matches desktop row height in CSS)
  const OVERSCAN = 5; // Extra rows to render above/below viewport

  let containerHeight = 600; // Will be calculated from container
  let scrollTop = 0;
  let scrollbarWidth = 0;
  let containerElement: HTMLDivElement;

  // Cache structures
  let failedImages = new Set<string>();
  const MAX_FAILED_IMAGES = 200;
  const trackAlbumArtCache = new Map<number, string | null>();
  let albumMap = new Map<number, any>();

  // Reactive play count map from library store
  let playCountMap = new Map<number, number>();
  $: playCountMap = new Map($libraryTracks.map(t => [t.id, t.play_count ?? 0]));
  // Force sort refresh when play counts change
  let playCountVersion = 0;
  $: { playCountMap; playCountVersion++; }

  // 1: Track albums by reference, not just length
  let lastAlbumsRef = $albums;
  $: {
    if ($albums !== lastAlbumsRef) {
      albumMap = new Map($albums.map((a) => [a.id, a]));
      lastAlbumsRef = $albums;
      trackAlbumArtCache.clear();
    }
  }

  // 2: Pre-compute playing track ID
  $: playingTrackId = $currentTrack?.id ?? null;

  // Mobile view mode: determines layout on small screens
  // 'album' = numbered list, no covers | 'playlist' = covers + info | 'library' = covers + full info
  $: mobileViewMode =
    !showAlbum && playbackContext?.type === "album"
      ? "album"
      : playbackContext?.type === "playlist"
        ? "playlist"
        : "library";

  // 3: Memoize availability check results
  const availabilityCache = new Map<number, boolean>();

  function isTrackUnavailable(track: Track): boolean {
    // Check cache first
    if (availabilityCache.has(track.id)) {
      return availabilityCache.get(track.id)!;
    }

    let unavailable = false;

    // Local tracks are always available
    if (!track.source_type || track.source_type === "local") {
      unavailable = false;
    } else if (track.local_src) {
      unavailable = false;
    } else {
      // Streaming track: only unavailable if NO plugin can play it
      const runtime = pluginStore.getRuntime();
      unavailable = !runtime || !runtime.streamResolvers.has(track.source_type);
    }

    availabilityCache.set(track.id, unavailable);
    return unavailable;
  }

  // Clear availability cache when dependencies change (including plugin store)
  // references $pluginStore to ensure reactivity when store state changes (e.g. init -> loaded)
  $: runtime = $pluginStore && pluginStore.getRuntime();
  $: {
    // Watch all relevant dependencies
    const _ = runtime;
    if ($isOnline !== undefined || isTidalAvailable !== undefined) {
      availabilityCache.clear();
    }
  }

  $: filteredTracks = tracks;

  // Sorting state
  type SortField =
    | "title"
    | "track_number"
    | "artist"
    | "album"
    | "duration"
    | "play_count"
    | null;
  let sortField: SortField = null;
  let sortDirection: "asc" | "desc" = "asc";

  function toggleSort(field: SortField) {
    if (sortField === field) {
      if (sortDirection === "asc") {
        sortDirection = "desc";
      } else {
        sortField = null;
        sortDirection = "asc";
      }
    } else {
      sortField = field;
      // For play_count, default to descending (most played first)
      sortDirection = field === "play_count" ? "desc" : "asc";
    }
  }

  // Optimized sorting with memoization
  let lastSortField: SortField = null;
  let lastSortDirection: "asc" | "desc" = "asc";
  let lastFilteredTracks: Track[] = [];
  let cachedSortedTracks: Track[] = [];

  $: {
    // Re-sort if sort params, tracks, or play counts changed
    // playCountVersion is tracked to force re-sort on play count updates
    const _pcv = playCountVersion;
    if (
      sortField !== lastSortField ||
      sortDirection !== lastSortDirection ||
      filteredTracks !== lastFilteredTracks ||
      (sortField === "play_count" && _pcv)
    ) {
      if (!sortField) {
        cachedSortedTracks = filteredTracks;
      } else {
        cachedSortedTracks = [...filteredTracks].sort((a, b) => {
          let valA: any = "";
          let valB: any = "";

          switch (sortField) {
            case "title":
              valA = (a.title || "").toLowerCase();
              valB = (b.title || "").toLowerCase();
              break;
            case "track_number":
              valA = a.track_number ?? a.id;
              valB = b.track_number ?? b.id;
              break;
            case "artist":
              valA = (a.artist || "").toLowerCase();
              valB = (b.artist || "").toLowerCase();
              break;
            case "album":
              valA = (a.album || "").toLowerCase();
              valB = (b.album || "").toLowerCase();
              break;
            case "duration":
              valA = a.duration || 0;
              valB = b.duration || 0;
              break;
            case "play_count":
              valA = playCountMap.get(a.id) ?? 0;
              valB = playCountMap.get(b.id) ?? 0;
              break;
          }

          if (valA < valB) return sortDirection === "asc" ? -1 : 1;
          if (valA > valB) return sortDirection === "asc" ? 1 : -1;
          return 0;
        });
      }

      lastSortField = sortField;
      lastSortDirection = sortDirection;
      lastFilteredTracks = filteredTracks;
    }
  }

  $: sortedTracks = cachedSortedTracks;

  // 4: Build track index map
  let trackIndexMap = new Map<number, number>();
  $: {
    trackIndexMap = new Map(
      sortedTracks.map((track, index) => [track.id, index]),
    );
  }

  // Batch virtual scroll calculations
  let virtualScrollState = {
    totalHeight: 0,
    startIndex: 0,
    endIndex: 0,
    offsetY: 0,
    visibleTracks: [] as Track[],
  };

  $: {
    if (disableVirtualScroll) {
      // No virtual scrolling: render all tracks
      virtualScrollState = {
        totalHeight: sortedTracks.length * TRACK_ROW_HEIGHT,
        startIndex: 0,
        endIndex: sortedTracks.length,
        offsetY: 0,
        visibleTracks: sortedTracks,
      };
    } else {
    const totalHeight = sortedTracks.length * TRACK_ROW_HEIGHT;
    const startIndex = Math.max(
      0,
      Math.floor(scrollTop / TRACK_ROW_HEIGHT) - OVERSCAN,
    );
    const endIndex = Math.min(
      sortedTracks.length,
      Math.ceil((scrollTop + containerHeight) / TRACK_ROW_HEIGHT) + OVERSCAN,
    );
    const visibleTracks = sortedTracks.slice(startIndex, endIndex);
    const offsetY = startIndex * TRACK_ROW_HEIGHT;

    virtualScrollState = {
      totalHeight,
      startIndex,
      endIndex,
      offsetY,
      visibleTracks,
    };
    }
  }

  // Infinite scroll: when virtual scroll nears the bottom of loaded tracks,
  // fetch the next paginated batch from the backend.
  $: {
    if (
      $applicationMode !== "controller" && virtualScrollState.endIndex >= sortedTracks.length - 10 &&
      sortedTracks.length > 0
    ) {
      loadMoreTracks();
    }
  }

  // 5: Pre-compute album art and availability for visible tracks
  type TrackWithMetadata = {
    track: Track;
    albumArt: string | null;
    unavailable: boolean;
  };

  $: visibleTracksWithMetadata = virtualScrollState.visibleTracks.map(
    (track) => {
      // Re-evaluate when runtime changes
      const _ = runtime;
      return {
        track,
        albumArt: getTrackAlbumArt(track),
        unavailable: isTrackUnavailable(track),
      };
    },
  ) as TrackWithMetadata[];

  function handleScroll(e: Event) {
    const target = e.target as HTMLElement;
    scrollTop = target.scrollTop;
    scrollbarWidth = Math.max(0, target.offsetWidth - target.clientWidth);
  }

  // Measure container height on mount
  onMount(() => {
        if ($applicationMode === "controller") return;
    // 5: Load playlists once on mount to avoid race conditions
    if ($playlists.length === 0) {
      loadPlaylists();
    }

    if (containerElement) {
      const updateHeight = () => {
        containerHeight = containerElement.clientHeight;
        scrollbarWidth = Math.max(
          0,
          containerElement.offsetWidth - containerElement.clientWidth,
        );
      };
      updateHeight();

      if (scrollKey) {
        const saved = getScroll(scrollKey);
        if (saved > 0 && containerElement) {
          containerElement.scrollTop = saved;
        }
      }

      window.addEventListener("resize", updateHeight);
      return () => {
        window.removeEventListener("resize", updateHeight);
      };
    }
  });

  // Cleanup for drag listeners to prevent memory leaks
  let cleanupDragListeners: (() => void) | null = null;

  // Cleanup on destroy
  onDestroy(() => {
    if (scrollKey) saveScroll(scrollKey, scrollTop);
    failedImages.clear();
    trackAlbumArtCache.clear();
    albumMap.clear();
    availabilityCache.clear();

    if (cleanupInterval) {
      clearInterval(cleanupInterval);
    }

    // Clean up drag listeners if component unmounts during drag
    if (cleanupDragListeners) {
      cleanupDragListeners();
    }

    // Clean up swipe timer
    if (swipeResetTimer) {
      clearTimeout(swipeResetTimer);
    }
  });

  // 6: cleanup interval
  let cleanupInterval: number | undefined;

  function startCleanupInterval() {
    if (cleanupInterval || typeof window === "undefined") return;

    cleanupInterval = window.setInterval(() => {
      if (failedImages.size > MAX_FAILED_IMAGES) {
        const toKeep = Array.from(failedImages).slice(-MAX_FAILED_IMAGES / 2);
        failedImages.clear();
        toKeep.forEach((src) => failedImages.add(src));
        failedImages = failedImages;
      }

      // Stop interval if no failed images
      if (failedImages.size === 0 && cleanupInterval) {
        clearInterval(cleanupInterval);
        cleanupInterval = undefined;
      }
    }, 300000);
  }

  // Cached album art lookup
  function getTrackAlbumArt(track: Track): string | null {
    // Check cache first
    if (trackAlbumArtCache.has(track.id)) {
      return trackAlbumArtCache.get(track.id) ?? null;
    }

    let result: string | null = null;

    // Priority 1: Track's own cover (handles both track_cover_path and track_cover)
    result = getTrackCoverSrc(track);

    // Priority 2: If no track cover, try album art
    if (!result && track.album_id) {
      const album = albumMap.get(track.album_id);
      if (album) {
        result = getAlbumCoverSrc(album);
      }
    }

    // Priority 3: fallback to library helper
    if (!result) {
      result = getTrackAlbumCover(track.id);
    }

    // Cache the result
    trackAlbumArtCache.set(track.id, result);
    return result;
  }

  // Event delegation
  function handleBodyClick(e: MouseEvent) {
    const row = (e.target as HTMLElement).closest(".track-row");
    if (!row) return;

    const trackId = parseInt(row.getAttribute("data-track-id") || "0");

    // In multi-select mode, clicking toggles selection
    if (multiSelectMode) {
      multiSelect.toggleTrack(trackId);
      return;
    }
  }

  function handleBodyDoubleClick(e: MouseEvent) {
    const row = (e.target as HTMLElement).closest(".track-row");
    if (!row) return;

    const trackId = parseInt(row.getAttribute("data-track-id") || "0");
    const trackIndex = trackIndexMap.get(trackId);

    if (trackIndex === undefined) return;

    const track = sortedTracks[trackIndex];
    if (!track || isTrackUnavailable(track)) return;

    // Use unified queueTracks if available
    if (queueTracks) {
      const globalIndex = queueTracks.findIndex((t) => t.id === trackId);
      if (globalIndex !== -1) {
        playTracks(queueTracks, globalIndex, playbackContext);
        return;
      }
    }

    playTracks(sortedTracks, trackIndex, playbackContext);
  }

  async function handleBodyContextMenu(e: MouseEvent) {
    const row = (e.target as HTMLElement).closest(".track-row");
    if (!row) return;

    e.preventDefault();

    const trackId = parseInt(row.getAttribute("data-track-id") || "0");
    const trackIndex = trackIndexMap.get(trackId);

    if (trackIndex === undefined) return;

    const track = sortedTracks[trackIndex];
    if (!track) return;

    const playlistItems = $playlists.map((playlist) => ({
      label: playlist.name,
      action: async () => {
        try {
          await addTrackToPlaylist(playlist.id, track.id);
        } catch (error) {
          console.error("Failed to add track to playlist:", error);
        }
      },
    }));

    const isUnavailable = isTrackUnavailable(track);

    const menuItems: any[] = [
      {
        label: $_('contextMenu.play'),
        action: () => {
          if (trackIndex !== undefined) {
            // Use unified queueTracks if available
            if (queueTracks) {
              const globalIndex = queueTracks.findIndex(
                (t) => t.id === trackId,
              );
              if (globalIndex !== -1) {
                playTracks(queueTracks, globalIndex, playbackContext);
                return;
              }
            }
            playTracks(sortedTracks, trackIndex, playbackContext);
          }
        },
        disabled: isUnavailable,
      },
      { type: "separator" },
      {
        label: $_('contextMenu.playNext'),
        action: () => playNext([track]),
        disabled: isUnavailable,
      },
      {
        label: $_('contextMenu.addToQueue'),
        action: () => addToQueue([track]),
        disabled: isUnavailable,
      },
      { type: "separator" },
      {
        label: $_('contextMenu.download'),
        action: async () => {
          if (needsDownloadLocation()) {
            addToast(
              "Please configure a download location in Settings first",
              "error",
            );
            return;
          }

          addToast(`Downloading "${track.title}"...`, "info");
          try {
            await downloadTrack(track);
            addToast(`Downloaded "${track.title}"`, "success");
          } catch (error) {
            console.error("Failed to download track:", error);
            addToast(`Failed to download "${track.title}"`, "error");
          }
        },
        disabled:
          !canDownload(track) ||
          (isUnavailable && !isTidalAvailable && !track.local_src),
      },
      { type: "separator" },
      {
        label: $_('contextMenu.addToPlaylist'),
        submenu:
          playlistItems.length > 0
            ? playlistItems
            : [
                {
                  label: $_('contextMenu.noPlaylists'),
                  action: () => {},
                  disabled: true,
                },
              ],
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
                const file = (e.target as HTMLInputElement).files?.[0];
                if (file) {
                  const reader = new FileReader();
                  reader.onload = () => {
                    const result = reader.result as string;
                    setCustomArtwork("track", track.id, result);
                    addToast("Artwork updated", "success");
                    // Refresh local cache for reactivity
                    trackAlbumArtCache.delete(track.id);
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
                placeholder: "https://example.com/image.jpg",
              });
              if (url && url.trim()) {
                setCustomArtwork("track", track.id, url.trim());
                addToast("Artwork updated", "success");
                trackAlbumArtCache.delete(track.id);
              }
            },
          },
        ],
      },
    ];

    menuItems.push(
      { type: "separator" },
      {
        label: $_('contextMenu.showMoreInfo'),
        action: () => {
          metadataModalTrack = track;
        },
      },
    );

    if (playlistId) {
      menuItems.push({
        label: $_('contextMenu.removeFromPlaylist'),
        action: async () => {
          try {
            await removeTrackFromPlaylist(playlistId, track.id);
            tracks = tracks.filter((t) => t.id !== track.id);
          } catch (error) {
            console.error("Failed to remove track from playlist:", error);
          }
        },
      });
    }

    menuItems.push(
      { type: "separator" },
      {
        label: $_('contextMenu.deleteFromLibrary'),
        danger: true,
        action: async () => {
          const confirmed = await confirm(
            `Are you sure you want to delete "${track.title}" from your library? This will also remove the file from your computer.`,
            {
              title: "Delete Track",
              confirmLabel: "Delete",
              danger: true,
            },
          );

          if (!confirmed) return;

          try {
            await deleteTrack(track.id);
            // Clear from cache
            trackAlbumArtCache.delete(track.id);
            availabilityCache.delete(track.id);
            await loadLibrary();
            // Also remove from local tracks array for immediate UI feedback
            tracks = tracks.filter((t) => t.id !== track.id);
          } catch (error) {
            console.error("Failed to delete track:", error);
          }
        },
      },
    );

    contextMenu.set({
      visible: true,
      x: e.clientX,
      y: e.clientY,
      items: menuItems,
    });
  }

  function handleImageError(albumArt: string) {
    if (failedImages.size >= MAX_FAILED_IMAGES) {
      const toKeep = Array.from(failedImages).slice(-MAX_FAILED_IMAGES / 2);
      failedImages.clear();
      toKeep.forEach((src) => failedImages.add(src));
    }

    failedImages.add(albumArt);
    failedImages = failedImages;

    // Start cleanup interval if needed
    startCleanupInterval();
  }

  // Drag and drop for playlist reordering (only enabled when playlistId is set)
  let draggedIndex: number | null = null;
  let dragOverIndex: number | null = null;
  let isDragging = false;

  function handlePointerDown(e: PointerEvent, actualIndex: number) {
    if (!playlistId) return; // Only allow dragging in playlists

    e.preventDefault();
    e.stopPropagation();
    e.stopImmediatePropagation(); // Prevent parent handlers
    isDragging = true;
    draggedIndex = actualIndex;

    // Capture pointer events
    const target = e.currentTarget as HTMLElement;
    target.setPointerCapture(e.pointerId);

    // Add global listeners
    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp);

    // Store cleanup function for memory leak prevention
    cleanupDragListeners = () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
    };
  }

  function handlePointerMove(e: PointerEvent) {
    if (!isDragging || draggedIndex === null || !playlistId) return;

    // Find element under pointer
    const elementsUnderPointer = document.elementsFromPoint(
      e.clientX,
      e.clientY,
    );
    const trackRow = elementsUnderPointer.find((el) =>
      el.classList.contains("track-row"),
    );

    if (trackRow) {
      const indexAttr = trackRow.getAttribute("data-track-index");
      if (indexAttr !== null) {
        const overIndex = parseInt(indexAttr, 10);
        if (overIndex !== draggedIndex) {
          dragOverIndex = overIndex;
        } else {
          dragOverIndex = null;
        }
      }
    } else {
      dragOverIndex = null;
    }
  }

  async function handlePointerUp() {
    if (
      isDragging &&
      draggedIndex !== null &&
      dragOverIndex !== null &&
      draggedIndex !== dragOverIndex &&
      playlistId
    ) {
      try {
        // Update backend
        await reorderPlaylistTracks(playlistId, draggedIndex, dragOverIndex);

        console.log("Reorder successful, updating local state");

        // Update local state for instant feedback
        const newTracks = [...tracks];
        const [removed] = newTracks.splice(draggedIndex, 1);
        newTracks.splice(dragOverIndex, 0, removed);
        tracks = newTracks;

        addToast("Tracks reordered", "success");
      } catch (error) {
        console.error("Failed to reorder tracks:", error);
        addToast(`Failed to reorder tracks: ${error}`, "error");
      }
    }

    // Cleanup
    isDragging = false;
    draggedIndex = null;
    dragOverIndex = null;

    // Clean up and clear the cleanup function
    if (cleanupDragListeners) {
      cleanupDragListeners();
      cleanupDragListeners = null;
    }
  }

  // â”€â”€ Swipe-to-queue (mobile only) â”€â”€
  let swipeStartX = 0;
  let swipeStartY = 0;
  let swipeDeltaX = 0;
  let swipingRow: HTMLElement | null = null;
  let swipeTrackId: number | null = null;
  let swipeCommitted = false;
  const SWIPE_THRESHOLD = 80; // px to trigger add-to-queue
  const SWIPE_MAX = 120;
  let swipeResetTimer: ReturnType<typeof setTimeout> | null = null;

  function handleSwipeTouchStart(e: TouchEvent) {
    if (!$isMobile || multiSelectMode) return;
    // Don't swipe on drag handles
    if ((e.target as HTMLElement).closest(".drag-handle")) return;

    const touch = e.touches[0];
    swipeStartX = touch.clientX;
    swipeStartY = touch.clientY;
    swipeDeltaX = 0;
    swipeCommitted = false;

    const row = (e.target as HTMLElement).closest(".track-row") as HTMLElement;
    if (row) {
      swipingRow = row;
      swipeTrackId = parseInt(row.getAttribute("data-track-id") || "0");
    }
  }

  function handleSwipeTouchMove(e: TouchEvent) {
    if (!swipingRow || swipeCommitted) return;

    const touch = e.touches[0];
    const dx = touch.clientX - swipeStartX;
    const dy = touch.clientY - swipeStartY;

    // If vertical movement is dominant, cancel swipe (allow scroll)
    if (Math.abs(dy) > Math.abs(dx) && Math.abs(dx) < 15) {
      swipingRow.style.transform = "";
      swipingRow.style.transition = "";
      swipingRow = null;
      return;
    }

    // Only right-swipe
    if (dx < 0) {
      swipeDeltaX = 0;
      swipingRow.style.transform = "";
      return;
    }

    // Prevent vertical scroll while swiping
    e.preventDefault();

    swipeDeltaX = Math.min(dx, SWIPE_MAX);
    swipingRow.style.transition = "none";
    swipingRow.style.transform = `translateX(${swipeDeltaX}px)`;

    // Visual feedback: change bg when past threshold
    if (swipeDeltaX >= SWIPE_THRESHOLD) {
      swipingRow.classList.add("swipe-queue-ready");
    } else {
      swipingRow.classList.remove("swipe-queue-ready");
    }
  }

  function handleSwipeTouchEnd() {
    if (!swipingRow) return;

    const row = swipingRow;
    const trackId = swipeTrackId;

    if (swipeDeltaX >= SWIPE_THRESHOLD && trackId) {
      swipeCommitted = true;
      row.classList.add("swipe-queue-added");
      row.classList.remove("swipe-queue-ready");

      // Find track and add to queue
      const trackIndex = trackIndexMap.get(trackId);
      if (trackIndex !== undefined) {
        const track = sortedTracks[trackIndex];
        if (track) {
          addToQueue([track]);
          addToast(`Added "${track.title}" to queue`, "success");
        }
      }

      // Animate back after short delay
      swipeResetTimer = setTimeout(() => {
        row.style.transition = "transform 0.25s ease";
        row.style.transform = "";
        row.classList.remove("swipe-queue-added");
      }, 400);
    } else {
      // Snap back
      row.style.transition = "transform 0.25s ease";
      row.style.transform = "";
      row.classList.remove("swipe-queue-ready");
    }

    swipingRow = null;
    swipeTrackId = null;
    swipeDeltaX = 0;
  }

  // Helper to handle album click from event delegation
  function handleAlbumClick(e: MouseEvent) {
    const albumButton = (e.target as HTMLElement).closest(".col-album-cell");
    if (!albumButton) return;

    e.stopPropagation();

    const row = albumButton.closest(".track-row");
    if (!row) return;

    const trackId = parseInt(row.getAttribute("data-track-id") || "0");
    const trackIndex = trackIndexMap.get(trackId);

    if (trackIndex === undefined) return;

    const track = sortedTracks[trackIndex];
    if (track && track.album_id) {
      goToAlbumDetail(track.album_id);
    }
  }

  function handleArtistClick(e: MouseEvent) {
    const artistButton = (e.target as HTMLElement).closest(".track-artist");
    if (!artistButton) return;

    e.stopPropagation();

    const row = artistButton.closest(".track-row");
    if (!row) return;

    const trackId = parseInt(row.getAttribute("data-track-id") || "0");
    const trackIndex = trackIndexMap.get(trackId);

    if (trackIndex === undefined) return;

    const track = sortedTracks[trackIndex];
    if (track && track.artist) {
      goToArtistDetail(track.artist);
    }
  }
</script>
{#if $applicationMode === "controller"}
 <ControllerBrowse query={{type:"tracks"}} heading="Tracks" enqueue={handleControllerQueue} execute={handleControllerIntent} />
{:else}


{#if metadataModalTrack}
  <MetadataModal
    track={metadataModalTrack}
    onClose={() => {
      metadataModalTrack = null;
    }}
  />
{/if}

<TrackRows {disableVirtualScroll} {showAlbum}>
 <svelte:fragment slot="header"><header
    class="list-header"
    class:no-album={!showAlbum}
    class:with-drag={playlistId !== null}
    class:multiselect={multiSelectMode}
    style={`--scrollbar-width: ${scrollbarWidth}px`}
  >
    {#if multiSelectMode}
      <div class="col-header col-checkbox">
        <input
          type="checkbox"
          on:change={(e) => {
            if (e.currentTarget.checked) {
              multiSelect.selectAll(sortedTracks.map((t) => t.id));
            } else {
              multiSelect.clearSelections();
            }
          }}
          checked={$multiSelect.selectedTrackIds.size > 0 &&
            $multiSelect.selectedTrackIds.size === sortedTracks.length}
          indeterminate={$multiSelect.selectedTrackIds.size > 0 &&
            $multiSelect.selectedTrackIds.size < sortedTracks.length}
        />
      </div>
    {/if}
    {#if playlistId !== null && !multiSelectMode}
      <span class="col-header col-drag"></span>
    {/if}
    <button class="col-header col-num sortable" on:click={() => toggleSort("track_number")}>
      #
      {#if sortField === "track_number"}
        <span class="sort-icon">{sortDirection === "asc" ? "â–²" : "â–¼"}</span>
      {/if}
    </button>
    <span class="col-header col-cover" aria-hidden="true"></span>
    <button
      class="col-header col-artist sortable"
      on:click={() => toggleSort("title")}
    >
      {$_('trackList.title')}
      {#if sortField === "title"}
        <span class="sort-icon">{sortDirection === "asc" ? "â–²" : "â–¼"}</span>
      {/if}
    </button>
    {#if showAlbum}
      <button
        class="col-header col-album sortable"
        on:click={() => toggleSort("album")}
      >
        {$_('trackList.album')}
        {#if sortField === "album"}
          <span class="sort-icon">{sortDirection === "asc" ? "â–²" : "â–¼"}</span>
        {/if}
      </button>
    {/if}
    <button
      class="col-header col-duration sortable"
      on:click={() => toggleSort("duration")}
    >
      {$_('trackList.duration')}
      {#if sortField === "duration"}
        <span class="sort-icon">{sortDirection === "asc" ? "â–²" : "â–¼"}</span>
      {/if}
    </button>
    <span class="col-header col-like">
      <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"/>
      </svg>
    </span>
    <button
      class="col-header col-plays sortable"
      on:click={() => toggleSort("play_count")}
    >
      {$_('trackList.plays')}
      {#if sortField === "play_count"}
        <span class="sort-icon">{sortDirection === "asc" ? "â–²" : "â–¼"}</span>
      {/if}
    </button>
  </header></svelte:fragment>
  <!-- Virtualized scrolling container -->
  {#if sortedTracks.length > 0}
    <!-- Event delegation - handlers on container instead of each row -->
    <div
      class="list-body"
      class:no-album={!showAlbum}
      class:no-scroll={disableVirtualScroll}
      class:with-drag={playlistId !== null && !multiSelectMode}
      class:multiselect={multiSelectMode}
      class:mobile-album={mobileViewMode === "album"}
      class:mobile-playlist={mobileViewMode === "playlist"}
      class:mobile-library={mobileViewMode === "library"}
      on:scroll={handleScroll}
      on:click={handleBodyClick}
      on:dblclick={handleBodyDoubleClick}
      on:contextmenu={handleBodyContextMenu}
      on:touchstart={handleSwipeTouchStart}
      on:touchmove={handleSwipeTouchMove}
      on:touchend={handleSwipeTouchEnd}
      bind:this={containerElement}
    >
      <div
        class="virtual-spacer"
        style="height: {virtualScrollState.totalHeight}px;"
      >
        <div
          class="virtual-content"
          style="transform: translateY({virtualScrollState.offsetY}px);"
        >
          {#each visibleTracksWithMetadata as { track, albumArt, unavailable }, index (track.id)}
            {@const actualIndex = virtualScrollState.startIndex + index}
            {@const isSelected = $multiSelect.selectedTrackIds.has(track.id)}
            {@const audioMeta = parseTrackMeta(track)}
            <div
              class="track-row"
              class:playing={playingTrackId === track.id}
              class:unavailable
              class:dragging={draggedIndex === actualIndex}
              class:drag-over={dragOverIndex === actualIndex}
              class:selected={multiSelectMode && isSelected}
              data-track-id={track.id}
              data-track-index={actualIndex}
              role="button"
              tabindex="0"
            >
              {#if multiSelectMode}
                <div
                  class="col-checkbox"
                  on:click|stopPropagation={() =>
                    multiSelect.toggleTrack(track.id)}
                  role="checkbox"
                  aria-checked={isSelected}
                  tabindex="0"
                >
                  <div class="custom-checkbox" class:checked={isSelected}>
                    {#if isSelected}
                      <svg
                        viewBox="0 0 24 24"
                        fill="currentColor"
                        width="14"
                        height="14"
                      >
                        <path
                          d="M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z"
                        />
                      </svg>
                    {/if}
                  </div>
                </div>
              {/if}
              {#if playlistId !== null && !multiSelectMode}
                <div
                  class="drag-handle"
                  on:pointerdown={(e) => handlePointerDown(e, actualIndex)}
                  on:click|stopPropagation
                  on:dblclick|stopPropagation
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
              {/if}
              <span class="col-num">
                {#if playingTrackId === track.id && $isPlaying}
                  <svg
                    class="playing-icon"
                    viewBox="0 0 24 24"
                    fill="currentColor"
                    width="18"
                    height="18"
                  >
                    <path
                      d="M12 3v10.55c-.59-.34-1.27-.55-2-.55-2.21 0-4 1.79-4 4s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z"
                    />
                  </svg>
                  <span class="equalizer-bars">
                    <span class="eq-bar"></span>
                    <span class="eq-bar"></span>
                    <span class="eq-bar"></span>
                    <span class="eq-bar"></span>
                  </span>
                {:else}
                  <span class="track-index">{actualIndex + 1}</span>
                  <button class="hover-play" on:click|stopPropagation={() => {
                    if (!isTrackUnavailable(track)) {
                      if (queueTracks) {
                        const gi = queueTracks.findIndex((t) => t.id === track.id);
                        if (gi !== -1) { playTracks(queueTracks, gi, playbackContext); return; }
                      }
                      playTracks(sortedTracks, actualIndex, playbackContext);
                    }
                  }} on:dblclick|stopPropagation title="Play" aria-label="Play">&#9654;</button>
                {/if}
              </span>

              <span class="col-cover">
                <div class="cover-wrapper">
                  {#if albumArt && !failedImages.has(albumArt)}
                    <img
                      src={albumArt}
                      alt="Album cover"
                      class="cover-image"
                      loading="lazy"
                      decoding="async"
                      on:error={() => handleImageError(albumArt)}
                    />
                  {:else}
                    <div class="cover-placeholder">
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
                  <div class="cover-play-overlay">
                    <svg
                      viewBox="0 0 24 24"
                      fill="currentColor"
                      width="18"
                      height="18"
                    >
                      <path d="M8 5v14l11-7z" />
                    </svg>
                  </div>
                </div>
              </span>

              {#if $isMobile}
                <div class="col-title">
                  <div class="title-row">
                    <span class="track-name truncate"
                      >{track.title || "Unknown Title"}</span
                    >

                    {#if !track.source_type || track.source_type === "local" || track.local_src}
                      <span class="downloaded-icon" title="Downloaded">
                        <svg
                          viewBox="0 0 24 24"
                          fill="currentColor"
                          width="14"
                          height="14"
                        >
                          <path
                            d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-2 15l-5-5 1.41-1.41L10 14.17l7.59-7.59L19 8l-9 9z"
                          />
                        </svg>
                      </span>
                    {/if}

                    {#if track.format}
                      {@const formatUpper = track.format.toUpperCase()}
                      {@const displayFormat =
                        formatUpper.includes("HI_RES") ||
                        formatUpper.includes("HIRES")
                          ? "HI-RES"
                          : formatUpper.includes("LOSSLESS")
                            ? "LOSSLESS"
                            : formatUpper.replace("MPEG", "MP3")}
                      <span
                        class="quality-tag"
                        class:high-quality={formatUpper.includes("FLAC") ||
                          formatUpper.includes("WAV") ||
                          formatUpper.includes("HI_RES") ||
                          formatUpper.includes("HIRES") ||
                          (track.bitrate && track.bitrate >= 320)}
                      >
                        {displayFormat}
                      </span>
                    {/if}
                    {#if audioMeta.sampleRate}
                      <span class="quality-tag">{fmtSr(audioMeta.sampleRate)}</span>
                    {/if}
                    {#if audioMeta.bitDepth}
                      <span class="quality-tag">{audioMeta.bitDepth}bit</span>
                    {/if}
                  </div>
                  {#if playbackContext?.type !== "album"}
                    <button
                      class="track-artist truncate"
                      on:click={handleArtistClick}
                      >{track.artist || "Unknown Artist"}</button
                    >
                  {/if}
                </div>
              {:else}
                <div class="col-artist">
                  <div class="artist-meta">
                    <span class="track-name truncate"
                      >{track.title || "Unknown Title"}</span
                    >
                    {#if playbackContext?.type !== "album"}
                      <button class="track-artist truncate" on:click={handleArtistClick}
                        >{track.artist || "Unknown Artist"}</button
                      >
                    {/if}
                    {#if track.format || audioMeta.sampleRate || audioMeta.bitDepth}
                      <div class="track-quality-row">
                        {#if track.format}
                          {@const desktopFormat = normalizeTrackFormat(track.format)}
                          {#if desktopFormat}
                            <span
                              class="quality-tag"
                              class:high-quality={track.format.toUpperCase().includes("FLAC") ||
                                track.format.toUpperCase().includes("WAV") ||
                                track.format.toUpperCase().includes("HI_RES") ||
                                track.format.toUpperCase().includes("HIRES") ||
                                (track.bitrate && track.bitrate >= 320)}
                            >
                              {desktopFormat}
                            </span>
                          {/if}
                        {/if}
                        {#if audioMeta.sampleRate}
                          <span class="quality-tag">{fmtSr(audioMeta.sampleRate)}</span>
                        {/if}
                        {#if audioMeta.bitDepth}
                          <span class="quality-tag">{audioMeta.bitDepth}bit</span>
                        {/if}
                      </div>
                    {/if}
                  </div>
                </div>
              {/if}
              {#if showAlbum}
                <button
                  class="col-album-cell truncate"
                  on:click={handleAlbumClick}
                  disabled={!track.album_id}>{track.album || "-"}</button
                >
              {/if}
              <span class="col-duration">{formatDuration(track.duration)}</span>
              {#if !$isMobile}
                <button
                  class="col-like"
                  class:liked={$likedTrackIds.has(track.id)}
                  on:click|stopPropagation={() => toggleLike(track.id)}
                  on:dblclick|stopPropagation
                  title={$likedTrackIds.has(track.id) ? "Unlike" : "Like"}
                >
                  <svg viewBox="0 0 24 24" width="16" height="16"
                    fill={$likedTrackIds.has(track.id) ? "currentColor" : "none"}
                    stroke="currentColor" stroke-width="2"
                  >
                    <path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"/>
                  </svg>
                </button>
                <span class="col-plays">{playCountMap.get(track.id) ?? 0}</span>
              {/if}
            </div>
          {/each}
        </div>
      </div>
    </div>
  {:else}
    <div class="list-body">
      <div class="empty-state">
        <svg viewBox="0 0 24 24" fill="currentColor" width="48" height="48">
          <path
            d="M12 3v10.55c-.59-.34-1.27-.55-2-.55-2.21 0-4 1.79-4 4s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z"
          />
        </svg>
        <h3>{$_('trackList.noTracksFound')}</h3>
        <p>{$_('trackList.addFolderToGetStarted')}</p>
      </div>
    </div>
  {/if}

</TrackRows>

{/if}
