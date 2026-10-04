import type { AlbumSort, DisplayAlbum } from "../types";
import type { ActionAvailability, BrowsePresentation, PresentationCommand } from "./types";

/** Display metadata only: no inferred liked, pinned or count fields. */
export function toAlbumCard(album: DisplayAlbum) {
 return { id: album.id, primaryText: album.name, secondaryText: (album.artist || "Unknown Artist") + (album.year ? ` · ${album.year}` : ""), qualityBadges: album.qualityBadges, ...(album.artwork ? { artwork: album.artwork } : {}) };
}
export function playAlbumGesture(albumId: number, playMode: "all" | "liked_only", availability: ActionAvailability, execute: PresentationCommand) {
 if (availability.enabled) return execute({ type: "play_album", albumId, playMode });
}
/** Preserve the page owner's identity/revision policy; this adapter never accumulates pages. */
export function toBrowsePresentation<T>(state: { items: T[]; revision: number | null; hasEarlier?: boolean; nextCursor: string | null; loading: boolean; error: string }): BrowsePresentation<T> {
 return { items: state.items, revision: state.revision, hasEarlier: !!state.hasEarlier, hasMore: state.nextCursor !== null, loading: state.loading, error: state.error };
}

    export const albumSortLabels: Record<AlbumSort, string> = {
        "artist-asc": "Artista (A-Z)",
        "artist-desc": "Artista (Z-A)",
        "year-desc": "Año (más reciente)",
        "year-asc": "Año (más antiguo)",
        "added-desc": "Antigüedad (agregados recientemente)",
        "added-asc": "Antigüedad (agregados antes)",
        "name-asc": "Álbum (A-Z)",
        "name-desc": "Álbum (Z-A)",
    };

    export const albumSortOptions: { value: AlbumSort; label: string }[] = [
        { value: "artist-asc", label: albumSortLabels["artist-asc"] },
        { value: "artist-desc", label: albumSortLabels["artist-desc"] },
        { value: "year-desc", label: albumSortLabels["year-desc"] },
        { value: "year-asc", label: albumSortLabels["year-asc"] },
        { value: "added-desc", label: albumSortLabels["added-desc"] },
        { value: "added-asc", label: albumSortLabels["added-asc"] },
        { value: "name-asc", label: albumSortLabels["name-asc"] },
        { value: "name-desc", label: albumSortLabels["name-desc"] },
    ];
