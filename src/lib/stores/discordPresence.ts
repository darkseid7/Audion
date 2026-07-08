// Discord Rich Presence orchestrator — bridges player lifecycle to the Tauri backend IPC.
// Subscribes to Svelte stores and calls discord_connect / discord_update_presence /
// discord_clear_presence / discord_disconnect / discord_reconnect as needed.
// Throttles time updates to one every 5 seconds; force-flushes on track &
// play/pause edges.
//
// Debug logging convention: every log line carries the [DiscordPresence] tag.
// Edge events (init / track change / play-pause / connect / clear / dispose /
// reconnect) are logged on every fire. The 5s heartbeat tick is logged once
// per minute to confirm the throttle loop is alive without flooding the
// console.

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

const TAG = "[DiscordPresence]";

let unsubscribeAll: (() => void) | null = null;
let throttleTimer: ReturnType<typeof setInterval> | null = null;
let connected = false;
let connectingPromise: Promise<boolean> | null = null;
let lastTrackId: number | null = null;
let lastSentCurrentTimeSec = -1;
let throttleTickCount = 0;

// ── internal helpers ──────────────────────────────────────────────────────────

/**
 * Ensures the Discord IPC connection is established.
 * Multiple concurrent callers share a single connect attempt — idempotent.
 */
async function ensureConnected(): Promise<boolean> {
  if (connected) {
    console.log(`${TAG} ensureConnected: already connected, reusing`);
    return true;
  }
  if (connectingPromise) {
    console.log(`${TAG} ensureConnected: another caller is already connecting, waiting`);
    return connectingPromise;
  }

  console.log(`${TAG} ensureConnected: invoking discord_connect…`);
  connectingPromise = invoke<string>("discord_connect")
    .then((result) => {
      console.log(`${TAG} ensureConnected: backend returned "${result}"`);
      if (result === "Connected to Discord" || result === "Already connected") {
        connected = true;
        return true;
      }
      console.warn(`${TAG} ensureConnected: unexpected result, marking disconnected`);
      connected = false;
      return false;
    })
    .catch((e) => {
      // Discord is not running (or rejected the connection).
      console.warn(`${TAG} ensureConnected: connect failed — is Discord open?`, e);
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
async function sendPresencePayload(reason: string): Promise<void> {
  const showDiscord = get(appSettings).showDiscord;
  if (!showDiscord) {
    console.log(`${TAG} sendPresencePayload (${reason}): skipped — showDiscord=false`);
    return;
  }
  const track = get(currentTrack);
  if (!track) {
    console.log(`${TAG} sendPresencePayload (${reason}): skipped — no currentTrack`);
    return;
  }

  if (!(await ensureConnected())) {
    console.warn(`${TAG} sendPresencePayload (${reason}): skipped — not connected to Discord`);
    return;
  }

  // Cover handling: Discord fetches cover images from its own servers, so the
  // URL must be publicly reachable. For local tracks, getTrackCoverSrc returns
  // a Tauri asset protocol URL (http://asset.localhost/...) which Discord
  // cannot resolve. We therefore only forward cover_url when the track comes
  // from a streaming source with a real public URL. For everything else we
  // omit the field and let the backend fall back to the audion_logo asset.
  const isLocal = !track.source_type || track.source_type === "local";
  const coverUrl =
    !isLocal && track.cover_url ? track.cover_url : undefined;
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

  console.log(
    `${TAG} sendPresencePayload (${reason}): track="${track.title}" artist="${track.artist}" playing=${playing} cover=${coverUrl ? "yes" : isLocal ? "skipped-local" : "no"} cur=${curTime.toFixed(1)}s dur=${dur.toFixed(1)}s`,
  );

  try {
    await invoke("discord_update_presence", { data: payload });
    console.log(`${TAG} sendPresencePayload (${reason}): backend accepted update`);
  } catch (e) {
    console.warn(`${TAG} sendPresencePayload (${reason}): backend rejected update`, e);
  }
}

function onThrottleTick(): void {
  throttleTickCount++;
  const track = get(currentTrack);
  const playing = get(isPlaying);
  const curTime = get(currentTime);

  // Only resend when currentTime has advanced at least ~1s since the last
  // successful send, so the throttle doesn't waste Discord calls.
  if (Math.abs(curTime - lastSentCurrentTimeSec) < 1) {
    // Heartbeat: log only once per minute to confirm the loop is alive.
    if (throttleTickCount % 12 === 0) {
      console.log(
        `${TAG} heartbeat: connected=${connected} track=${track ? track.title : "<none>"} playing=${playing} cur=${curTime.toFixed(1)}s`,
      );
    }
    return;
  }

  sendPresencePayload("throttle").then(() => {
    lastSentCurrentTimeSec = curTime;
  });
  if (throttleTickCount % 12 === 0) {
    console.log(`${TAG} heartbeat: tick=${throttleTickCount}`);
  }
}

function startThrottle(): void {
  if (throttleTimer) return;
  console.log(`${TAG} startThrottle: starting 5s tick`);
  throttleTickCount = 0;
  throttleTimer = setInterval(onThrottleTick, 5000);
}

function stopThrottle(): void {
  if (throttleTimer) {
    console.log(`${TAG} stopThrottle: clearing tick`);
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
  if (unsubscribeAll) {
    console.log(`${TAG} init: already initialised, no-op`);
    return;
  }
  console.log(`${TAG} init: installing subscriptions (showDiscord defaults to ${get(appSettings).showDiscord})`);

  // Guards skip the initial fire that Svelte stores emit on subscribe.
  let trackReady = false;
  let playingReady = false;
  let settingsReady = false;

  const unsubTrack = currentTrack.subscribe((track) => {
    if (!trackReady) {
      trackReady = true;
      console.log(`${TAG} track subscribe: initial fire consumed (track=${track ? track.title : "<none>"})`);
      return;
    }
    if (!get(appSettings).showDiscord) {
      console.log(`${TAG} track subscribe: ignored — showDiscord=false`);
      return;
    }

    if (!track) {
      // Track cleared (stop / end) — clear display but keep connection.
      console.log(`${TAG} track cleared — calling discord_clear_presence`);
      lastTrackId = null;
      lastSentCurrentTimeSec = -1;
      stopThrottle();
      invoke("discord_clear_presence").catch((e) =>
        console.warn(`${TAG} discord_clear_presence failed`, e),
      );
      return;
    }

    if (track.id !== lastTrackId) {
      console.log(`${TAG} track change detected: id=${track.id} title="${track.title}"`);
      lastTrackId = track.id;
      lastSentCurrentTimeSec = -1;
      // If we're paused, activity is cleared (Spotify-style) — don't push a
      // paused-state payload for the new track; it would just show a phantom
      // card. Sending happens on resume.
      if (get(isPlaying)) {
        sendPresencePayload("track-change");
        startThrottle();
      } else {
        console.log(`${TAG} track changed while paused — activity stays cleared`);
        stopThrottle();
      }
    } else {
      console.log(`${TAG} track subscribe: same track id=${track.id}, no-op`);
    }
  });

  const unsubPlaying = isPlaying.subscribe((playing) => {
    if (!playingReady) {
      playingReady = true;
      console.log(`${TAG} playing subscribe: initial fire consumed (playing=${playing})`);
      return;
    }
    if (!get(appSettings).showDiscord) {
      console.log(`${TAG} playing subscribe: ignored — showDiscord=false`);
      return;
    }
    const track = get(currentTrack);
    if (!track) {
      console.log(`${TAG} playing subscribe: ignored — no currentTrack`);
      return;
    }

    console.log(`${TAG} play/pause edge: playing=${playing} track="${track.title}"`);
    if (playing) {
      sendPresencePayload("play-on");
      startThrottle();
    } else {
      // Spotify-style: pause immediately hides the activity card from
      // Discord instead of leaving a count-up timestamp running.
      stopThrottle();
      invoke("discord_clear_presence").catch((e) =>
        console.warn(`${TAG} play-off: discord_clear_presence failed`, e),
      );
    }
  });

  const unsubSettings = appSettings.subscribe((settings) => {
    if (!settingsReady) {
      settingsReady = true;
      console.log(`${TAG} settings subscribe: initial fire consumed (showDiscord=${settings.showDiscord})`);
      return;
    }

    console.log(`${TAG} settings changed: showDiscord=${settings.showDiscord}`);

    if (!settings.showDiscord) {
      console.log(`${TAG} toggle OFF — clearing presence and disconnecting`);
      stopThrottle();
      lastTrackId = null;
      lastSentCurrentTimeSec = -1;
      invoke("discord_clear_presence").catch((e) =>
        console.warn(`${TAG} discord_clear_presence failed`, e),
      );
      invoke("discord_disconnect").catch((e) =>
        console.warn(`${TAG} discord_disconnect failed`, e),
      );
      connected = false;
      connectingPromise = null;
    }
    // Toggle ON: intentionally does NOT auto-connect — wait for next
    // track-change / play edge so we avoid a noisy connect attempt at startup
    // or when Discord isn't running.
  });

  unsubscribeAll = () => {
    console.log(`${TAG} disposing subscriptions`);
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
  console.log(`${TAG} dispose: tearing down`);
  unsubscribeAll?.();
  unsubscribeAll = null;
  stopThrottle();
  lastTrackId = null;
  lastSentCurrentTimeSec = -1;
  connected = false;
  connectingPromise = null;

  invoke("discord_clear_presence").catch((e) =>
    console.warn(`${TAG} dispose: discord_clear_presence failed`, e),
  );
  invoke("discord_disconnect").catch((e) =>
    console.warn(`${TAG} dispose: discord_disconnect failed`, e),
  );
}

/**
 * Forces a disconnect + reconnect cycle. Useful as a Settings button for
 * recovery when Discord was restarted or the IPC connection was lost.
 */
export async function reconnectDiscord(): Promise<void> {
  console.log(`${TAG} reconnect: invoked from Settings`);
  try {
    const result = await invoke<string>("discord_reconnect");
    console.log(`${TAG} reconnect: backend returned "${result}"`);
    connected = true;
    console.log(`${TAG} reconnect: success`);
  } catch (e) {
    connected = false;
    console.warn(`${TAG} reconnect: failed — is Discord open?`, e);
  }
}
