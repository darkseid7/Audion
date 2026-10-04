import { PlaybackFailure } from "./playback-coordinator";
import { isStreaming, sliderToAudioVolume, audioVolumeToSlider } from "../playback-helpers";
export { isStreaming, sliderToAudioVolume, audioVolumeToSlider } from "../playback-helpers";
import { disconnectSqueezePlayer } from "$lib/stores/squeeze";
import type { RemoteDevice } from "$lib/stores/websocket";
import { playbackStateWriter, pluginEvents, currentPlaylistId, currentAlbumId, currentArtistName, currentTrackId, progress, type PlaybackContext, type ActiveBackend } from "$lib/stores/playback-state";
export { pluginEvents, currentPlaylistId, currentAlbumId, currentArtistName, currentTrackId, progress };
export type { PlaybackContext, ActiveBackend };
export const { activeBackend, playbackContext, currentTrack, isPlaying, queue, queueIndex, userQueueCount, volume, currentTime, duration, shuffle, repeat, shuffledIndices, shuffledIndex } = playbackStateWriter;
// Player store - manages audio playback state
import { writable, derived, get } from "svelte/store";
import { wsStore } from "$lib/stores/websocket";
import type { Track } from "$lib/api/tauri";
import {
  getAudioSrc,
  getAlbumArtSrc,
  getTrackCoverSrc,
  convertFileSrc,
  listen,
  initWindowsThumbar,
  updateWindowsThumbarState,
} from "$lib/api/tauri";
import { invoke } from "@tauri-apps/api/core";
import { addToast } from "$lib/stores/toast";
import {
  handleSleepTimerCheck,
  isTimerModeTrackOrAlbumEnd,
  isTimerModeAlbumEnd,
  stopSleepTimer,
} from "$lib/stores/sleepTimer";
import {
  tracks as libraryTracks,
  getFullTrack,
  getAlbumCoverFromTracks,
  updateTrackCover,
  getTrackByIdSync,
  incrementPlayCount,
} from "$lib/stores/library";
import { fetchTrackCover } from "$lib/services/cover-fetcher";
import { appSettings } from "$lib/stores/settings";
import {
  equalizer,
  EQ_FREQUENCIES,
  type EqualizerState,
} from "$lib/stores/equalizer";
import { pluginStore } from "$lib/stores/plugin-store";
import { recordTrackPlay } from "$lib/stores/activity";
import { submitListenbrainzListen } from "$lib/api/tauri";
import { activeRemoteDevice } from "$lib/stores/websocket";
import {
  activeSqueezePlayer,
  squeezePlayerState,
  setSqueezeVolumeCooldown,
  invalidateSqueezePollOwnership,
  captureSqueezeTargetOwnership,
  reconcileSqueezeAcknowledgement,
} from "$lib/stores/squeeze";
import { isInListenLater, toggleListenLater } from "$lib/stores/listen-later";
import {
  squeezePause,
  squeezeResume,
  squeezeNext,
  squeezePrevious,
  squeezeSeek,
  squeezeSetVolume,
  squeezeSetShuffle,
  squeezeSetRepeat,
  squeezePlay,
  squeezeInsertQueue,
  squeezeUpdateQueue,
} from "$lib/api/tauri";

// =============================================================================
// NATIVE AUDIO BACKEND
// =============================================================================
// Native Rust backend (rodio) is used when selected/available.
// HTML5/WebAudio is used for streaming paths and configured fallback.
// EQ is applied consistently across both playback pipelines.
// =============================================================================
import {
  nativeAudioPlay,
  nativeAudioPreload,
  nativeAudioPause,
  nativeAudioResume,
  nativeAudioStop,
  nativeAudioSetVolume,
  nativeAudioSeek,
  nativeAudioGetState,
  nativeAudioSetRepeatOne,
  nativeAudioPollEvent,
  type AudioEventType,
  nativeAudioSetEq,
  shouldUseNativeAudio,
  type NativePlaybackState,
} from "$lib/services/native-audio";

// Interval for polling native playback state
let nativeStatePoller: ReturnType<typeof setInterval> | null = null;

// Watchdog: if the native backend reports the position stuck at/past the
// track duration while still "playing", the TrackFinished event has been
// lost (or never fired for this format) and we must manually advance the
// queue. Reset on every new track and when an explicit end event arrives.
let pendingForcedEnd: ReturnType<typeof setTimeout> | null = null;
function clearPendingForcedEnd() {
  if (pendingForcedEnd !== null) {
    clearTimeout(pendingForcedEnd);
    pendingForcedEnd = null;
  }
}

// HTML5 Audio element for streaming (initialized lazily)
let html5Audio: HTMLAudioElement | null = null;
let html5Origin: { audio: HTMLAudioElement; session: number; emit?: CompletionEmitter } | undefined;
/** Called at the coordinator commit boundary, never at event delivery. */
export function refreshPlaybackSignalOwnership(): void {
  if (html5Origin && html5Origin.audio === html5Audio && html5Origin.session === currentSessionId) {
    html5Origin.emit = completionSink?.();
  }
}

// HTML5 WebAudio graph for EQ processing
let html5AudioContext: AudioContext | null = null;
let html5AudioSourceNode: MediaElementAudioSourceNode | null = null;
let html5EqFilters: BiquadFilterNode[] = [];
let html5EqGainNode: GainNode | null = null;
let lastEqBypassWarningHost: string | null = null;

// dash.js player instance for Hi-Res DASH/MPD streaming
let dashPlayer: any | null = null;

// Track which backend is currently active ('native', 'html5', 'remote', or 'none')
// Squeeze polling uses an epoch in addition to current store values so an
// A -> other -> A transition cannot let an old request publish.
activeBackend.subscribe(() => invalidateSqueezePollOwnership());

// Track if we should use native audio based on platform/settings
let nativeAudioUsed = false;

type AudioPathKind = "local" | "stream" | "blob" | "custom-scheme";

function classifyAudioPath(path: string): AudioPathKind {
  if (path.startsWith("blob:")) return "blob";
  if (path.startsWith("http://") || path.startsWith("https://"))
    return "stream";
  if (
    path.startsWith("file://") ||
    path.startsWith("asset://") ||
    path.startsWith("tauri://")
  )
    return "local";
  if (path.includes("://")) return "custom-scheme";
  return "local"; // absolute/relative filesystem path
}

// Lazily load dash.js and create a player instance
async function getDashPlayer(): Promise<any> {
  if (typeof window === "undefined") throw new Error("No window");

  // Load dash.js from CDN if not already loaded
  if (!(window as any).dashjs) {
    await new Promise<void>((resolve, reject) => {
      const script = document.createElement("script");
      script.src =
        "https://cdnjs.cloudflare.com/ajax/libs/dashjs/4.7.4/dash.all.min.js";
      script.onload = () => resolve();
      script.onerror = () => reject(new Error("Failed to load dash.js"));
      document.head.appendChild(script);
    });
  }

  return (window as any).dashjs;
}

async function playWithDash(
  blobUrl: string,
  audioElement: HTMLAudioElement,
): Promise<void> {
  if (dashPlayer) {
    try {
      dashPlayer.destroy();
    } catch (_) {}
    dashPlayer = null;
  }

  const mpdText = await fetch(blobUrl).then((r) => r.text());
  URL.revokeObjectURL(blobUrl);

  const bytes = new TextEncoder().encode(mpdText);
  const binary = Array.from(bytes).reduce(
    (acc, byte) => acc + String.fromCharCode(byte),
    "",
  );
  const dataUrl = "data:application/dash+xml;base64," + btoa(binary);

  const dashjs = await getDashPlayer();
  dashPlayer = dashjs.MediaPlayer().create();
  dashPlayer.initialize(audioElement, dataUrl, true);

  dashPlayer.on(dashjs.MediaPlayer.events.ERROR, (e: any) => {
    console.error("[Player] dash.js error:", e);
    addToast(
      `Hi-Res playback error: ${e.error?.message || "Unknown error"}`,
      "error",
    );
  });
}

function getHtml5Audio(): HTMLAudioElement {
  if (!html5Audio && typeof window !== "undefined") {
    html5Audio = new Audio();
    setupHtml5AudioListeners(html5Audio);
  }
  return html5Audio!;
}

function cleanupHtml5EqGraph(): void {
  if (html5AudioSourceNode) {
    try {
      html5AudioSourceNode.disconnect();
    } catch (_) {}
    html5AudioSourceNode = null;
  }
  html5EqFilters.forEach((filter) => {
    try {
      filter.disconnect();
    } catch (_) {}
  });
  html5EqFilters = [];
  if (html5EqGainNode) {
    try {
      html5EqGainNode.disconnect();
    } catch (_) {}
    html5EqGainNode = null;
  }
  if (html5AudioContext) {
    html5AudioContext.close().catch(() => {});
    html5AudioContext = null;
  }
}

function recreateHtml5AudioElement(): HTMLAudioElement {
  html5Origin = undefined;
  if (html5Audio) {
    html5Audio.pause();
    html5Audio.src = "";
  }

  cleanupHtml5EqGraph();

  html5Audio = new Audio();
  setupHtml5AudioListeners(html5Audio);
  return html5Audio;
}

function canUseHtml5EqForPath(path: string): boolean {
  if (typeof window === "undefined") return false;

  const kind = classifyAudioPath(path);
  if (kind === "local" || kind === "blob") return true;
  if (kind !== "stream") return true;

  // Cross-origin streams often block WebAudio processing without CORS headers.
  // To avoid silent playback, only allow same-origin streams in EQ graph.
  try {
    const url = new URL(path);
    return url.origin === window.location.origin;
  } catch {
    return false;
  }
}

async function prepareHtml5AudioForPath(
  audio: HTMLAudioElement,
  path: string,
): Promise<HTMLAudioElement> {
  const eqEnabled = get(equalizer).enabled;
  const canUseEq = canUseHtml5EqForPath(path);

  if (eqEnabled && canUseEq) {
    if (classifyAudioPath(path) === "stream") {
      audio.crossOrigin = "anonymous";
    }
    ensureHtml5EqGraph(audio);
    await resumeHtml5AudioContext();
    return audio;
  }

  // If this element is already attached to a WebAudio source node, it will stay routed
  // through that graph. Recreate the element to restore direct output when EQ must be bypassed.
  if (html5AudioSourceNode) {
    const next = recreateHtml5AudioElement();
    next.volume = audio.volume;
    audio = next;
  }

  if (eqEnabled && !canUseEq && classifyAudioPath(path) === "stream") {
    try {
      const host = new URL(path).host;
      if (lastEqBypassWarningHost !== host) {
        lastEqBypassWarningHost = host;
        addToast(
          "EQ is bypassed for this stream due to CORS restrictions",
          "warning",
        );
      }
    } catch {
      addToast(
        "EQ is bypassed for this stream due to CORS restrictions",
        "warning",
      );
    }
  }

  return audio;
}

function ensureHtml5EqGraph(audio: HTMLAudioElement): void {
  if (typeof window === "undefined") return;
  if (html5AudioSourceNode && html5EqGainNode && html5EqFilters.length > 0)
    return;

  try {
    if (!html5AudioContext) {
      const AudioContextCtor =
        (window as any).AudioContext || (window as any).webkitAudioContext;
      if (!AudioContextCtor) {
        console.warn("[EQ] WebAudio AudioContext is not available");
        return;
      }
      html5AudioContext = new AudioContextCtor();
    }
    const ctx = html5AudioContext;
    if (!ctx) return;

    if (!html5AudioSourceNode) {
      html5AudioSourceNode = ctx.createMediaElementSource(audio);
    }

    if (!html5EqGainNode) {
      html5EqGainNode = ctx.createGain();
      html5EqGainNode.gain.value = 1;
    }

    if (html5EqFilters.length === 0) {
      html5EqFilters = EQ_FREQUENCIES.map((freq) => {
        const filter = ctx.createBiquadFilter();
        filter.type = "peaking";
        filter.frequency.value = freq;
        filter.Q.value = 1.41;
        filter.gain.value = 0;
        return filter;
      });
    }

    try {
      html5AudioSourceNode.disconnect();
    } catch (_) {}
    html5EqFilters.forEach((filter) => {
      try {
        filter.disconnect();
      } catch (_) {}
    });
    try {
      html5EqGainNode.disconnect();
    } catch (_) {}

    html5AudioSourceNode.connect(html5EqFilters[0]);
    for (let i = 0; i < html5EqFilters.length - 1; i++) {
      html5EqFilters[i].connect(html5EqFilters[i + 1]);
    }
    html5EqFilters[html5EqFilters.length - 1].connect(html5EqGainNode);
    html5EqGainNode.connect(ctx.destination);

    applyHtml5EqState(equalizer.getState());
  } catch (err) {
    console.error("[EQ] Failed to initialize HTML5 EQ graph:", err);
    html5AudioSourceNode = null;
    html5EqFilters = [];
    html5EqGainNode = null;
  }
}

function applyHtml5EqState(state: EqualizerState): void {
  if (!html5AudioContext || html5EqFilters.length === 0) return;

  const now = html5AudioContext.currentTime;
  for (let i = 0; i < html5EqFilters.length; i++) {
    const gain = state.enabled ? (state.bands[i]?.gain ?? 0) : 0;
    html5EqFilters[i].gain.cancelScheduledValues(now);
    html5EqFilters[i].gain.setTargetAtTime(gain, now, 0.01);
  }
}

async function resumeHtml5AudioContext(): Promise<void> {
  if (!html5AudioContext || html5AudioContext.state !== "suspended") return;

  try {
    await html5AudioContext.resume();
  } catch (err) {
    console.warn("[EQ] Failed to resume HTML5 AudioContext:", err);
  }
}

function setupHtml5AudioListeners(audio: HTMLAudioElement): void {
  const origin = { audio, session: currentSessionId, emit: completionSink?.() };
  html5Origin = origin;
  const owns = () => html5Origin === origin && html5Audio === audio
    && origin.session === currentSessionId && get(activeBackend) === "html5";
  audio.addEventListener("timeupdate", () => {
    if (owns()) {
      currentTime.set(audio.currentTime);
    }
  });

  audio.addEventListener("durationchange", () => {
    if (owns()) {
      if (audio.duration && !isNaN(audio.duration)) {
        duration.set(audio.duration);
      }
    }
  });

  audio.addEventListener("play", () => {
    if (owns()) {
      isPlaying.set(true);
      updateMediaSessionPlaybackState("playing");
    }
  });

  audio.addEventListener("pause", () => {
    if (owns()) {
      isPlaying.set(false);
      updateMediaSessionPlaybackState("paused");
    }
  });

  audio.addEventListener("ended", () => {
    if (owns()) {
      // The captured emitter belongs to this media session, not whichever
      // occurrence happens to own the coordinator when a delayed event arrives.
      void (origin.emit ? origin.emit("completion") : applyTrackEnd()).catch(console.error);
    }
  });

  audio.addEventListener("error", (e) => {
    if (owns()) {
      console.error("[Player] HTML5 audio error:", audio.error);
      addToast(
        `Streaming playback failed: ${audio.error?.message || "Unknown error"}`,
        "error",
      );
    }
  });
}

// =============================================================================
// PLAYLIST URL RESOLUTION
// =============================================================================
// Many radio stations provide playlist files (.m3u, .m3u8, .pls) that contain
// the actual stream URL. HTML5 <audio> cannot parse these formats directly,
// so we fetch them and extract the real stream URL before playback.
// =============================================================================

/** Known playlist file extensions that need resolution */
const PLAYLIST_EXTENSIONS = [".m3u", ".m3u8", ".pls"];

/**
 * Check if a URL points to a playlist file that needs resolution.
 * Strips query strings and fragments before checking the extension.
 */
function isPlaylistUrl(url: string): boolean {
  try {
    const pathname = new URL(url).pathname.toLowerCase();
    return PLAYLIST_EXTENSIONS.some((ext) => pathname.endsWith(ext));
  } catch {
    // Fallback for malformed URLs: check the raw string
    const lower = url.toLowerCase().split("?")[0].split("#")[0];
    return PLAYLIST_EXTENSIONS.some((ext) => lower.endsWith(ext));
  }
}

/**
 * Parse a .pls playlist file and return the first stream URL.
 * PLS format: INI-like with File1=<url>, File2=<url>, etc.
 */
function parsePlsPlaylist(text: string): string | null {
  for (const line of text.split(/\r?\n/)) {
    const trimmed = line.trim();
    const match = trimmed.match(/^File\d+\s*=\s*(.+)$/i);
    if (match && match[1].startsWith("http")) {
      return match[1].trim();
    }
  }
  return null;
}

/**
 * Parse an .m3u/.m3u8 playlist file and return the first stream URL.
 * If the file is a true HLS manifest (contains #EXT-X- directives),
 * returns null so the original URL is used as-is — browsers/WebViews
 * often handle HLS natively.
 */
function parseM3uPlaylist(text: string): string | null {
  const lines = text.split(/\r?\n/);

  // Detect true HLS manifests — these should be played directly
  const isHls = lines.some((l) => l.trim().startsWith("#EXT-X-"));
  if (isHls) return null; // Let the browser handle HLS natively

  // Simple M3U: find the first http(s) URL line
  for (const line of lines) {
    const trimmed = line.trim();
    if (
      trimmed &&
      !trimmed.startsWith("#") &&
      (trimmed.startsWith("http://") || trimmed.startsWith("https://"))
    ) {
      return trimmed;
    }
  }
  return null;
}

/**
 * Resolve a playlist URL to a direct stream URL.
 * If the URL isn't a playlist format or resolution fails, returns the
 * original URL unchanged so playback can still be attempted.
 */
async function resolvePlaylistUrl(url: string): Promise<string> {
  if (!isPlaylistUrl(url)) return url;

  console.log(`[Player] Resolving playlist URL: ${url}`);

  try {
    const response = await fetch(url, {
      signal: AbortSignal.timeout(8000), // 8s timeout for slow servers
    });

    if (!response.ok) {
      console.warn(
        `[Player] Playlist fetch failed (${response.status}), using original URL`,
      );
      return url;
    }

    const text = await response.text();
    const lower = url.toLowerCase().split("?")[0].split("#")[0];
    let resolved: string | null = null;

    if (lower.endsWith(".pls")) {
      resolved = parsePlsPlaylist(text);
    } else {
      // .m3u or .m3u8
      resolved = parseM3uPlaylist(text);
    }

    if (resolved) {
      console.log(`[Player] Resolved playlist URL: ${url} → ${resolved}`);
      return resolved;
    }

    // Could be HLS or empty playlist — use original URL
    console.log(
      `[Player] Playlist did not yield a direct URL (may be HLS), using original`,
    );
    return url;
  } catch (err) {
    console.warn(
      `[Player] Playlist resolution failed, using original URL:`,
      err,
    );
    return url;
  }
}

// Playback session tracking
let currentSessionId = 0;
let observationGeneration = 0;
export function invalidateDesktopObservations(): void { observationGeneration += 1; }

// Track play start time for accurate duration recording
let playStartTime: number = 0;

// Track the last preloaded path so handleGaplessAdvance can detect mismatches
let lastPreloadedPath: string | null = null;

// Current time and duration

// Shuffle and repeat

// Subscribe to EQ changes to update native backend
// Debounced subscription: batch rapid EQ changes and avoid thrashing
let _eqApplyTimer: ReturnType<typeof setTimeout> | null = null;
let _latestEqState: any = null;
equalizer.subscribe((state) => {
  _latestEqState = state;

  // Apply immediately to HTML5 WebAudio graph when available
  applyHtml5EqState(state);

  // Only attempt to apply when native backend is active
  if (get(activeBackend) !== "native") return;

  // Debounce rapid updates (200ms)
  if (_eqApplyTimer) clearTimeout(_eqApplyTimer);
  _eqApplyTimer = setTimeout(async () => {
    try {
      await nativeAudioSetEq(_latestEqState);
    } catch (err) {
      console.error("[EQ] Failed to apply settings:", err);
    } finally {
      _eqApplyTimer = null;
    }
  }, 200);
});

// =============================================================================
// BACKEND INITIALIZATION
// =============================================================================
const runtimeSubscriptions: (() => void)[] = [];
let audioInitialized = false;

export async function initAudioBackend(): Promise<void> {
  if (audioInitialized) return;
  audioInitialized = true;
  console.log("[Player] Initializing audio backend");

  // Check if we should use native audio
  nativeAudioUsed = await shouldUseNativeAudio();
  console.log(`[Player] Native audio preferred: ${nativeAudioUsed}`);

  // Start/stop poller based on playback state and notify remote devices
  runtimeSubscriptions.push(isPlaying.subscribe((playing) => {
    updateWindowsThumbarState(playing).catch(() => {});

    // Force an immediate broadcast when play/pause state changes
    // so remote Connect Panels stay perfectly in sync
    broadcastState(true);
    const backend = get(activeBackend);
    if (playing) {
      if (backend !== "remote" && backend !== "squeeze") {
        startStatePoller();
      }
    } else {
      stopStatePoller();
    }
  }));

  // Also force broadcast when the actual track changes regardless of play state
  runtimeSubscriptions.push(currentTrack.subscribe(() => {
    broadcastState(true);
  }));

  // Subscribe to volume changes to keep backends in sync
  runtimeSubscriptions.push(volume.subscribe((val) => {
    const backend = get(activeBackend);
    if (backend === "squeeze" || backend === "remote") return;

    const audioVol = sliderToAudioVolume(val);

    // Update HTML5 backend
    if (html5Audio) {
      html5Audio.volume = audioVol;
    }

    // Update Native backend
    if (nativeAudioUsed) {
      nativeAudioSetVolume(audioVol).catch((err) => {
        console.warn("[Player] Failed to set native volume:", err);
      });
    }
  }));

  // Force sync initial volume to native backend so it matches
  // the frontend's logarithmic curve from the start, before any track plays.
  if (nativeAudioUsed) {
    nativeAudioSetVolume(sliderToAudioVolume(get(volume))).catch((err) => {
      console.warn("[Player] Failed to set initial native volume:", err);
    });
  }

  // If native backend is available, apply current EQ state once to ensure
  // native side has the latest settings (prevents mismatch / thrash on first play)
  if (nativeAudioUsed) {
    try {
      // Use equalizer.getState() to get the current stored state
      const state = equalizer.getState();
      nativeAudioSetRepeatOne(get(repeat) === "one").catch(console.error);
      await nativeAudioSetEq(state);
      console.log("[Player] Applied initial EQ settings to native backend");
    } catch (err) {
      console.warn("[Player] Failed to apply initial EQ settings:", err);
    }
  }

  // Subscribe to WebSocket messages
  runtimeSubscriptions.push(wsStore.onMessage((type, payload) => {
    switch (type) {
      case "transfer_playback":
        void (transferSink ? transferSink(payload) : Promise.reject(new Error("Desktop transfer ingress is not ready"))).catch(console.error);
        break;
      case "remote_command":
        void handleRemoteCommand(payload).catch(console.error);
        break;
      case "player_state":
        handleRemotePlayerState(payload);
        break;
    }
  }));

  // Sub due to initialization of active setting
  runtimeSubscriptions.push(activeBackend.subscribe((b) => {
    if (b === "remote" || b === "squeeze") {
      stopStatePoller();
    }
  }));

  await initWindowsThumbarIntegration();
}

function handleRemotePlayerState(payload: any) {
  const isLocalPlaying = get(isPlaying) && get(activeBackend) !== "remote";

  // Auto-switch to tracking the remote device if we are idle and it's playing
  if (!isLocalPlaying && payload.isPlaying && payload.deviceId) {
    if (get(activeBackend) !== "remote") {
      activeBackend.set("remote");
      activeRemoteDevice.set(payload.deviceId);
      console.log(
        `[Player] Auto-switched to remote session for device: ${payload.deviceId}`,
      );
    }
  }

  // If we are tracking THIS remote device, pipe the state into the local UI variables
  if (
    get(activeBackend) === "remote" &&
    get(activeRemoteDevice) === payload.deviceId
  ) {
    if (payload.track) {
      const remoteTrack = payload.track;
      const currentObj = get(currentTrack);
      const remoteTrackId = Number(remoteTrack.id);

      if (!currentObj || Number(currentObj.id) !== remoteTrackId) {
        // Try to resolve track locally for better cover art (Fast O(1) lookup)
        let localTrack: any = getTrackByIdSync(remoteTrackId);

        // Falling back to O(N) search only if ID fails (rare in synced libraries)
        if (!localTrack) {
          const $library = get(libraryTracks);
          localTrack = $library.find(
            (t) =>
              t.title === remoteTrack.title && t.artist === remoteTrack.artist,
          );
        }

        currentTrack.set({
          ...remoteTrack,
          ...(localTrack || {}),
          id: remoteTrackId, // Ensure ID is a number
          track_cover: localTrack
            ? getTrackCoverSrc(localTrack)
            : remoteTrack.coverUrl,
        } as any);
      }
    } else {
      if (get(currentTrack) !== null) currentTrack.set(null);
    }

    // Only update these if they changed to prevent spamming subscribers
    if (get(isPlaying) !== payload.isPlaying) isPlaying.set(payload.isPlaying);

    // Only update time if the difference is significant (>250ms or specifically requested)
    const currentT = get(currentTime);
    if (
      Math.abs(currentT - payload.currentTime) > 0.25 ||
      payload.isPlaying === false
    ) {
      currentTime.set(payload.currentTime);
    }

    if (get(duration) !== payload.duration) duration.set(payload.duration);

    if (payload.volume !== undefined && get(volume) !== payload.volume)
      volume.set(payload.volume);
    if (payload.shuffle !== undefined && get(shuffle) !== payload.shuffle)
      shuffle.set(payload.shuffle);
    if (payload.repeat !== undefined && get(repeat) !== payload.repeat)
      repeat.set(payload.repeat);
  }
}

// Poll the native backend for state changes (only while playing)
const POLL_INTERVAL_MS = 50;

function startStatePoller(): void {
  if (nativeStatePoller) return;

  nativeStatePoller = setInterval(async () => {
    const emitCompletion = completionSink?.();
    const session = currentSessionId;
    const observation = observationGeneration;
    try {
      const track = get(currentTrack);
      if (!track) return;

      if (get(activeBackend) === "native") {
        const state = await nativeAudioGetState();
        if (observation !== observationGeneration || session !== currentSessionId || get(activeBackend) !== "native") return;
        const uiDuration = get(duration);

        // Only advance currentTime while the backend reports playback.
        // Otherwise we can keep pushing the position past duration when the
        // last track of an album has ended but the UI hasn't caught up yet,
        // making the counter appear to run to infinity.
        //
        // Clamp to the UI duration (from the database) so the counter never
        // runs past the displayed end. The Rust backend can fail to detect a
        // file's duration (e.g. some VBR MP3s return n_frames=None), in
        // which case position_secs() in audio.rs is unclamped and grows
        // indefinitely — without this clamp the counter visibly counts to
        // infinity on the last track of an album.
        if (state.is_playing) {
          currentTime.set(
            uiDuration > 0 ? Math.min(state.position, uiDuration) : state.position,
          );
        } else if (
          get(isPlaying) &&
          ((state.duration > 0 && state.position >= state.duration) ||
            (uiDuration > 0 && state.position >= uiDuration))
        ) {
          // Backend stopped at/past the end — snap UI to track end.
          currentTime.set(state.duration > 0 ? state.duration : uiDuration);
        }
        if (state.duration > 0) {
          duration.set(state.duration);
        } else if (uiDuration > 0) {
          // Backend has no duration but the DB does — keep the UI duration
          // we already trust so the progress bar / counter remain stable.
          duration.set(uiDuration);
        } else {
          console.warn(
            "[Poller] Native backend reported 0 duration for track at:",
            state.position,
          );
        }

        // Watchdog: if the backend keeps reporting is_playing=true with the
        // position stuck at/past the track end, the TrackFinished event has
        // been lost (e.g. last track of an album where the rodio queue never
        // completes for some formats). Schedule a forced end-of-track after a
        // short delay so the counter doesn't run to infinity and the queue
        // actually advances / stops.
        //
        // Fall back to the UI duration when the backend couldn't determine
        // the file's duration — otherwise the watchdog never fires for those
        // files and the counter is left running past the end.
        const effectiveDuration =
          state.duration > 0 ? state.duration : uiDuration;
        if (
          state.is_playing &&
          effectiveDuration > 0 &&
          state.position >= effectiveDuration - 0.05 // tiny tolerance
        ) {
          if (pendingForcedEnd === null) {
            pendingForcedEnd = setTimeout(() => {
              pendingForcedEnd = null;
              if (observation !== observationGeneration || session !== currentSessionId || get(activeBackend) !== "native") return;
              const st = get(currentTime);
              const du = get(duration);
              if (du > 0 && st >= du - 0.1 && get(isPlaying)) {
                console.warn(
                  "[Player] Forced track end via watchdog (backend missed TrackFinished event)",
                );
                // Make sure the native backend actually stops playing — if
                // it's stuck emitting silence past the end, this is the only
                // way to make the queue advance to the next track (or stop).
                handleTrackEnd(emitCompletion);
              }
            }, 400);
          }
        } else {
          clearPendingForcedEnd();
        }

        // Poll for audio events
        const event = await nativeAudioPollEvent();
        if (observation !== observationGeneration || session !== currentSessionId || get(activeBackend) !== "native") return;

        if (event.type === "TrackFinished") {
          // Track ended naturally, nothing was preloaded.
          clearPendingForcedEnd();
          handleTrackEnd(emitCompletion);
        } else if (event.type === "TrackAdvanced") {
          // Gapless advance: audio backend already moved to the next track.
          // We must NOT call nativeAudioPlay() — that would restart it.
          // Just advance the UI queue index and update metadata.
          clearPendingForcedEnd();
          handleGaplessAdvance(emitCompletion);
        } else if (event.type === "StateChanged") {
          // Backend confirmed a seek or loop — update UI immediately
          currentTime.set(event.data.position);
          if (event.data.position === 0) {
            // repeat-one loop — reset isPlaying to true in case UI lost sync
            isPlaying.set(true);
            updateMediaSessionPlaybackState("playing");
          }
        }

        // Sync isPlaying state — ignore false when duration is 0 (track still loading)
        if (state.is_playing !== get(isPlaying)) {
          if (
            state.is_playing === false &&
            state.duration === 0 &&
            state.position === 0
          ) {
            // Backend hasn't loaded track yet, don't trust this state
          } else {
            isPlaying.set(state.is_playing);
            updateMediaSessionPlaybackState(
              state.is_playing ? "playing" : "paused",
            );
          }
        }

        // Emit time update for plugins
        pluginEvents.emit("timeUpdate", {
          currentTime: state.position,
          duration: state.duration,
        });
      } else if (get(activeBackend) === "html5" && html5Audio) {
        const pos = html5Audio.currentTime;
        const dur = html5Audio.duration || 0;
        const uiDuration = get(duration);

        // Sync isPlaying state (HTML5 events should handle this, but poller is a good fallback)
        const playing = !html5Audio.paused && !html5Audio.ended;
        if (playing !== get(isPlaying)) {
          // Do not sync HTML5 state if activeBackend changed
          if (get(activeBackend) === "html5") {
            isPlaying.set(playing);
            updateMediaSessionPlaybackState(playing ? "playing" : "paused");
          }
        }

        // Only advance currentTime while the audio is actually playing.
        // If the track has ended (or stalled past duration) we MUST NOT keep
        // pushing the position forward — that's what caused the "counter
        // counts to infinity on the last track of an album" bug when the
        // `ended` event was missed or delayed.
        //
        // Clamp to the UI duration (from the database) for live streams and
        // formats where audio.duration is 0/Infinity — otherwise the counter
        // runs past the displayed end.
        if (playing) {
          currentTime.set(uiDuration > 0 ? Math.min(pos, uiDuration) : pos);
        } else if (get(isPlaying) && (html5Audio.ended || (dur > 0 && pos >= dur))) {
          // Audio finished but UI still thinks we're playing — snap to end.
          currentTime.set(dur > 0 ? dur : pos);
        }
        if (dur > 0 && !isNaN(dur)) {
          duration.set(dur);
        } else if (uiDuration > 0) {
          duration.set(uiDuration);
        }

        // Watchdog: same as the native branch — if the browser hasn't fired
        // `ended` but the position is stuck at/past duration, force the end.
        // Fall back to the UI duration for streams/odd formats where
        // audio.duration is 0.
        const effectiveDuration = dur > 0 ? dur : uiDuration;
        if (playing && effectiveDuration > 0 && pos >= effectiveDuration - 0.05) {
          if (pendingForcedEnd === null) {
            pendingForcedEnd = setTimeout(() => {
              pendingForcedEnd = null;
              if (observation !== observationGeneration || session !== currentSessionId || get(activeBackend) !== "html5" || !html5Audio) return;
              const st = get(currentTime);
              const du = get(duration);
              if (du > 0 && st >= du - 0.1 && get(isPlaying)) {
                console.warn(
                  "[Player] Forced track end via watchdog (HTML5 missed `ended` event)",
                );
                handleTrackEnd(emitCompletion);
              }
            }, 400);
          }
        } else {
          clearPendingForcedEnd();
        }

        // Emit time update for plugins
        pluginEvents.emit("timeUpdate", {
          currentTime: pos,
          duration: dur,
        });
      } else if (get(activeBackend) === "remote") {
        // If remote, do NOT poll native audio. We rely purely on WebSocket pushes.
      } else if (get(activeBackend) === "squeeze") {
        // Squeeze store handles time updates via polling.
      }

      // Sync Media Session position if something is playing
      if (get(isPlaying)) {
        updateMediaSessionPosition();
      }

      // Broadcast state to WebSocket (throttled to ~2s)
      broadcastState();
    } catch (e) {
      console.error("[Player] Poller error:", e);
    }
  }, POLL_INTERVAL_MS);
}

let lastBroadcast = 0;
function broadcastState(force = false) {
  // CRITICAL: Do not broadcast if this device is not the owner of the playback.
  // This prevents infinite state "echo" loops across devices.
  if (get(activeBackend) === "remote" || get(activeBackend) === "squeeze")
    return;

  const now = Date.now();
  if (!force && now - lastBroadcast < 2000) return;

  const track = get(currentTrack);
  const playing = get(isPlaying);
  const pos = get(currentTime);
  const dur = get(duration);

  if (track || lastBroadcast === 0) {
    wsStore.send("player_state", {
      track: track
        ? {
            id: track.id,
            title: track.title,
            artist: track.artist,
            album: track.album,
            coverUrl: getTrackCoverSrc(track),
          }
        : null,
      isPlaying: playing,
      currentTime: pos,
      duration: dur,
      volume: get(volume),
      shuffle: get(shuffle),
      repeat: get(repeat),
    });
    lastBroadcast = now;
  }
}

function stopStatePoller(): void {
  if (nativeStatePoller) {
    clearInterval(nativeStatePoller);
    nativeStatePoller = null;
  }
}

// cleanup function for app unmount or hot reload
export function cleanupPlayer(): void {
  currentSessionId++;
  html5Origin = undefined;
  console.log("[Player] Cleaning up player resources");
  audioInitialized = false;
  runtimeSubscriptions.splice(0).forEach(unsubscribe => unsubscribe());
  clearPendingForcedEnd();
  if (_eqApplyTimer) clearTimeout(_eqApplyTimer);
  _eqApplyTimer = null;
  if (squeezeVolumeTimer) clearTimeout(squeezeVolumeTimer);
  squeezeVolumeTimer = null;
  squeezeVolumePending = null;
  Object.values(remoteThrottleTimers).forEach(clearTimeout);
  remoteThrottleTimers = {};
  windowsThumbarInitialized = false;
  stopStatePoller();
  nativeAudioStop().catch(console.error);

  if (dashPlayer) {
    try {
      dashPlayer.destroy();
    } catch (_) {}
    dashPlayer = null;
  }

  // Cleanup HTML5
  if (html5Audio) {
    html5Audio.pause();
    html5Audio.src = "";
  }

  // Cleanup HTML5 WebAudio EQ graph
  cleanupHtml5EqGraph();

  // Reset stores
  activeBackend.set("none");
  isPlaying.set(false);
  currentTrack.set(null);
  currentTime.set(0);
  duration.set(0);

  // Clear Media Session
  updateMediaSessionPlaybackState("none");
  if ("mediaSession" in navigator) {
    try {
      navigator.mediaSession.metadata = null;
    } catch (_) {
      /* ignore */
    }
  }
}

export function shutdownPlayer(): void {
  cleanupPlayer();
}

// ── Media Session API (Now Playing notification / lock screen controls) ──
// Works in Android WebView, desktop browsers, and any environment that supports
// the Web MediaSession API. Provides notification shade artwork, lock screen
// controls, and hardware media button support.

let mediaSessionInitialized = false;

let windowsThumbarInitialized = false;

async function initWindowsThumbarIntegration(): Promise<void> {
  if (windowsThumbarInitialized) return;

  try {
    const initialized = await initWindowsThumbar();
    if (!initialized) return;

    const unlistenThumbar = await listen<{ action?: string }>(
      "windows://thumbar-action",
      ({ payload }) => {
        const action = payload?.action;
        if (!action) return;

        switch (action) {
          case "previous":
            emitDesktopCommand({ type: "previous" }, previousTrack);
            break;
          case "toggle_play_pause":
            emitDesktopCommand({ type: get(isPlaying) ? "pause" : "resume" }, togglePlay);
            break;
          case "next":
            emitDesktopCommand({ type: "next" }, nextTrack);
            break;
        }
      },
    );

    runtimeSubscriptions.push(unlistenThumbar);
    windowsThumbarInitialized = true;
    await updateWindowsThumbarState(get(isPlaying));
    console.log("[Player] Windows taskbar thumbar initialized");
  } catch (err) {
    console.warn("[Player] Windows thumbar init failed:", err);
  }
}

function initMediaSessionHandlers(): void {
  if (mediaSessionInitialized || !("mediaSession" in navigator)) return;

  const ms = navigator.mediaSession;

  const setHandler = (
    action: MediaSessionAction,
    handler: MediaSessionActionHandler | null,
  ) => {
    try {
      ms.setActionHandler(action, handler);
    } catch (err) {
      // Some environments/WebViews don't support every action.
      // Keep registering the rest instead of aborting initialization.
      console.debug(`[MediaSession] Action not supported: ${action}`, err);
    }
  };

  // IMPORTANT: use explicit pause/resume handlers (not toggle).
  // Some Bluetooth headsets can emit repeated pause events; toggle would
  // accidentally resume playback and make pause appear broken.
  setHandler("play", () => {
    emitDesktopCommand({ type: "resume" }, resume);
  });
  setHandler("pause", () => {
    emitDesktopCommand({ type: "pause" }, pause);
  });
  setHandler("stop", () => {
    emitDesktopCommand({ type: "pause" }, pause);
  });
  setHandler("previoustrack", () => {
    emitDesktopCommand({ type: "previous" }, previousTrack);
  });
  setHandler("nexttrack", () => {
    emitDesktopCommand({ type: "next" }, nextTrack);
  });
  setHandler("seekto", (details) => {
    if (details.seekTime != null) {
      const dur = get(duration);
      if (dur > 0) {
        emitDesktopCommand({ type: "seek", seconds: details.seekTime }, () => seek(details.seekTime! / dur));
      }
    }
  });
  setHandler("seekbackward", (details) => {
    const offset = details.seekOffset || 10;
    const cur = get(currentTime);
    const dur = get(duration);
    if (dur > 0) {
      emitDesktopCommand({ type: "seek", seconds: Math.max(0, cur - offset) }, () => seek(Math.max(0, cur - offset) / dur));
    }
  });
  setHandler("seekforward", (details) => {
    const offset = details.seekOffset || 10;
    const cur = get(currentTime);
    const dur = get(duration);
    if (dur > 0) {
      emitDesktopCommand({ type: "seek", seconds: Math.min(dur, cur + offset) }, () => seek(Math.min(dur, cur + offset) / dur));
    }
  });

  mediaSessionInitialized = true;
  console.log("[Player] MediaSession action handlers registered");
}

async function updateMediaSessionMetadata(track: Track): Promise<void> {
  if (!("mediaSession" in navigator)) return;

  // Initialize handlers on first use (needs user gesture context)
  initMediaSessionHandlers();

  console.log("[MediaSession] Updating metadata for:", track.title);

  // Resolve artwork URL
  const artworkSources: MediaImage[] = [];

  // Specifically for MediaSession, we want to avoid asset:// URLs if possible
  // because Android's system notification usually can't resolve them.
  // AND we want to avoid large Base64 strings to avoid Binder limit crashes.
  let artUrl: string | null = null;

  if (track.track_cover && track.track_cover.startsWith("data:")) {
    try {
      console.log("[MediaSession] Saving Base64 artwork to temp file...");
      const tempPath = await invoke<string>("save_notification_image", {
        dataUri: track.track_cover,
      });
      artUrl = convertFileSrc(tempPath);
      console.log("[MediaSession] Artwork saved to:", artUrl);
    } catch (e) {
      console.error("[MediaSession] Failed to save notification image:", e);
      // Fallback to Base64 if saving fails (might still crash if too big)
      artUrl = track.track_cover;
    }
  } else {
    artUrl = getTrackCoverSrc(track);
  }

  // Also try album cover as fallback if still no art
  if (!artUrl && track.album_id) {
    artUrl = getAlbumCoverFromTracks(track.album_id);
  }

  if (artUrl) {
    console.log(
      "[MediaSession] Setting artwork src:",
      artUrl.substring(0, 50) + "...",
    );
    artworkSources.push({ src: artUrl, sizes: "512x512", type: "image/jpeg" });
  }

  try {
    navigator.mediaSession.metadata = new MediaMetadata({
      title: track.title || "Unknown Title",
      artist: track.artist || "Unknown Artist",
      album: track.album || "",
      artwork: artworkSources,
    });
    console.log("[MediaSession] Metadata set successfully");
  } catch (err) {
    console.warn("[Player] Failed to set MediaSession metadata:", err);
  }
}

function updateMediaSessionPlaybackState(
  state: "playing" | "paused" | "none",
): void {
  if (!("mediaSession" in navigator)) return;
  try {
    navigator.mediaSession.playbackState = state;
  } catch (err) {
    // Ignore — some environments don't support playbackState setter
  }
}

function updateMediaSessionPosition(): void {
  if (!("mediaSession" in navigator)) return;

  let dur = get(duration);
  let pos = get(currentTime);
  let rate = 1;

  // Basic validity check
  if (!dur || !isFinite(dur) || isNaN(dur)) {
    // Only log if it's 0 after playback started (might be intentional for a moment)
    return;
  }

  try {
    // Ensure position is within bounds [0, duration]
    const safePos = Math.max(0, Math.min(pos, dur));

    navigator.mediaSession.setPositionState({
      duration: dur,
      playbackRate: rate,
      position: safePos,
    });
  } catch (err) {
    console.error("[MediaSession] setPositionState failed:", err);
  }
}

export interface SqueezePlayCommit {
  track: Track;
  startTime: number;
  sessionId: number;
  isCurrentSession: () => boolean;
  commit: () => void;
}

/**
 * Issue a Squeeze play request and commit confirmed state only on success.
 * The callback keeps this seam independent from the rest of the audio setup,
 * while the session predicate prevents an older successful request winning.
 */
export async function playTrackOnSqueeze(
  mac: string,
  trackIds: number[],
  startIndex: number,
  commit: SqueezePlayCommit,
): Promise<"played" | "failed" | "superseded"> {
  try {
    await squeezePlay(mac, trackIds, startIndex);
  } catch (err) {
    console.error("[Player] Squeeze play failed:", err);
    addToast(
      `Squeeze playback failed: ${err instanceof Error ? err.message : "Unknown error"}`,
      "error",
    );
    return "failed";
  }

  if (!commit.isCurrentSession()) return "superseded";
  commit.commit();
  return "played";
}

// Play a specific track
export async function playTrack(
  track: Track,
  skipLocalSrc = false,
  startTime = 0,
  queuePlan?: { tracks: Track[]; index: number; commit(): void },
): Promise<void> {
  const previousTrackObj = get(currentTrack);
  const sessionId = ++currentSessionId;

  // Reset playStartTime — play counting only happens on natural track completion
  // (handleTrackEnd / handleGaplessAdvance), not on manual skip/play.
  const partialEffects: string[] = [];
  clearPendingForcedEnd();

  // ListenBrainz: notify 'playing_now'
  if (get(appSettings).listenBrainzEnabled) {
    submitListenbrainzListen(
      track.artist ?? "Unknown Artist",
      track.title ?? "Unknown",
      track.album,
      track.duration,
      true,
    ).catch((e) => console.warn("[ListenBrainz] Now-playing failed:", e));
  }

  // Get full track with base64 data URI for plugins
  const fullTrack = await getFullTrack(track.id, true);

  // Check session ID before proceeding after await
  if (sessionId !== currentSessionId) throw new PlaybackFailure({ code: "revision_conflict", message: "Playback session changed", retryable: false }, "superseded");

  const trackForPlugins = fullTrack || track;
  const confirmTrack = () => {
    playStartTime = Date.now();
    pluginEvents.emit("trackChange", { track: trackForPlugins, previousTrack: previousTrackObj });
  };

  // Update Media Session early so the UI reflects the change immediately
  // even if the audio engine takes a moment to initialize or resolve streams.
  console.log(
    "[Player] Preparing MediaSession metadata for:",
    trackForPlugins.title,
  );
  await updateMediaSessionMetadata(trackForPlugins);

  // AUTO-FETCH COVER LOGIC
  // If the track is missing a cover, attempt to fetch it from an external source.
  // This runs asynchronously and does not block playback.
  if (!track.track_cover_path && !track.cover_url) {
    fetchTrackCover(track)
      .then(async (newCoverUrl) => {
        if (newCoverUrl) {
          console.log(
            `[Player] Auto-fetched cover for "${track.title}": ${newCoverUrl}`,
          );

          // 1. Persist to Backend Database
          try {
            await invoke("update_track_cover_url", {
              trackId: track.id,
              coverUrl: newCoverUrl,
            });
          } catch (e) {
            console.error(
              "[Player] Failed to persist fetched cover to database:",
              e,
            );
          }

          // 2. Update reactive library store (metadata, cache, and main list)
          updateTrackCover(track.id, newCoverUrl);

          // 3. Update current player state if still playing the same track
          const current = get(currentTrack);
          if (current && current.id === track.id) {
            currentTrack.update((t) =>
              t ? { ...t, cover_url: newCoverUrl } : t,
            );

            // 4. Update Media Session (system notification) immediately with new art
            updateMediaSessionMetadata({
              ...track,
              cover_url: newCoverUrl,
            }).catch(() => {});
          }
        }
      })
      .catch((err) => {
        console.error("[Player] Failed to auto-fetch cover:", err);
      });
  }

  if (sessionId !== currentSessionId) {
    console.log(
      "[Player] Session changed during metadata update, aborting playback",
    );
    throw new PlaybackFailure({ code: "revision_conflict", message: "Playback session changed", retryable: false }, "superseded");
  }

  if (get(activeBackend) === "squeeze") {
    const mac = get(activeSqueezePlayer);
    if (mac) {
      const q = queuePlan?.tracks ?? get(queue);
      const idx = queuePlan?.index ?? get(queueIndex);
      const trackIds = q.length > 0 ? q.map((t) => t.id) : [track.id];
      const startIdx = q.length > 0 ? idx : 0;
      const ownsTarget = captureSqueezeTargetOwnership(mac);
      const result = await playTrackOnSqueeze(mac, trackIds, startIdx, {
        track: trackForPlugins,
        startTime,
        sessionId,
        isCurrentSession: () => sessionId === currentSessionId && ownsTarget(),
        commit: () => {
          confirmTrack();
          queuePlan?.commit();
          currentTrack.set(trackForPlugins);
          currentTime.set(startTime);
          duration.set(track.duration || 0);
          isPlaying.set(true);
        },
      });
      if (result !== "played") throw new PlaybackFailure({ code: result === "failed" ? "execution_failed" : "revision_conflict", message: `Squeeze playback ${result}`, retryable: false }, result, result === "superseded" ? ["Previous Squeeze target acknowledged playback"] : []);
    } else {
      throw new PlaybackFailure({ code: "output_unavailable", message: "No Squeeze target selected", retryable: false });
    }
    return;
  }

  try {
    let audioPath = track.local_src || track.path;

    // Fallback for plugins using stream_url
    if (!audioPath && (track as any).stream_url) {
      audioPath = (track as any).stream_url;
    }

    // Fallback for plugins using external_id as URL (common in radio plugins)
    if (
      !audioPath &&
      track.external_id &&
      (track.external_id.startsWith("http://") ||
        track.external_id.startsWith("https://"))
    ) {
      audioPath = track.external_id;
    }

    const streaming = isStreaming(track) || !!(track as any).stream_url;

    // Prep the backends
    if (streaming) {
      // Ensure we have a valid path
      if (!audioPath) {
        throw new Error("No audio path or stream URL found for track");
      }

      // Stop native audio
      await nativeAudioStop();
      partialEffects.push("Previous PC playback stopped");

      // Resolve custom schemes (like tidal://) to HTTP or blob URLs
      if (classifyAudioPath(audioPath) === "custom-scheme") {
        const runtime = pluginStore.getRuntime();
        if (runtime) {
          const sourceType = track.source_type;
          const externalId = track.external_id;
          if (sourceType && externalId) {
            console.log(`[Player] Resolving custom scheme: ${audioPath}`);
            const resolved = await runtime.resolveStreamUrl(
              sourceType,
              externalId,
              { track: trackForPlugins },
            );
            if (resolved) {
              audioPath = resolved;
            } else {
              throw new Error(`Failed to resolve stream URL for ${sourceType}`);
            }
          }
        }
      }

      // Start HTML5
      let audio = recreateHtml5AudioElement();

      // Reset src to avoid overlap issues
      audio.pause();

      // Destroy any existing dash player before switching tracks
      if (dashPlayer) {
        try {
          dashPlayer.destroy();
        } catch (_) {}
        dashPlayer = null;
      }

      const finalKind = classifyAudioPath(audioPath);

      if (finalKind === "blob") {
        audio = await prepareHtml5AudioForPath(audio, audioPath);
        audio.volume = sliderToAudioVolume(get(volume));

        await playWithDash(audioPath, audio);
        partialEffects.push("Playback started");
        activeBackend.set("html5");
        console.log("[Player] dash.js DASH streaming started:", track.title);
      } else {
        // Resolve playlist-format URLs (.m3u, .pls, .m3u8) to direct stream URLs
        audioPath = await resolvePlaylistUrl(audioPath);
        audio = await prepareHtml5AudioForPath(audio, audioPath);

        audio.src = audioPath;
        audio.volume = sliderToAudioVolume(get(volume));

        // Wrap play in a handler to catch AbortError (common with rapid skipping)
        try {
          await audio.play();
          partialEffects.push("Playback started");
          activeBackend.set("html5");
        } catch (err) {
          if (err instanceof DOMException && err.name === "AbortError") {
            console.warn(
              "[Player] Playback aborted (likely replaced by new track)",
              err,
            );
            throw new PlaybackFailure({ code: "revision_conflict", message: "Playback aborted", retryable: false }, "superseded");
          } else {
            throw err;
          }
        }

        if (startTime > 0) {
          audio.currentTime = startTime;
        }

        console.log("[Player] HTML5 streaming started:", track.title);
      }

      activeBackend.set("html5");
    } else {
      if (!audioPath) {
        throw new Error("No local audio path found for track");
      }

      if (nativeAudioUsed) {
        // Stop HTML5 audio
        if (html5Audio) {
          html5Audio.pause();
          html5Audio.src = "";
        }

        // Play via native backend
        await nativeAudioPlay(audioPath, (track as any).replay_gain_db ?? null);
        partialEffects.push("Playback started");
        activeBackend.set("native");

        // Sync volume
        const vol = sliderToAudioVolume(get(volume));
        await nativeAudioSetVolume(vol);

        // Seek if starting from a specific position (for playback transfer)
        if (startTime > 0 && track.duration) {
          await nativeAudioSeek(startTime / track.duration);
        }

        // Preload next track for gapless playback
        _schedulePreload();

        activeBackend.set("native");
        console.log("[Player] Native playback started:", track.title);
      } else {
        // Fallback to HTML5 via convertFileSrc if native is disabled (e.g. on macOS)
        let audio = recreateHtml5AudioElement();
        audio = await prepareHtml5AudioForPath(audio, audioPath);
        audio.pause();

        // Use convertFileSrc to get a URL that the browser can play
        audio.src = convertFileSrc(audioPath);
        audio.volume = sliderToAudioVolume(get(volume));

        await audio.play();
        partialEffects.push("Playback started");
        activeBackend.set("html5");
        if (startTime > 0) {
          audio.currentTime = startTime;
        }
        activeBackend.set("html5");
        console.log("[Player] Local playback started via HTML5:", track.title);
      }
    }

    confirmTrack();
    queuePlan?.commit();
    currentTrack.set(trackForPlugins);
    currentTime.set(startTime);
    duration.set(track.duration || 0);
    isPlaying.set(true);

    // Update Media Session state and position (metadata was updated earlier)
    updateMediaSessionPlaybackState("playing");
    updateMediaSessionPosition();
  } catch (err) {
    console.error("[Player] Playback failed:", err);
    addToast(
      `Playback failed: ${err instanceof Error ? err.message : "Unknown error"}`,
      "error",
    );
    if (err instanceof PlaybackFailure) throw new PlaybackFailure(err.controlError, err.status, [...partialEffects, ...err.partialEffects]);
    throw new PlaybackFailure({ code: "execution_failed", message: err instanceof Error ? err.message : String(err), retryable: false }, "failed", partialEffects);
  }
}

// Shuffled Queue State

// Helper to shuffle array (Fisher-Yates)
function shuffleArray<T>(array: T[]): T[] {
  const arr = [...array];
  for (let i = arr.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [arr[i], arr[j]] = [arr[j], arr[i]];
  }
  return arr;
}

// Play a list of tracks starting at index
export async function playTracks(
  tracks: Track[], startIndex = 0, context?: PlaybackContext,
): Promise<void> {
  if (!tracks[startIndex]) throw new Error("Starting track is unavailable");
  const indices = get(shuffle) ? [startIndex, ...shuffleArray(tracks.map((_, i) => i).filter(i => i !== startIndex))] : get(shuffledIndices);
  await playTrack(tracks[startIndex], false, 0, {
    tracks, index: startIndex,
    commit() {
      queue.set(tracks);
      queueIndex.set(startIndex);
      userQueueCount.set(0);
      playbackContext.set(context ?? null);
      if (get(shuffle)) { shuffledIndices.set(indices); shuffledIndex.set(0); }
      pluginEvents.emit("queueChange", { queue: tracks, index: startIndex });
    },
  });
  _schedulePreload();
}

export async function togglePlay(): Promise<void> {
  if (get(isPlaying)) {
    await pause();
  } else {
    await resume();
  }
}

export async function pause(): Promise<void> {
  if (get(activeBackend) === "squeeze") {
    const mac = get(activeSqueezePlayer);
    if (!mac) throw new Error("No Squeeze target selected");
    await runOwnedSqueeze(mac, () => squeezePause(mac));
    isPlaying.set(false);
    return;
  }

  if (get(activeBackend) === "remote") {
    const targetId = get(activeRemoteDevice);
    if (targetId) {
      sendRemoteCommand(targetId, "pause");
    }
    return;
  }

  try {
    if (get(activeBackend) === "html5") {
      getHtml5Audio().pause();
    } else if (get(activeBackend) === "native") {
      await nativeAudioPause();
    }
    isPlaying.set(false);
    updateMediaSessionPlaybackState("paused");
  } catch (err) {
    console.error("[Player] Pause failed:", err);
    throw err;
  }
}

export async function resume(): Promise<void> {
  if (get(activeBackend) === "squeeze") {
    const mac = get(activeSqueezePlayer);
    if (!mac) throw new Error("No Squeeze target selected");

    const state = get(squeezePlayerState);
    if (state?.current_track) {
      await runOwnedSqueeze(mac, () => squeezeResume(mac));
    } else {
      const q = get(queue);
      const idx = get(queueIndex);
      const track = get(currentTrack);
      if (q.length > 0) {
        const trackIds = q.map((t) => t.id);
        await runOwnedSqueeze(mac, () => squeezePlay(mac, trackIds, idx));
      } else if (track) {
        await runOwnedSqueeze(mac, () => squeezePlay(mac, [track.id], 0));
      } else throw new Error("No track to resume");
    }
    isPlaying.set(true);
    return;
  }

  if (get(activeBackend) === "remote") {
    const targetId = get(activeRemoteDevice);
    if (targetId) {
      sendRemoteCommand(targetId, "resume");
    }
    return;
  }

  try {
    const track = get(currentTrack);
    if (!track) return;

    if (get(currentTime) >= get(duration) && get(duration) > 0) {
      await playTrack(track);
    } else if (get(activeBackend) === "none") {
      // App just opened, no audio loaded yet - start playback from saved position
      await playTrack(track, false, get(currentTime));
    } else if (get(activeBackend) === "html5") {
      await resumeHtml5AudioContext();
      await getHtml5Audio().play();
      isPlaying.set(true);
      updateMediaSessionPlaybackState("playing");
    } else if (get(activeBackend) === "native") {
      await nativeAudioResume();
      isPlaying.set(true);
      updateMediaSessionPlaybackState("playing");
    }
    updateMediaSessionPosition();
  } catch (err) {
    console.error("[Player] Resume failed:", err);
    throw err;
  }
}

// =============================================================================
// QUEUE INDEX HELPERS
// =============================================================================

/**
 * Advance queue index stores (shuffledIndex, userQueueCount) and return the
 * next absolute queue index. Returns null if playback should stop or hand off
 * to autoplay (caller must handle those cases).
 *
 * @param dry — if true, compute the next index without writing any stores.
 *              Used by _schedulePreload() to peek ahead.
 */
function _advanceQueueIndex(dry = false): number | null {
  const q = get(queue);
  const rep = get(repeat);
  const shuf = get(shuffle);
  const userCount = get(userQueueCount);
  const settings = get(appSettings);
  let idx = get(queueIndex);

  if (q.length === 0) return null;

  // Check if we have user-queued tracks to play first
  if (userCount > 0 && !shuf) {
    // Play next user-queued track sequentially (always sequential for user queue)
    // User queue tracks are inserted directly after current track in the main queue list.
    // So we just increment normal index.
    idx = idx + 1;
    if (!dry) userQueueCount.update((c) => Math.max(0, c - 1));
  } else if (shuf) {
    const shufIndices = get(shuffledIndices);
    let shufIdx = get(shuffledIndex) + 1;

    if (shufIdx >= shufIndices.length) {
      if (rep === "all") {
        shufIdx = 0;
      } else {
        // End of shuffle — caller decides autoplay/stop
        return null;
      }
    }

    if (!dry) {
      shuffledIndex.set(shufIdx);
      if (userCount > 0) userQueueCount.set(userCount - 1);
    }
    idx = shufIndices[shufIdx];
  } else {
    idx = idx + 1;

    if (idx >= q.length) {
      if (rep === "all") {
        idx = 0;
      } else {
        // End of queue — caller decides autoplay/stop
        return null;
      }
    }
  }

  return idx;
}

// Next track
export async function nextTrack(): Promise<void> {
  if (get(activeBackend) === "squeeze") {
    const mac = get(activeSqueezePlayer);
    if (mac) await navigateSqueeze(mac, () => squeezeNext(mac));
    return;
  }

  if (get(activeBackend) === "remote") {
    const targetId = get(activeRemoteDevice);
    if (targetId) {
      sendRemoteCommand(targetId, "next");
    }
    return;
  }

  const q = get(queue);
  const settings = get(appSettings);

  if (q.length === 0) {
    if (settings.autoplay) await playRandomFromLibrary();
    return;
  }

  const idx = _advanceQueueIndex(true);

  if (idx === null) {
    // End of queue — check if we should remove from listen-later
    const ctx = get(playbackContext);
    if (ctx?.type === "album" && ctx.albumId && isInListenLater(ctx.albumId)) {
      console.log(
        "[Player] Album finished, removing from listen-later:",
        ctx.albumId,
      );
      void toggleListenLater(ctx.albumId);
    }

    // End of queue/shuffle with no repeat
    if (settings.autoplay) {
      await playRandomFromLibrary();
    } else {
      // Stop playback completely
      if (get(activeBackend) === "native") {
        await nativeAudioStop();
      } else if (get(activeBackend) === "html5" && html5Audio) {
        html5Audio.pause();
        html5Audio.currentTime = 0;
      }
      isPlaying.set(false);
      currentTime.set(0);
      updateMediaSessionPlaybackState("paused");
    }
    return;
  }

  await playTrack(q[idx], false, 0, { tracks: q, index: idx, commit() { _advanceQueueIndex(); queueIndex.set(idx); } });
}

// Play a random track from the library (for autoplay feature)
async function playRandomFromLibrary(): Promise<void> {
  const allTracks = get(libraryTracks);
  if (allTracks.length === 0) {
    isPlaying.set(false);
    return;
  }

  // Pick a random track, avoiding the current one if possible
  const current = get(currentTrack);
  let availableTracks = allTracks;

  if (current && allTracks.length > 1) {
    availableTracks = allTracks.filter((t) => t.id !== current.id);
  }

  const randomIndex = Math.floor(Math.random() * availableTracks.length);
  const randomTrack = availableTracks[randomIndex];

  // Add to queue and play
  const newQueue = [...get(queue), randomTrack];
  const index = newQueue.length - 1;
  await playTrack(randomTrack, false, 0, { tracks: newQueue, index, commit() { queue.set(newQueue); queueIndex.set(index); } });
}

// Previous track
export async function previousTrack(): Promise<void> {
  if (get(activeBackend) === "squeeze") {
    const mac = get(activeSqueezePlayer);
    if (mac) await navigateSqueeze(mac, () => squeezePrevious(mac));
    return;
  }

  if (get(activeBackend) === "remote") {
    const targetId = get(activeRemoteDevice);
    if (targetId) {
      sendRemoteCommand(targetId, "previous");
    }
    return;
  }

  const q = get(queue);
  const shuf = get(shuffle);
  let idx = get(queueIndex);

  if (q.length === 0) return;

  // If more than 3 seconds in, restart current track
  try {
    let pos = get(currentTime);

    if (pos > 3) {
      if (get(activeBackend) === "html5") {
        getHtml5Audio().currentTime = 0;
      } else if (get(activeBackend) === "native") {
        await nativeAudioSeek(0);
      }
      return;
    }
  } catch (err) {
    console.error("[Player] Restart track failed:", err);
    throw err;
  }

  let nextShuffleIndex = get(shuffledIndex);
  if (shuf) {
    // Persistent Shuffle Previous
    const shufIndices = get(shuffledIndices);
    let shufIdx = get(shuffledIndex);

    shufIdx = shufIdx - 1;
    if (shufIdx < 0) {
      shufIdx = get(repeat) === "all" ? shufIndices.length - 1 : 0;
    }

    nextShuffleIndex = shufIdx;
    idx = shufIndices[shufIdx];
  } else {
    idx = idx - 1;
    if (idx < 0) {
      idx = get(repeat) === "all" ? q.length - 1 : 0;
    }
  }

  await playTrack(q[idx], false, 0, { tracks: q, index: idx, commit() { queueIndex.set(idx); if (shuf) shuffledIndex.set(nextShuffleIndex); } });
}

// Seek to position (0-1)
export async function seek(position: number): Promise<void> {
  if (get(activeBackend) === "squeeze") {
    const mac = get(activeSqueezePlayer);
    if (mac) {
      const posSeconds = position * get(duration);
      await navigateSqueeze(mac, () => squeezeSeek(mac, posSeconds));
    }
    return;
  }

  if (get(activeBackend) === "remote") {
    const targetId = get(activeRemoteDevice);
    if (targetId) {
      throttledRemoteCommand(targetId, "seek", { position }, 100);
    }
    return;
  }

  try {
    if (get(activeBackend) === "html5") {
      const audio = getHtml5Audio();
      if (audio.duration) {
        audio.currentTime = position * audio.duration;
      }
    } else if (get(activeBackend) === "native") {
      await nativeAudioSeek(position);
      // Update UI immediately — poller is stopped while paused
      if (!get(isPlaying)) {
        currentTime.set(position * get(duration));
      }
    }
    updateMediaSessionPosition();
  } catch (err) {
    console.error("[Player] Seek failed:", err);
    throw err;
  }
}

// Set volume (slider value 0-1, will be converted to logarithmic for audio)
export async function setVolume(sliderValue: number): Promise<void> {
  if (get(activeBackend) === "squeeze") {
    const mac = get(activeSqueezePlayer);
    if (mac) {
      await runOwnedSqueeze(mac, () => squeezeSetVolume(mac, Math.round(sliderValue * 100)));
      volume.set(sliderValue);
      setSqueezeVolumeCooldown();
    }
    return;
  }

  if (get(activeBackend) === "remote") {
    const targetId = get(activeRemoteDevice);
    if (targetId) {
      throttledRemoteCommand(targetId, "volume", { volume: sliderValue }, 100);
    }
    return;
  }

  const vol = sliderToAudioVolume(sliderValue);

  try {
    // Update HTML5 volume
    if (html5Audio) {
      html5Audio.volume = vol;
    }
    // Update native volume
    if (nativeAudioUsed) {
      await nativeAudioSetVolume(vol);
    }
    volume.set(sliderValue);
  } catch (err) {
    console.error("[Player] Volume set failed:", err);
    throw err;
  }
}

// Toggle shuffle
export async function toggleShuffle(): Promise<void> {
  if (get(activeBackend) === "squeeze") {
    const mac = get(activeSqueezePlayer);
    if (mac) { await runOwnedSqueeze(mac, () => squeezeSetShuffle(mac, !get(shuffle))); shuffle.set(!get(shuffle)); }
    return;
  }

  if (get(activeBackend) === "remote") {
    const targetId = get(activeRemoteDevice);
    if (targetId) {
      sendRemoteCommand(targetId, "shuffle", { shuffle: !get(shuffle) });
    }
    return;
  }

  shuffle.update((s) => {
    const newState = !s;

    if (newState) {
      // Shuffle disables album-end mode (exploration R2).
      // Shuffling would immediately change the next track's album,
      // making album-end detection unreliable.
      if (isTimerModeAlbumEnd()) {
        stopSleepTimer(true);
        addToast("Album-end mode disabled during shuffle", "info");
      }

      // Turn ON: Generate shuffled order
      const q = get(queue);
      const currentIdx = get(queueIndex);

      // Create indices array
      const indices = q.map((_, i) => i);
      const shuffled = shuffleArray(indices);

      // Set shuffled indices
      console.log("Regenerating shuffle in toggleShuffle");
      shuffledIndices.set(shuffled);

      // Find current track in shuffled list to maintain continuity
      const ptr = shuffled.indexOf(currentIdx);
      shuffledIndex.set(ptr !== -1 ? ptr : 0);
    } else {
      // Turn OFF: Just stop using shuffle
      // QueueIndex is already correct
    }

    return newState;
  });
}

// Cycle repeat mode
export async function setRepeatMode(mode: "none" | "all" | "one"): Promise<void> {
  if (get(activeBackend) === "squeeze") {
    const mac = get(activeSqueezePlayer);
    if (!mac) throw new Error("No Squeeze target selected");
    await runOwnedSqueeze(mac, () => squeezeSetRepeat(mac, mode === "none" ? "off" : mode));
  } else if (get(activeBackend) === "remote") {
    const target = get(activeRemoteDevice);
    if (target) sendRemoteCommand(target, "repeat", { repeat: mode });
    return;
  } else if (get(activeBackend) === "native") await nativeAudioSetRepeatOne(mode === "one");
  repeat.set(mode);
}
export async function cycleRepeat(): Promise<void> {
  const mode = get(repeat);
  await setRepeatMode(mode === "none" ? "all" : mode === "all" ? "one" : "none");
}

// Handle track end
export async function applyTrackEnd(): Promise<void> {
  // Record play for the track that just ended
  const track = get(currentTrack);
  if (track && playStartTime > 0) {
    const durationPlayed = Math.floor((Date.now() - playStartTime) / 1000);
    if (durationPlayed > 5) {
      console.log(
        `[Player] Recording play for "${track.title}" (${durationPlayed}s)`,
      );
      recordTrackPlay(track.id, track.album_id ?? null, durationPlayed);
      incrementPlayCount(track.id);
      // ListenBrainz: scrobble if >= 50 % of duration or 4 minutes
      const trackDuration = track.duration ?? 0;
      if (get(appSettings).listenBrainzEnabled && trackDuration > 0) {
        const threshold = Math.min(Math.floor(trackDuration / 2), 240);
        if (durationPlayed >= threshold) {
          submitListenbrainzListen(
            track.artist ?? "Unknown Artist",
            track.title ?? "Unknown",
            track.album,
            track.duration,
            false,
          ).catch((e) => console.warn("[ListenBrainz] Scrobble failed:", e));
        }
      }
    } else {
      console.log(
        `[Player] Track "${track.title}" played only ${durationPlayed}s, not recording`,
      );
    }
    playStartTime = 0;
  }

  // Sleep timer check — MUST precede repeat-one (AD3).
  // If the timer is in track_end/album_end mode, it may pause playback
  // and we must return early without advancing the queue.
  if (isTimerModeTrackOrAlbumEnd()) {
    // Dry-run to peek at the next track's album_id for album_end detection
    let nextAlbumId: number | null = null;
    const nextIdx = _advanceQueueIndex(true);
    if (nextIdx !== null) {
      const q = get(queue);
      const nextTrack = q[nextIdx];
      if (nextTrack) {
        nextAlbumId = nextTrack.album_id ?? null;
      }
    }
    // nextAlbumId=null means: no next track (queue end) or next track has no album
    if (handleSleepTimerCheck(track, nextAlbumId, false)) {
      // Timer fired — await the pause before confirming this completion.
      await pause();
      return;
    }
  }

  // Repeat one logic for backends that don't handle it internally (like HTML5)
  if (get(repeat) === "one" && track) {
    console.log("[Player] Repeat one: restarting current track");
    await playTrack(track);
    return;
  }

  await nextTrack();
}

// Handle gapless advance — audio backend already playing the next track.
export async function applyGaplessAdvance(): Promise<void> {
  const q = get(queue);

  // Record play for the track that just ended
  const prevTrack = get(currentTrack);
  if (prevTrack && playStartTime > 0) {
    const durationPlayed = Math.floor((Date.now() - playStartTime) / 1000);
    if (durationPlayed > 5) {
      console.log(
        `[Player] Gapless: recording play for "${prevTrack.title}" (${durationPlayed}s)`,
      );
      recordTrackPlay(prevTrack.id, prevTrack.album_id ?? null, durationPlayed);
      incrementPlayCount(prevTrack.id);
      const trackDuration = prevTrack.duration ?? 0;
      if (get(appSettings).listenBrainzEnabled && trackDuration > 0) {
        const threshold = Math.min(Math.floor(trackDuration / 2), 240);
        if (durationPlayed >= threshold) {
          submitListenbrainzListen(
            prevTrack.artist ?? "Unknown Artist",
            prevTrack.title ?? "Unknown",
            prevTrack.album,
            prevTrack.duration,
            false,
          ).catch((e) => console.warn("[ListenBrainz] Scrobble failed:", e));
        }
      }
    } else {
      console.log(
        `[Player] Gapless: track "${prevTrack.title}" played only ${durationPlayed}s, not recording`,
      );
    }
  }
  playStartTime = Date.now();

  // Sleep timer check — same as handleTrackEnd but for gapless transitions.
  // The backend has already started playing the next track. If the timer
  // fires, pause() stops the just-started next track.
  if (isTimerModeTrackOrAlbumEnd()) {
    let nextAlbumId: number | null = null;
    const nextIdx = _advanceQueueIndex(true);
    if (nextIdx !== null) {
      const q2 = get(queue);
      const nextTrack = q2[nextIdx];
      if (nextTrack) {
        nextAlbumId = nextTrack.album_id ?? null;
      }
    }
    if (handleSleepTimerCheck(prevTrack, nextAlbumId, false)) {
      await pause();
      return;
    }
  }

  const idx = _advanceQueueIndex(true);
  if (idx === null) {
    // End of queue after gapless advance — play was already recorded above.
    // Just handle queue end (listen-later removal, autoplay, stop).
    playStartTime = 0;
    const ctx = get(playbackContext);
    if (ctx?.type === "album" && ctx.albumId && isInListenLater(ctx.albumId)) {
      console.log(
        "[Player] Album finished (gapless), removing from listen-later:",
        ctx.albumId,
      );
      void toggleListenLater(ctx.albumId);
    }
    const settings = get(appSettings);
    if (settings.autoplay) {
      await playRandomFromLibrary();
    } else {
      // Stop playback completely
      if (get(activeBackend) === "native") {
        await nativeAudioStop();
      } else if (get(activeBackend) === "html5" && html5Audio) {
        html5Audio.pause();
        html5Audio.currentTime = 0;
      }
      isPlaying.set(false);
      currentTime.set(0);
      updateMediaSessionPlaybackState("paused");
    }
    return;
  }

  const commitAdvance = () => { _advanceQueueIndex(); queueIndex.set(idx); };
  const nextTrackObj = q[idx];
  if (!nextTrackObj) return;

  // Check if the backend is playing the correct track.
  // If the queue was modified (add/reorder/remove) after the preload,
  // the backend may be playing the wrong track. In that case, force-play the correct one.
  const expectedPath = nextTrackObj.local_src || nextTrackObj.path;
  if (expectedPath && lastPreloadedPath && expectedPath !== lastPreloadedPath) {
    console.log(
      "[Player] Gapless mismatch: expected",
      expectedPath,
      "but preloaded",
      lastPreloadedPath,
    );
    lastPreloadedPath = null;
    await playTrack(nextTrackObj, false, 0, { tracks: q, index: idx, commit: commitAdvance });
    return;
  }

  await _advanceUiToTrack(nextTrackObj, commitAdvance);
}

// Update all UI state for a track without touching the audio backend.
async function _advanceUiToTrack(track: Track, commitAdvance: () => void): Promise<void> {
  const previousTrackObj = get(currentTrack);

  // Full track for plugins / cover art
  const fullTrack = await getFullTrack(track.id, true);
  const trackForPlugins = fullTrack || track;

  commitAdvance();
  currentTrack.set(trackForPlugins);
  currentTime.set(0);
  duration.set(track.duration || 0);
  isPlaying.set(true);

  pluginEvents.emit("trackChange", {
    track: trackForPlugins,
    previousTrack: previousTrackObj,
  });
  pluginEvents.emit("queueChange", {
    queue: get(queue),
    index: get(queueIndex),
  });

  await updateMediaSessionMetadata(trackForPlugins);
  updateMediaSessionPlaybackState("playing");
  updateMediaSessionPosition();

  // Schedule preload of the NEXT-next track to keep gapless chain alive
  _schedulePreload();

  // ListenBrainz now-playing
  if (get(appSettings).listenBrainzEnabled) {
    submitListenbrainzListen(
      track.artist ?? "Unknown Artist",
      track.title ?? "Unknown",
      track.album,
      track.duration,
      true,
    ).catch((e) => console.warn("[ListenBrainz] Now-playing failed:", e));
  }
}

// =============================================================================
// GAPLESS PRELOAD
// =============================================================================
// After playTrack() starts a local file on the native backend, we immediately
// tell the backend to open and buffer the NEXT track in the queue.
// The backend appends it to its internal rodio queue so the transition is
// seamless — no gap between tracks.
//
// We call this every time a new track starts. The backend ignores duplicate
// preloads for the same path.
// =============================================================================

function _schedulePreload(): void {
  if (get(activeBackend) !== "native") return;

  const q = get(queue);
  const nextIdx = _advanceQueueIndex(true); // dry run — no store writes

  if (nextIdx === null || nextIdx >= q.length) {
    lastPreloadedPath = null;
    return;
  }

  const nextTrackObj = q[nextIdx];
  if (!nextTrackObj || isStreaming(nextTrackObj)) {
    lastPreloadedPath = null;
    return;
  }

  const nextPath = nextTrackObj.local_src || nextTrackObj.path;
  if (!nextPath) {
    lastPreloadedPath = null;
    return;
  }

  lastPreloadedPath = nextPath;
  nativeAudioPreload(
    nextPath,
    (nextTrackObj as any).replay_gain_db ?? null,
  ).catch((e) => {
    console.warn("[Player] Preload failed (non-fatal):", e);
    lastPreloadedPath = null;
  });
}

// Progress as percentage (0-1)


// Queue management functions

// Add tracks to queue (Spotify-like: after current track + previously user-added tracks)
export async function addToQueue(tracks: Track[]): Promise<void> {
  const plan = readQueuePlan();
  const currentIdx = plan.queueIndex;
  const userCount = plan.userQueueCount;
  // Insert position: after current track + user-added tracks
  const insertPosition = currentIdx + 1 + userCount;
  const addedCount = tracks.length;

  plan.queue = ((q) => {
    const newQueue = [...q];
    newQueue.splice(insertPosition, 0, ...tracks);

    // Emit queueChange event for plugins

    return newQueue;
  })(plan.queue);

  // Update user queue count
  plan.userQueueCount += addedCount;

  // Sync queue change to the active backend


  // Update shuffled indices to reflect the shift in queue
  if (get(shuffle)) {
    console.log("Updating shuffle in addToQueue");
    plan.shuffledIndices = ((indices) => {
      // 1. Shift existing indices that are after insertion point
      const shifted = indices.map((i) =>
        i >= insertPosition ? i + addedCount : i,
      );

      // Immediate entries belong in the authoritative order, not a second
      // advancement lane that later replays them from the shuffled tail.
      const newIndices = Array.from({ length: addedCount }, (_, i) => insertPosition + i);
      const insertAt = plan.shuffledIndex + 1 + userCount;
      return [...shifted.slice(0, insertAt), ...newIndices, ...shifted.slice(insertAt)];
    })(plan.shuffledIndices);
  }

  if (get(activeBackend) === "squeeze") {
    const mac = get(activeSqueezePlayer);
    if (mac) {
      await runOwnedSqueeze(mac, () => squeezeInsertQueue(
        mac,
        tracks.map((t) => t.id),
        insertPosition,
      ));
    }
  }
  commitQueuePlan(plan);
}


// Append tracks to the END of the queue (after all existing tracks).
// Unlike `addToQueue` (which inserts "up next"), this preserves whatever
// is already playing and whatever is queued — the new tracks play last.
export async function appendToQueueEnd(tracks: Track[]): Promise<void> {
  const plan = readQueuePlan();
  if (tracks.length === 0) return;
  const addedCount = tracks.length;
  const start = plan.queue.length;
  plan.queue = [...plan.queue, ...tracks];
  // Tail entries never enlarge the immediate user-queue prefix.

  // Shuffle: append the new indices at the tail of the shuffled list
  // so they play after everything else.
  if (get(shuffle)) {
    plan.shuffledIndices = ((indices) => {
      const newIndices = Array.from(
        { length: addedCount },
        (_, i) => start + i,
      );
      return [...indices, ...newIndices];
    })(plan.shuffledIndices);
  }


  if (get(activeBackend) === "squeeze") {
    const mac = get(activeSqueezePlayer);
    if (mac) {
      await runOwnedSqueeze(mac, () => squeezeInsertQueue(
        mac,
        tracks.map((t) => t.id),
        // -1 means "append" in squeezeInsertQueue.
        -1,
      ));
    }
  }
  commitQueuePlan(plan);
}


// Insert immediately after the current occurrence in physical and playback order.
export async function playNext(tracks: Track[]): Promise<void> {
  const plan = readQueuePlan();
  if (tracks.length === 0) return;
  const currentIdx = plan.queueIndex;
  const addedCount = tracks.length;
  // Insert at currentIdx+1 — directly after current track
  const insertPosition = currentIdx + 1;

  plan.queue = ((q) => {
    const newQueue = [...q];
    newQueue.splice(insertPosition, 0, ...tracks);
    return newQueue;
  })(plan.queue);

  // Preserve an existing immediate prefix behind this higher-priority batch.
  if (plan.userQueueCount > 0) plan.userQueueCount += addedCount;

  // Update shuffled indices — insert at shuffledIndex+1, NOT appended to end.
  if (get(shuffle)) {
    plan.shuffledIndices = ((indices) => {
      // 1. Shift existing indices at or after insertion point
      const shifted = indices.map((i) =>
        i >= insertPosition ? i + addedCount : i,
      );

      // 2. New indices for the inserted tracks
      const newIndices = Array.from(
        { length: addedCount },
        (_, i) => insertPosition + i,
      );

      // Keep the immediate batch in the same order as its physical prefix.

      // 4. Insert at shuffledIndex+1 (not append to end)
      const shufIdx = plan.shuffledIndex;
      const insertAt = shufIdx + 1;

      return [
        ...shifted.slice(0, insertAt),
        ...newIndices,
        ...shifted.slice(insertAt),
      ];
    })(plan.shuffledIndices);
  }

  if (get(activeBackend) === "squeeze") {
    const mac = get(activeSqueezePlayer);
    if (mac) {
      await runOwnedSqueeze(mac, () => squeezeInsertQueue(
        mac,
        tracks.map((t) => t.id),
        insertPosition,
      ));
    }
  }
  commitQueuePlan(plan);
}


// Remove track from queue by index
export async function removeFromQueue(index: number): Promise<void> {
  if (get(currentTrack) && get(queue)[index] && index === get(queueIndex)) {
    throw new PlaybackFailure({ code: "unsupported", message: "The active queue entry cannot be removed; select another entry first", retryable: false });
  }
  const plan = readQueuePlan();
  const currentIdx = plan.queueIndex;
  const sourceIndices = plan.queue.map((_, index) => index);

  sourceIndices.splice(index, 1);
  plan.queue = ((q) => {
    const newQueue = [...q];
    newQueue.splice(index, 1);
    return newQueue;
  })(plan.queue);

  // Adjust current index if needed
  if (index < currentIdx) {
    plan.queueIndex -= 1;
  }

  // Update shuffle indices
  if (get(shuffle)) {
    plan.shuffledIndices = ((indices) => {
      // Remove the deleted index and shift others
      return indices
        .filter((i) => i !== index)
        .map((i) => (i > index ? i - 1 : i));
    })(plan.shuffledIndices);


  }

  // Fix shuffledIndex pointer
  if (get(shuffle)) {
    // We need to find where the current track is now in the shuffled list
    // The current track index in queue might have changed (handled above).
    const actualCurrentQIdx = plan.queueIndex;
    const sIndices = plan.shuffledIndices;
    const ptr = sIndices.indexOf(actualCurrentQIdx);
    if (ptr !== -1) {
      plan.shuffledIndex = ptr;
    }
  }

  // Sync queue change to the active backend


  if (get(activeBackend) === "squeeze") {
    const mac = get(activeSqueezePlayer);
    const current = get(currentTrack);
    if (mac && current) {
      const q = plan.queue;
      await runOwnedSqueeze(mac, () => squeezeUpdateQueue(
        mac,
        q.map((t) => t.id),
        current.id,
        plan.queue[plan.queueIndex]?.id === current.id ? plan.queueIndex : undefined,
        sourceIndices,
      ));
    }
  }
  commitQueuePlan(plan);
}


// Reorder queue (move track from one position to another)
export async function reorderQueue(fromIndex: number, toIndex: number): Promise<void> {
  const plan = readQueuePlan();
  const currentIdx = plan.queueIndex;
  const sourceIndices = plan.queue.map((_, index) => index);
  const isShuffle = get(shuffle);

  if (fromIndex === toIndex) return;

  const queueBefore = plan.queue;
  if (
    fromIndex < 0 ||
    toIndex < 0 ||
    fromIndex >= queueBefore.length ||
    toIndex >= queueBefore.length
  ) {
    return;
  }

  const [source] = sourceIndices.splice(fromIndex, 1);
  sourceIndices.splice(toIndex, 0, source);
  plan.queue = ((q) => {
    const newQueue = [...q];
    const [removed] = newQueue.splice(fromIndex, 1);
    newQueue.splice(toIndex, 0, removed);
    return newQueue;
  })(plan.queue);

  // Adjust current index
  if (fromIndex === currentIdx) {
    plan.queueIndex = toIndex;
  } else if (fromIndex < currentIdx && toIndex >= currentIdx) {
    plan.queueIndex -= 1;
  } else if (fromIndex > currentIdx && toIndex <= currentIdx) {
    plan.queueIndex += 1;
  }

  // Update shuffle indices
  // This is tricky. An item moved from A to B.
  // Indices between A and B shifted.
  // The item at 'fromIndex' is now at 'toIndex'.
  if (isShuffle) {
    plan.shuffledIndices = ((indices) => {
      const fromPos = indices.indexOf(fromIndex);
      const toPos = indices.indexOf(toIndex);

      // First remap numeric queue indices so they still reference
      // the same tracks after the queue array reorder.
      const remapped = indices.map((i) => {
        if (i === fromIndex) return toIndex;
        if (fromIndex < toIndex) {
          // Moved down: items between from+1 and to shift up (-1)
          if (i > fromIndex && i <= toIndex) return i - 1;
        } else {
          // Moved up: items between to and from-1 shift down (+1)
          if (i >= toIndex && i < fromIndex) return i + 1;
        }
        return i;
      });

      // Then reflect manual user intent in the visible shuffled order.
      if (fromPos !== -1 && toPos !== -1 && fromPos !== toPos) {
        const [moved] = remapped.splice(fromPos, 1);
        remapped.splice(toPos, 0, moved);
      }

      return remapped;
    })(plan.shuffledIndices);

    // Keep shuffled cursor aligned to the currently playing queue index.
    const currentQueueIdx = plan.queueIndex;
    const ptr = plan.shuffledIndices.indexOf(currentQueueIdx);
    if (ptr !== -1) {
      plan.shuffledIndex = ptr;
    }
  }

  // Sync queue change to the active backend


  if (get(activeBackend) === "squeeze") {
    const mac = get(activeSqueezePlayer);
    const current = get(currentTrack);
    if (mac && current) {
      const q = plan.queue;
      await runOwnedSqueeze(mac, () => squeezeUpdateQueue(
        mac,
        q.map((t) => t.id),
        current.id,
        plan.queue[plan.queueIndex]?.id === current.id ? plan.queueIndex : undefined,
        sourceIndices,
      ));
    }
  }
  commitQueuePlan(plan);
}


// Clear upcoming queue (keep history)
export async function clearUpcoming(): Promise<void> {
  const plan = readQueuePlan();
  const currentIdx = plan.queueIndex;
  const history = get(shuffle)
    ? plan.shuffledIndices.slice(0, plan.shuffledIndex + 1)
    : plan.queue.map((_, index) => index).slice(0, currentIdx + 1);
  // Keep physical storage stable; only playback order partitions history/upcoming.
  const retained = new Set(history);
  const sourceIndices = plan.queue.map((_, index) => index).filter(index => retained.has(index));
  plan.queue = sourceIndices.map(index => plan.queue[index]);
  plan.queueIndex = Math.max(0, sourceIndices.indexOf(currentIdx));
  plan.userQueueCount = 0;
  if (get(shuffle)) {
    plan.shuffledIndices = history.map(index => sourceIndices.indexOf(index));
    plan.shuffledIndex = Math.max(0, history.length - 1);
  }

  if (get(activeBackend) === "squeeze") {
    const mac = get(activeSqueezePlayer);
    const current = get(currentTrack);
    if (mac && current) await runOwnedSqueeze(mac, () => squeezeUpdateQueue(mac, plan.queue.map(t => t.id), current.id, plan.queue[plan.queueIndex]?.id === current.id ? plan.queueIndex : undefined, sourceIndices));
  }
  commitQueuePlan(plan);
}


// Play from specific index in queue
export async function playFromQueue(index: number): Promise<void> {
  const plan = readQueuePlan();
  if (!plan.queue[index]) throw new Error("Queue entry is unavailable");
  const userQueueEnd = plan.queueIndex + 1 + plan.userQueueCount;
  if (index > plan.queueIndex && index <= userQueueEnd) plan.userQueueCount = Math.max(0, plan.userQueueCount - (index - plan.queueIndex));
  else if (index > userQueueEnd) plan.userQueueCount = 0;
  plan.queueIndex = index;
  if (get(shuffle)) {
    const pointer = plan.shuffledIndices.indexOf(index);
    if (pointer !== -1) plan.shuffledIndex = pointer;
  }
  await playTrack(plan.queue[index], false, 0, { tracks: plan.queue, index, commit: () => commitQueuePlan(plan) });
}

/**
 * Helper to check if a specific playlist is currently playing
 */
export function isPlaylistPlaying(playlistId: number): boolean {
  const ctx = get(playbackContext);
  return ctx?.type === "playlist" && ctx.playlistId === playlistId;
}

/**
 * Helper to check if a specific album is currently playing
 */
export function isAlbumPlaying(albumId: number): boolean {
  const ctx = get(playbackContext);
  return ctx?.type === "album" && ctx.albumId === albumId;
}

/**
 * Helper to check if a specific artist is currently playing
 */
export function isArtistPlaying(artistName: string): boolean {
  const ctx = get(playbackContext);
  return ctx?.type === "artist" && ctx.artistName === artistName;
}

/**
 * Transfer playback from a remote device to this one.
 */
export async function transferPlayback(state: any) {
  if (!state || !state.track) return;

  console.log(
    "[Player] Transferring playback to this device...",
    state.track.title,
  );

  // 1. Stop remote playback (by sending a command to specific device)
  if (state.deviceId) {
    console.log("[Player] Pausing remote device:", state.deviceId);
    sendRemoteCommand(state.deviceId, "pause");
  }

  // 2. Resolve the local track object if possible (Fast ID lookup first)
  const remoteTrack = state.track;
  let localTrack: any = getTrackByIdSync(Number(remoteTrack.id));

  if (!localTrack) {
    // Falling back to O(N) search
    const $library = get(libraryTracks);
    localTrack = $library.find(
      (t) => t.title === remoteTrack.title && t.artist === remoteTrack.artist,
    );
  }

  if (localTrack) {
    // Use local cover for better reliability
    // We spread localTrack to have full metadata (album_id, path, etc.)
    const trackWithLocalCover = {
      ...state.track,
      ...localTrack,
      coverUrl: getTrackCoverSrc(localTrack),
    };

    await playTrack(localTrack, false, state.currentTime);
    if (!state.isPlaying) {
      await pause();
    }
  } else {
    // If not found in local library, we might need to "External Track" play (later feature)
    console.warn(
      "[Player] Could not find local track for transfer:",
      state.track.title,
    );
    addToast(
      `Cannot transfer: "${state.track.title}" not found in local library`,
      "error",
    );
  }
}

/**
 * Control a remote device.
 */
export function sendRemoteCommand(
  targetDeviceId: string,
  command: string,
  data?: any,
) {
  wsStore.send("remote_command", {
    targetDeviceId,
    command,
    data,
  });
}

/**
 * Throttled version of sendRemoteCommand for high-frequency events like seeking or volume slides.
 */
let remoteThrottleTimers: Record<string, ReturnType<typeof setTimeout>> = {};
function throttledRemoteCommand(
  targetDeviceId: string,
  command: string,
  data: any,
  delay: number,
) {
  const key = `${targetDeviceId}:${command}`;
  if (remoteThrottleTimers[key]) return;

  sendRemoteCommand(targetDeviceId, command, data);

  remoteThrottleTimers[key] = setTimeout(() => {
    delete remoteThrottleTimers[key];
  }, delay);
}

let squeezeVolumeTimer: ReturnType<typeof setTimeout> | null = null;
let squeezeVolumePending: { mac: string; vol: number } | null = null;
function throttledSqueezeVolume(mac: string, vol: number) {
  squeezeVolumePending = { mac, vol };
  if (squeezeVolumeTimer) return;
  squeezeSetVolume(mac, vol).catch(console.error);
  squeezeVolumeTimer = setTimeout(() => {
    squeezeVolumeTimer = null;
    if (squeezeVolumePending) {
      squeezeSetVolume(
        squeezeVolumePending.mac,
        squeezeVolumePending.vol,
      ).catch(console.error);
      squeezeVolumePending = null;
    }
  }, 150);
}

/**
 * Handle a remote command received via WebSocket.
 */
async function handleRemoteCommand(payload: any) {
  const { command, data } = payload;
  const dispatch = async (intent: import("../types").ApplicationIntent, fallback: () => Promise<void>) => {
    if (commandSink) await commandSink(intent); else await fallback();
  };
  switch (command) {
    case "resume": await dispatch({ type: "resume" }, resume); break;
    case "pause": await dispatch({ type: "pause" }, pause); break;
    case "next": await dispatch({ type: "next" }, nextTrack); break;
    case "previous": await dispatch({ type: "previous" }, previousTrack); break;
    case "seek": if (data?.position != null) await dispatch({ type: "seek", seconds: data.position * get(duration) }, () => seek(data.position)); break;
    case "volume": if (data?.volume != null) await dispatch({ type: "set_volume", volume: data.volume }, () => setVolume(data.volume)); break;
    case "shuffle": if (data?.shuffle != null) await dispatch({ type: "set_shuffle", enabled: data.shuffle }, async () => { if (get(shuffle) !== data.shuffle) await toggleShuffle(); }); break;
    case "repeat": if (["none", "all", "one"].includes(data?.repeat)) await dispatch({ type: "set_repeat", mode: data.repeat }, () => setRepeatMode(data.repeat)); break;
  }
}

export function selectThisDevice() {
    if (get(activeBackend) === "squeeze" && get(activeSqueezePlayer)) {
      disconnectSqueezePlayer(get(activeSqueezePlayer)!);
    } else if (get(activeBackend) === "remote") {
      activeBackend.set("none");
      activeRemoteDevice.set(null);
    }
  }

export function toggleRemoteControl(device: RemoteDevice) {
    if (
      get(activeBackend) === "remote" &&
      get(activeRemoteDevice) === device.deviceId
    ) {
      activeBackend.set("none");
      activeRemoteDevice.set(null);
    } else {
      activeBackend.set("remote");
      activeRemoteDevice.set(device.deviceId);

      if (device.playerState && device.playerState.track) {
        const remoteTrack = device.playerState.track;
        const remotePlaying = device.playerState.isPlaying;
        const remoteTrackId = Number(remoteTrack.id);

        let localTrack: any = getTrackByIdSync(remoteTrackId);
        if (!localTrack) {
          const $library = get(libraryTracks);
          localTrack = $library.find(
            (t) =>
              t.title === remoteTrack.title && t.artist === remoteTrack.artist,
          );
        }

        currentTrack.set({
          ...remoteTrack,
          ...(localTrack || {}),
          id: remoteTrackId,
          track_cover: localTrack
            ? getTrackCoverSrc(localTrack)
            : remoteTrack.coverUrl,
        } as any);

        isPlaying.set(remotePlaying);
      }
    }
  }

/** Backend completion sources enter the same desktop command lane. */
type CompletionEmitter = (kind: "completion" | "gapless") => Promise<void>;
let completionSink: (() => CompletionEmitter) | undefined;
export function bindPlaybackSignals(sink: typeof completionSink): void { completionSink = sink; refreshPlaybackSignalOwnership(); }
function handleTrackEnd(emit = completionSink?.()): void { void (emit ? emit("completion") : applyTrackEnd()).catch(console.error); }
function handleGaplessAdvance(emit = completionSink?.()): void { void (emit ? emit("gapless") : applyGaplessAdvance()).catch(console.error); }
/** Stop the owned PC pipeline before confirming a different output. */
export async function stopLocalOutput(): Promise<void> {
  if (get(activeBackend) === "native") await nativeAudioStop();
  if (html5Audio) { html5Audio.pause(); html5Audio.currentTime = 0; }
  isPlaying.set(false);
  currentTime.set(0);
}

function readQueuePlan() {
  return { queue: get(queue), queueIndex: get(queueIndex), userQueueCount: get(userQueueCount), shuffledIndices: get(shuffledIndices), shuffledIndex: get(shuffledIndex) };
}
function commitQueuePlan(plan: ReturnType<typeof readQueuePlan>): void {
  queue.set(plan.queue);
  queueIndex.set(plan.queueIndex);
  userQueueCount.set(plan.userQueueCount);
  shuffledIndices.set(plan.shuffledIndices);
  shuffledIndex.set(plan.shuffledIndex);
  pluginEvents.emit("queueChange", { queue: plan.queue, index: plan.queueIndex });
  _schedulePreload();
}

let transferSink: ((payload: unknown) => Promise<void>) | undefined;
export function bindDesktopTransfers(sink: typeof transferSink): void { transferSink = sink; }

let commandSink: ((intent: import("../types").ApplicationIntent) => Promise<void>) | undefined;
export function bindDesktopCommands(sink: typeof commandSink): void { commandSink = sink; }
function emitDesktopCommand(intent: import("../types").ApplicationIntent, fallback: () => Promise<void>): void {
  void (commandSink ? commandSink(intent) : fallback()).catch(console.error);
}

async function runOwnedSqueeze(mac: string, operation: () => Promise<void>): Promise<void> {
  const owns = captureSqueezeTargetOwnership(mac);
  try { await operation(); } catch (error) {
    if (String(error).includes("SQUEEZE_QUEUE_BUSY:")) {
      throw new PlaybackFailure({ code: "unsupported", message: String(error), retryable: true });
    }
    throw error;
  }
  if (!owns()) throw new PlaybackFailure({ code: "revision_conflict", message: "Squeeze output ownership changed", retryable: false }, "superseded", ["Previous Squeeze target acknowledged the operation"]);
}

async function navigateSqueeze(mac: string, operation: () => Promise<void>): Promise<void> {
  await runOwnedSqueeze(mac, operation);
  try {
    await runOwnedSqueeze(mac, () => reconcileSqueezeAcknowledgement(mac));
  } catch (error) {
    if (error instanceof PlaybackFailure) throw error;
    throw new PlaybackFailure({ code: "execution_failed", message: String(error), retryable: true }, "failed", ["Squeeze navigation acknowledged"]);
  }
}
