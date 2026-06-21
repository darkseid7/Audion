// Activity store - manages play history and activity data
import { writable, get } from 'svelte/store';
import { albums as libraryAlbums } from '$lib/stores/library';
import {
    recordPlay,
    getTopTracks,
    getTopAlbums,
    getRecentlyPlayed,
    getRecentlyPlayedAlbums,
    getPlayedThisWeek,
    getTopArtists,
    getStatsSummary,
    type Track,
    type TrackWithCount,
    type Album,
    type AlbumWithCount,
    type ArtistWithCount,
    type StatsSummary,
} from '$lib/api/tauri';

export const topTracks = writable<TrackWithCount[]>([]);
export const topAlbums = writable<AlbumWithCount[]>([]);
export const topArtists = writable<ArtistWithCount[]>([]);
export const recentlyPlayed = writable<Track[]>([]);
export const recentlyPlayedAlbums = writable<Album[]>([]);
export const thisWeekPlayed = writable<Track[]>([]);
export const statsSummary = writable<StatsSummary | null>(null);
export const isLoadingActivity = writable<boolean>(false);

/**
 * Dedupe a list of tracks (ordered most-recent-first) to unique albums,
 * preserving the most-recent play order. Tracks without an album_id are
 * skipped (they would have no artwork anyway).
 *
 * Library lookup is done against the global `albums` store so the result
 * matches the existing carousel pattern.
 */
export function dedupeTracksToAlbums(
    tracks: Track[],
    limit?: number,
): Album[] {
    const library = get(libraryAlbums);
    const seen = new Map<number, Album>();
    for (const t of tracks) {
        if (t.album_id == null) continue;
        if (seen.has(t.album_id)) continue;
        const a = library.find((x) => x.id === t.album_id);
        if (a) seen.set(t.album_id, a);
        if (limit != null && seen.size >= limit) break;
    }
    return Array.from(seen.values());
}

// Record a play event for a track
export async function recordTrackPlay(trackId: number, albumId: number | null, durationPlayed: number): Promise<void> {
    // Only record for tracks with numeric IDs (library tracks)
    if (typeof trackId !== 'number') {
        return;
    }

    try {
        await recordPlay(trackId, albumId, durationPlayed);
    } catch (error) {
        console.error('[Activity] Failed to record play:', error);
    }
}

// Load activity data (top tracks, top albums, recently played)
export async function loadActivityData(): Promise<void> {
    if (get(isLoadingActivity)) return;

    isLoadingActivity.set(true);
    try {
        const [topT, topA, topArt, recent, recentAlbums, thisWeek, stats] =
            await Promise.all([
                getTopTracks(50),
                getTopAlbums(20),
                getTopArtists(20),
                getRecentlyPlayed(20),
                getRecentlyPlayedAlbums(12),
                // Fetch more than the visible cap so the frontend knows
                // whether to show a "View all" link when the section
                // "fills up".
                getPlayedThisWeek(40),
                getStatsSummary(),
            ]);
        topTracks.set(topT);
        topAlbums.set(topA);
        topArtists.set(topArt);
        recentlyPlayed.set(recent);
        recentlyPlayedAlbums.set(recentAlbums);
        thisWeekPlayed.set(thisWeek);
        statsSummary.set(stats);
    } catch (error) {
        console.error('[Activity] Failed to load activity data:', error);
    } finally {
        isLoadingActivity.set(false);
    }
}

// Refresh just recently played (lightweight)
export async function refreshRecentlyPlayed(): Promise<void> {
    try {
        const [recent, recentAlbums, thisWeek] = await Promise.all([
            getRecentlyPlayed(20),
            getRecentlyPlayedAlbums(12),
            getPlayedThisWeek(40),
        ]);
        recentlyPlayed.set(recent);
        recentlyPlayedAlbums.set(recentAlbums);
        thisWeekPlayed.set(thisWeek);
    } catch (error) {
        console.error('[Activity] Failed to refresh recently played:', error);
    }
}
