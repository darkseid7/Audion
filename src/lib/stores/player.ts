export { isStreaming, sliderToAudioVolume, audioVolumeToSlider } from "$lib/application/playback-helpers";
/** Shared typed facade. Never import the desktop runtime here. */
import { get } from "svelte/store";
import { getApplicationPort } from "$lib/application/port";
import type { ApplicationIntent, CommandPreconditions } from "$lib/application/types";
import type { Track } from "$lib/api/tauri";
import { isPlaying, shuffle, repeat, playbackContext, duration } from "./playback-state";
import type { PlaybackContext } from "./playback-state";
export { activeBackend, playbackContext, currentTrack, isPlaying, queue, queueIndex, userQueueCount, volume, currentTime, duration, shuffle, repeat, shuffledIndices, shuffledIndex, currentPlaylistId, currentAlbumId, currentArtistName, currentTrackId, progress, pluginEvents } from "./playback-state";
export type { ActiveBackend, PlaybackContext } from "./playback-state";
// Task 3 installs snapshot-backed preconditions together with the desktop adapter.
let preconditions: CommandPreconditions | undefined;
export function setPlayerPreconditions(value: CommandPreconditions | undefined): void { preconditions = value; }
async function dispatch(intent: ApplicationIntent): Promise<void> {
    if (!preconditions)
        throw new Error("Application unavailable");
    const result = await getApplicationPort().execute(intent, preconditions);
    if (result.status === "failed" || result.status === "superseded")
        throw new Error(result.error.message);
}
function unavailable(): never { throw new Error("Application capability unavailable"); }
export const pause = () => dispatch({ type: "pause" });
export const resume = () => dispatch({ type: "resume" });
export const togglePlay = () => get(isPlaying) ? pause() : resume();
export const nextTrack = () => dispatch({ type: "next" });
export const previousTrack = () => dispatch({ type: "previous" });
export const seek = (position: number) => dispatch({ type: "seek", seconds: position * get(duration) });
export const setVolume = (volume: number) => dispatch({ type: "set_volume", volume });
export const setShuffle = (enabled: boolean) => dispatch({ type: "set_shuffle", enabled });
export const toggleShuffle = () => setShuffle(!get(shuffle));
export const cycleRepeat = () => dispatch({ type: "set_repeat", mode: get(repeat) === "none" ? "all" : get(repeat) === "all" ? "one" : "none" });
export async function playTrack(track: Track, skipLocalSrc = false, startTime = 0): Promise<void> {
    if (skipLocalSrc || startTime !== 0) unavailable();
    await dispatch({ type: "play_track", trackId: track.id });
}
export const addToQueue = (tracks: Track[]) => dispatch({ type: "queue_insert", trackIds: tracks.map(t => t.id), placement: "after_user_queue" });
export const playNext = (tracks: Track[]) => dispatch({ type: "queue_insert", trackIds: tracks.map(t => t.id), placement: "next" });
export const appendToQueueEnd = (tracks: Track[]) => dispatch({ type: "queue_append", trackIds: tracks.map(t => t.id) });
export const clearUpcoming = () => dispatch({ type: "queue_clear_upcoming" });
// Legacy list/index/cloud APIs need a desktop-only binding in Task 3. No local fallback.
export function playTracks(_tracks: Track[], _index = 0, _context?: PlaybackContext): void { unavailable(); }
export function removeFromQueue(_index: number): void { unavailable(); }
export function reorderQueue(_from: number, _to: number): void { unavailable(); }
export function playFromQueue(_index: number): void { unavailable(); }
export async function transferPlayback(_state: unknown): Promise<void> { unavailable(); }
export function sendRemoteCommand(_target: string, _command: string, _data?: unknown): void { unavailable(); }
export function shutdownPlayer(): void { unavailable(); }
export function isPlaylistPlaying(playlistId: number): boolean {
  const ctx = get(playbackContext);
  return ctx?.type === "playlist" && ctx.playlistId === playlistId;
}
export function isAlbumPlaying(albumId: number): boolean {
  const ctx = get(playbackContext);
  return ctx?.type === "album" && ctx.albumId === albumId;
}
export function isArtistPlaying(artistName: string): boolean {
  const ctx = get(playbackContext);
  return ctx?.type === "artist" && ctx.artistName === artistName;
}
export function selectThisDevice(): void { unavailable(); }
export function toggleRemoteControl(_device: import("./websocket").RemoteDevice): void { unavailable(); }
