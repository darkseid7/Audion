import { writable } from 'svelte/store';

const STORAGE_KEY = 'rlist_playlist_covers_v1';

type CoversMap = Record<number, string>; // playlistId -> dataURL

function loadFromStorage(): CoversMap {
    if (typeof window === 'undefined') return {};
    try {
        const raw = localStorage.getItem(STORAGE_KEY);
        if (!raw) return {};
        return JSON.parse(raw) as CoversMap;
    } catch (e) {
        console.error('[playlistCovers] failed to load:', e);
        return {};
    }
}

function saveToStorage(map: CoversMap) {
    if (typeof window === 'undefined') return;
    try {
        localStorage.setItem(STORAGE_KEY, JSON.stringify(map));
    } catch (e) {
        console.error('[playlistCovers] failed to save:', e);
    }
}

const initial = {};
export const playlistCovers = writable<CoversMap>(initial);

export function initializePlaylistCovers(): () => void {
    playlistCovers.set(loadFromStorage());
    return playlistCovers.subscribe(saveToStorage);
}

export function setPlaylistCover(playlistId: number, dataUrl: string) {
    playlistCovers.update((m) => ({ ...m, [playlistId]: dataUrl }));
}

export function removePlaylistCover(playlistId: number) {
    playlistCovers.update((m) => {
        const copy = { ...m };
        delete copy[playlistId];
        return copy;
    });
}

export function getPlaylistCoverSync(map: CoversMap, playlistId: number): string | null {
    return map && map[playlistId] ? map[playlistId] : null;
}
