import { writable, derived, get } from 'svelte/store';
import {
    addAlbumToListenLater,
    removeAlbumFromListenLater,
    getListenLaterAlbumIds,
} from '$lib/api/tauri';

export const listenLaterAlbumIds = writable<Set<number>>(new Set());
export const listenLaterAlbumOrder = writable<number[]>([]);

export const listenLaterCount = derived(listenLaterAlbumIds, ($ids) => $ids.size);

export async function loadListenLaterAlbums(): Promise<void> {
    try {
        const ids = await getListenLaterAlbumIds();
        listenLaterAlbumOrder.set(ids);
        listenLaterAlbumIds.set(new Set(ids));
    } catch (error) {
        console.error('[ListenLater] Failed to load albums:', error);
    }
}

export function isInListenLater(albumId: number): boolean {
    return get(listenLaterAlbumIds).has(albumId);
}

export async function toggleListenLater(albumId: number): Promise<void> {
    const currentIds = get(listenLaterAlbumIds);
    const currentOrder = get(listenLaterAlbumOrder);
    const wasSaved = currentIds.has(albumId);

    const nextIds = new Set(currentIds);
    const nextOrder = currentOrder.slice();

    if (wasSaved) {
        nextIds.delete(albumId);
        const idx = nextOrder.indexOf(albumId);
        if (idx >= 0) nextOrder.splice(idx, 1);
    } else {
        nextIds.add(albumId);
        nextOrder.unshift(albumId);
    }

    listenLaterAlbumIds.set(nextIds);
    listenLaterAlbumOrder.set(nextOrder);

    try {
        if (wasSaved) {
            await removeAlbumFromListenLater(albumId);
        } else {
            await addAlbumToListenLater(albumId);
        }
    } catch (error) {
        console.error('[ListenLater] Failed to toggle album:', error);
        listenLaterAlbumIds.set(currentIds);
        listenLaterAlbumOrder.set(currentOrder);
    }
}
