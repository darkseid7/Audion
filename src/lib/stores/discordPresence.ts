// Discord Rich Presence orchestrator — bridges player lifecycle to the Tauri backend IPC.
// Subscribes to Svelte stores and calls discord_connect / discord_update_presence /
// discord_clear_presence / discord_disconnect / discord_reconnect as needed.
// Throttles time updates to one every 5 seconds; force-flushes on track &
// play/pause edges.

import { get } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import {
  currentTrack,
  isPlaying,
  currentTime,
  duration,
} from "$lib/stores/player";
import { appSettings } from "$lib/stores/settings";
import { getTrackCoverSrc } from "$lib/api/tauri";

let unsubscribeAll: (() => void) | null = null;
let throttleTimer: ReturnType<typeof setInterval> | null = null;
let connected = false;
let connectingPromise: Promise<boolean> | null = null;
let lastTrackId: number | null = null;

// ── internal helpers ──────────────────────────────────────────────────────────

/**
 * Ensures the Discord IPC connection is established.
 * Multiple concurrent callers share a single connect attempt — idempotent.
 */
async function ensureConnected(): Promise<boolean> {
  if (connected) return true;
  if (connectingPromise) return connectingPromise;

  connectingPromise = invoke<string>("discord_connect")
    .then((result) => {
      if (result === "Connected to Discord" || result === "Already connected") {
        connected = true;
        return true;
      }
      console.warn("[DiscordPresence] Unexpected connect result:", result);
      connected = false;
      return false;
    })
    .catch((e) => {
      // Discord is not running — swallow and set disconnected
      console.warn("[DiscordPresence] Discord is not running:", e);
      connected = false;
      return false;
    })
    .finally(() => {
      connectingPromise = null;
    });

  return connectingPromise;
}

/**
 * Reads current stores, builds a PresenceData payload, and sends it via IPC.
 * No-op when showDiscord is off or no track is loaded.
 */
async function sendPresencePayload(): Promise<void> {
  if (!get(appSettings).showDiscord) return;
  const track = get(currentTrack);
  if (!track) return;

  if (!(await ensureConnected())) return;

  const coverSrc = getTrackCoverSrc(track);
  const coverUrl =
    coverSrc &&
    (coverSrc.startsWith("http://") || coverSrc.startsWith("https://"))
      ? coverSrc
      : undefined;
  const dur = get(duration);
  const curTime = get(currentTime);
  const playing = get(isPlaying);

  const payload: Record<string, unknown> = {
    line1: track.title ?? "Unknown",
    line2: track.artist ?? "Unknown",
    status_display_type: "details",
    is_playing: playing,
    show_pause_icon: !playing,
    app_name: "Audion",
  };

  if (track.album) payload.line3 = track.album;
  if (coverUrl) payload.cover_url = coverUrl;
  if (dur > 0) {
    payload.current_time = Math.round(curTime * 1000);
    payload.duration = Math.round(dur * 1000);
  }

  try {
    await invoke("discord_update_presence", { data: payload });
  } catch (e) {
    console.warn("[DiscordPresence] Update failed:", e);
  }
}

function startThrottle(): void {
  if (throttleTimer) return;
  throttleTimer = setInterval(sendPresencePayload, 5000);
}

function stopThrottle(): void {
  if (throttleTimer) {
    clearInterval(throttleTimer);
    throttleTimer = null;
  }
}

// ── public API ────────────────────────────────────────────────────────────────

/**
 * Installs Svelte store subscriptions that react to track, play/pause, and
 * settings changes. Idempotent — safe to call more than once.
 */
export function initDiscordPresence(): void {
  if (unsubscribeAll) return;

  // Guards skip the initial fire that Svelte stores emit on subscribe.
  let trackReady = false;
  let playingReady = false;
  let settingsReady = false;

  const unsubTrack = currentTrack.subscribe((track) => {
    if (!trackReady) {
      trackReady = true;
      return;
    }
    if (!get(appSettings).showDiscord) return;

    if (!track) {
      // Track cleared (stop / end) — clear display but keep connection.
      lastTrackId = null;
      stopThrottle();
      invoke("discord_clear_presence").catch(() => {});
      return;
    }

    if (track.id !== lastTrackId) {
      lastTrackId = track.id;
      sendPresencePayload();
      if (get(isPlaying)) startThrottle();
      else stopThrottle();
    }
  });

  const unsubPlaying = isPlaying.subscribe((playing) => {
    if (!playingReady) {
      playingReady = true;
      return;
    }
    if (!get(appSettings).showDiscord) return;
    const track = get(currentTrack);
    if (!track) return;

    sendPresencePayload();
    if (playing) startThrottle();
    else stopThrottle();
  });

  const unsubSettings = appSettings.subscribe((settings) => {
    if (!settingsReady) {
      settingsReady = true;
      return;
    }

    if (!settings.showDiscord) {
      stopThrottle();
      lastTrackId = null;
      invoke("discord_clear_presence").catch(() => {});
      invoke("discord_disconnect").catch(() => {});
      connected = false;
      connectingPromise = null;
    }
    // Toggle ON: intentionally does NOT auto-connect — wait for next
    // track-change / play edge so we avoid a noisy connect attempt at startup
    // or when Discord isn't running.
  });

  unsubscribeAll = () => {
    unsubTrack();
    unsubPlaying();
    unsubSettings();
  };
}

/**
 * Tears down all subscriptions, clears Discord presence, and disconnects.
 * Call from onDestroy / beforeunload / hot-reload disposal.
 */
export function disposeDiscordPresence(): void {
  unsubscribeAll?.();
  unsubscribeAll = null;
  stopThrottle();
  lastTrackId = null;
  connected = false;
  connectingPromise = null;

  invoke("discord_clear_presence").catch(() => {});
  invoke("discord_disconnect").catch(() => {});
}

/**
 * Forces a disconnect + reconnect cycle. Useful as a Settings button for
 * recovery when Discord was restarted or the IPC connection was lost.
 */
export async function reconnectDiscord(): Promise<void> {
  try {
    await invoke("discord_reconnect");
    connected = true;
    console.log("[DiscordPresence] Reconnected to Discord");
  } catch (e) {
    console.warn("[DiscordPresence] Reconnect failed:", e);
    connected = false;
  }
}
