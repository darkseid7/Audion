import { writable, get } from "svelte/store";
import { likeAlbum, unlikeAlbum, getLikedAlbumIds } from "$lib/api/tauri";

export const likedAlbumIds = writable<Set<number>>(new Set());

export async function loadLikedAlbums(): Promise<void> {
  try {
    const ids = await getLikedAlbumIds();
    likedAlbumIds.set(new Set(ids));
  } catch (error) {
    console.error("[LikedAlbums] Failed to load:", error);
  }
}

export function isAlbumLiked(albumId: number): boolean {
  return get(likedAlbumIds).has(albumId);
}

export async function toggleAlbumLike(albumId: number): Promise<void> {
  const currentIds = get(likedAlbumIds);
  const wasLiked = currentIds.has(albumId);

  const newIds = new Set(currentIds);
  if (wasLiked) {
    newIds.delete(albumId);
  } else {
    newIds.add(albumId);
  }
  likedAlbumIds.set(newIds);

  try {
    if (wasLiked) {
      await unlikeAlbum(albumId);
    } else {
      await likeAlbum(albumId);
    }
  } catch (error) {
    console.error("[LikedAlbums] Failed to toggle:", error);
    likedAlbumIds.set(currentIds);
  }
}
