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
/** Installed only by desktop bootstrap; controller mode never loads an executor. */
export interface DesktopPlayerLink {
  execute(intent: ApplicationIntent): Promise<void>;
  addToQueue(tracks: Track[]): Promise<void>;
  playNext(tracks: Track[]): Promise<void>;
  appendToQueueEnd(tracks: Track[]): Promise<void>;
  playTrack(track: Track, skipLocalSrc?: boolean, startTime?: number): Promise<void>;
  playTracks(tracks: Track[], index?: number, context?: PlaybackContext): Promise<void>;
  removeFromQueue(index: number): Promise<void>;
  reorderQueue(from: number, to: number): Promise<void>;
  playFromQueue(index: number): Promise<void>;
  transferPlayback(state: unknown): Promise<void>;
  sendRemoteCommand(target: string, command: string, data?: unknown): Promise<void>;
  shutdownPlayer(): Promise<void>;
  selectThisDevice(): Promise<void>;
  toggleRemoteControl(device: import("./websocket").RemoteDevice): Promise<void>;
}
let desktop: DesktopPlayerLink | undefined;
export function registerDesktopPlayer(link: DesktopPlayerLink): () => void {
  desktop = link;
  return () => { if (desktop === link) desktop = undefined; };
}
function desktopOnly(): DesktopPlayerLink {
  if (!desktop) throw new Error("Application capability unavailable");
  return desktop;
}
// Task 3 installs snapshot-backed preconditions together with the desktop adapter.
let preconditions: CommandPreconditions | undefined;
export function setPlayerPreconditions(value: CommandPreconditions | undefined): void { preconditions = value; }
async function dispatch(intent: ApplicationIntent): Promise<void> {
    if (desktop) return desktop.execute(intent);
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
    if (desktop) return desktop.playTrack(track, skipLocalSrc, startTime);
    if (skipLocalSrc || startTime !== 0) unavailable();
    await dispatch({ type: "play_track", trackId: track.id });
}
export const addToQueue = (tracks: Track[]) => desktop ? desktop.addToQueue(tracks) : dispatch({ type: "queue_insert", trackIds: tracks.map(t => t.id), placement: "after_user_queue" });
export const playNext = (tracks: Track[]) => desktop ? desktop.playNext(tracks) : dispatch({ type: "queue_insert", trackIds: tracks.map(t => t.id), placement: "next" });
export const appendToQueueEnd = (tracks: Track[]) => desktop ? desktop.appendToQueueEnd(tracks) : dispatch({ type: "queue_append", trackIds: tracks.map(t => t.id) });
export const clearUpcoming = () => dispatch({ type: "queue_clear_upcoming" });
export const playTracks = (tracks: Track[], index = 0, context?: PlaybackContext) => desktopOnly().playTracks(tracks, index, context);
export const removeFromQueue = (index: number) => desktopOnly().removeFromQueue(index);
export const reorderQueue = (from: number, to: number) => desktopOnly().reorderQueue(from, to);
export const playFromQueue = (index: number) => desktopOnly().playFromQueue(index);
export const transferPlayback = (state: unknown) => desktopOnly().transferPlayback(state);
export const sendRemoteCommand = (target: string, command: string, data?: unknown) => desktopOnly().sendRemoteCommand(target, command, data);
export const shutdownPlayer = () => desktopOnly().shutdownPlayer();
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
export const selectThisDevice = () => desktopOnly().selectThisDevice();
export const toggleRemoteControl = (device: import("./websocket").RemoteDevice) => desktopOnly().toggleRemoteControl(device);
