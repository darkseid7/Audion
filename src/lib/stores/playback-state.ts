/** Passive singleton. Views receive subscriptions; domain code alone uses the writer. */
import { writable, derived, readonly } from "svelte/store";
import type { Track } from "$lib/api/tauri";
import { EventEmitter, type PluginEvents } from "$lib/plugins/event-emitter";
export type ActiveBackend = "native" | "html5" | "remote" | "squeeze" | "none";
export interface PlaybackContext {
    type: "playlist" | "album" | "artist" | "liked" | "track" | "queue";
    playMode?: "all" | "liked_only";
    trackId?: number;
    /** For playlists: playlist ID */
    playlistId?: number;
    /** For albums: album ID */
    albumId?: number;
    /** For artists: artist name */
    artistName?: string;
    /** Display name for UI */
    displayName?: string;
}
const activeBackend = writable<ActiveBackend>("none");
const playbackContext = writable<PlaybackContext | null>(null);
const currentTrack = writable<Track | null>(null);
const isPlaying = writable(false);
const queue = writable<Track[]>([]);
const queueIndex = writable(0);
const userQueueCount = writable(0);
const volume = writable(0.7);
const currentTime = writable(0);
const duration = writable(0);
const shuffle = writable(false);
const repeat = writable<"none" | "one" | "all">("none");
const shuffledIndices = writable<number[]>([]);
const shuffledIndex = writable<number>(0);
/** @internal Projection writes for desktop execution and future controller snapshots. */
export const playbackStateWriter = { activeBackend, playbackContext, currentTrack, isPlaying, queue, queueIndex, userQueueCount, volume, currentTime, duration, shuffle, repeat, shuffledIndices, shuffledIndex };
const activeBackendView = readonly(activeBackend);
const playbackContextView = readonly(playbackContext);
const currentTrackView = readonly(currentTrack);
const isPlayingView = readonly(isPlaying);
const queueView = readonly(queue);
const queueIndexView = readonly(queueIndex);
const userQueueCountView = readonly(userQueueCount);
const volumeView = readonly(volume);
const currentTimeView = readonly(currentTime);
const durationView = readonly(duration);
const shuffleView = readonly(shuffle);
const repeatView = readonly(repeat);
const shuffledIndicesView = readonly(shuffledIndices);
const shuffledIndexView = readonly(shuffledIndex);
export { activeBackendView as activeBackend, playbackContextView as playbackContext, currentTrackView as currentTrack, isPlayingView as isPlaying, queueView as queue, queueIndexView as queueIndex, userQueueCountView as userQueueCount, volumeView as volume, currentTimeView as currentTime, durationView as duration, shuffleView as shuffle, repeatView as repeat, shuffledIndicesView as shuffledIndices, shuffledIndexView as shuffledIndex };
export const currentPlaylistId = derived(playbackContext, ($ctx) => $ctx?.type === "playlist" ? ($ctx.playlistId ?? null) : null);
export const currentAlbumId = derived(playbackContext, ($ctx) => $ctx?.type === "album" ? ($ctx.albumId ?? null) : null);
export const currentArtistName = derived(playbackContext, ($ctx) => $ctx?.type === "artist" ? ($ctx.artistName ?? null) : null);
export const currentTrackId = derived(currentTrack, ($t) => $t?.id ?? null);
export const progress = derived([currentTime, duration], ([$currentTime, $duration]) => {
    if (!$duration || $duration === 0)
        return 0;
    return $currentTime / $duration;
});
export const pluginEvents = new EventEmitter<PluginEvents>();
