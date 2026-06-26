import { writable, get } from "svelte/store";
import {
  squeezeGetPlayerState,
  squeezeGetPlayers,
  squeezeStop,
  squeezeStartServer,
  squeezeIsRunning,
  squeezePlay,
  getTrackCoverSrc,
  getTrackById,
  type SqueezePlayerInfo,
  type Track,
} from "$lib/api/tauri";
import {
  currentTrack,
  isPlaying,
  currentTime,
  duration,
  volume,
  activeBackend,
  shuffle,
  repeat,
  queue,
  queueIndex,
} from "$lib/stores/player";
import { activeRemoteDevice } from "$lib/stores/websocket";
import {
  getTrackByIdSync,
  incrementPlayCount,
  cacheTrack,
} from "$lib/stores/library";
import { recordTrackPlay } from "$lib/stores/activity";

export const activeSqueezePlayer = writable<string | null>(null);
export const squeezePlayerState = writable<SqueezePlayerInfo | null>(null);

/**
 * Players currently visible on the network. Updated by the module-level
 * discovery poll started in `startGlobalSqueezeDiscovery()` so every
 * consumer (notably the ConnectPanel) sees the same list without having
 * to mount and run its own setInterval.
 */
export const discoveredSqueezePlayers = writable<SqueezePlayerInfo[]>([]);

let pollInterval: ReturnType<typeof setInterval> | null = null;
let volumeCooldownUntil = 0;

let discoveryInterval: ReturnType<typeof setInterval> | null = null;
let discoveryStarting = false;

/**
 * Idempotent. Starts a 1 Hz poll that:
 *   1. Keeps `discoveredSqueezePlayers` in sync with the backend.
 *   2. Auto-selects the first discovered player when nothing is active,
 *      so an Eversolo (or any Squeeze player) that comes online is
 *      picked up immediately without the user having to open the
 *      Connect panel.
 *
 * The poll keeps running as long as the Squeeze server is up; it stops
 * when `stopGlobalSqueezeDiscovery()` is called (e.g. when the user
 * hits the Stop button in the Connect panel).
 */
export async function startGlobalSqueezeDiscovery(): Promise<void> {
  if (discoveryInterval || discoveryStarting) return;
  discoveryStarting = true;
  try {
    // Make sure the server is up before we start polling. If it's already
    // running this resolves immediately; otherwise it boots it.
    if (!(await squeezeIsRunning().catch(() => false))) {
      await squeezeStartServer().catch((e) =>
        console.warn("[SQUEEZE] Auto-start failed:", e),
      );
    }
    // First tick immediately so the UI doesn't have to wait a full
    // second for the initial state.
    await pollSqueezePlayersOnce();
    discoveryInterval = setInterval(pollSqueezePlayersOnce, 1000);
  } finally {
    discoveryStarting = false;
  }
}

export function stopGlobalSqueezeDiscovery(): void {
  if (discoveryInterval) {
    clearInterval(discoveryInterval);
    discoveryInterval = null;
  }
  discoveredSqueezePlayers.set([]);
}

async function pollSqueezePlayersOnce(): Promise<void> {
  try {
    const players = await squeezeGetPlayers();
    discoveredSqueezePlayers.set(players ?? []);
    // Auto-connect: pick the first available player when no squeeze
    // player is currently selected. Mirrors the previous in-panel logic
    // so the experience is identical whether the user opens the Connect
    // panel or just lets the Eversolo come online on its own.
    if (players && players.length > 0 && !get(activeSqueezePlayer)) {
      activeSqueezePlayer.set(players[0].mac);
      activeBackend.set("squeeze");
      activeRemoteDevice.set(null);
    }
  } catch {
    // Server may have been stopped mid-tick; ignore.
  }
}

// Watchdog: some LMS hardware players (e.g. Eversolo) don't transition to
// "Stopped" after the last track of an album finishes — they keep reporting
// state="Playing" with elapsed_ms past the track duration, so the UI would
// stay stuck on the pause button and the play count would never increment.
// We detect that with a short delay and force the player to stop + record
// the play. Reset on every track change.
let pendingSqueezeForcedEnd: ReturnType<typeof setTimeout> | null = null;
let lastForcedEndTrackId: number | null = null;
function clearPendingSqueezeForcedEnd() {
  if (pendingSqueezeForcedEnd !== null) {
    clearTimeout(pendingSqueezeForcedEnd);
    pendingSqueezeForcedEnd = null;
  }
}

export function setSqueezeVolumeCooldown() {
  volumeCooldownUntil = Date.now() + 2000;
}

activeSqueezePlayer.subscribe((mac) => {
  if (pollInterval) {
    clearInterval(pollInterval);
    pollInterval = null;
  }

  if (mac) {
    pollSqueezeState(mac);
    pollInterval = setInterval(() => pollSqueezeState(mac), 500);
  } else {
    squeezePlayerState.set(null);
  }
});

async function pollSqueezeState(mac: string) {
  if (get(activeBackend) !== "squeeze") return;

  try {
    const info = await squeezeGetPlayerState(mac);
    squeezePlayerState.set(info);

    const playing = info.state === "Playing";
    if (get(isPlaying) !== playing) isPlaying.set(playing);

    const prevTrack = get(currentTrack);
    const prevElapsed = get(currentTime);
    const elapsed = info.elapsed_ms / 1000;
    // Clamp to the displayed duration so the counter never runs past the end
    // of a track. LMS / hardware players (e.g. Eversolo) can keep reporting
    // elapsed_ms past the track duration for the last track of an album,
    // which would otherwise make the counter count to infinity.
    const dur = get(duration);
    currentTime.set(dur > 0 ? Math.min(elapsed, dur) : elapsed);

    if (info.current_track) {
      const trackDur = info.current_track.duration;
      if (get(duration) !== trackDur) duration.set(trackDur);

      const currentObj = get(currentTrack);
      let localTrack = getTrackByIdSync(info.current_track.id);
      const sameTrack = currentObj?.id === info.current_track.id;

      // If track not in memory cache, fetch from backend and cache it
      if (!localTrack && !sameTrack) {
        try {
          const fetched = await getTrackById(info.current_track.id);
          if (fetched) {
            cacheTrack(fetched);
            localTrack = fetched;
          }
        } catch {
          /* non-critical */
        }
      }

      const canUpgradeFromLocal =
        sameTrack &&
        !!localTrack &&
        ((!currentObj?.track_cover_path && !!localTrack.track_cover_path) ||
          (!currentObj?.track_cover && !!localTrack.track_cover) ||
          (!currentObj?.cover_url && !!localTrack.cover_url) ||
          (!currentObj?.album_id && !!localTrack.album_id));

      // Update when track changed, or when same track can be enriched with local metadata.
      if (!sameTrack || canUpgradeFromLocal) {
        // In squeeze mode, track transitions are driven by state polling, not native/html5 end events.
        // Record the previous track play when we detect a real track-id change.
        if (!sameTrack && prevTrack && prevTrack.id !== info.current_track.id) {
          const durationPlayed = Math.floor(prevElapsed);
          if (durationPlayed > 5) {
            void recordTrackPlay(
              prevTrack.id,
              prevTrack.album_id ?? null,
              durationPlayed,
            );
            incrementPlayCount(prevTrack.id);
          }
        }

        if (localTrack) {
          currentTrack.set({
            ...localTrack,
            track_cover: getTrackCoverSrc(localTrack),
          } as any);
        } else if (!sameTrack) {
          currentTrack.set({
            id: info.current_track.id,
            title: info.current_track.title,
            artist: info.current_track.artist,
            album: info.current_track.album,
            path: info.current_track.path,
            duration: info.current_track.duration,
          } as any);
        }
      }
    } else {
      if (get(currentTrack) !== null) currentTrack.set(null);
    }

    if (Date.now() > volumeCooldownUntil) {
      const vol = info.volume / 100;
      if (Math.abs(get(volume) - vol) > 0.01) volume.set(vol);
    }

    const shuf = info.shuffle;
    if (get(shuffle) !== shuf) shuffle.set(shuf);

    const rep =
      info.repeat === "Off" ? "none" : info.repeat === "One" ? "one" : "all";
    if (get(repeat) !== rep) repeat.set(rep);

    // Watchdog: if LMS says "Playing" but the position is at/past the track
    // duration, the player hasn't realised the track ended (common on the
    // Eversolo and similar hardware for the last track of an album). Force
    // the end after a short delay so the UI updates, the play count is
    // recorded, and the player actually stops.
    const trackId = info.current_track?.id ?? null;
    if (
      playing &&
      info.current_track &&
      info.current_track.duration > 0 &&
      elapsed >= info.current_track.duration - 0.05 &&
      lastForcedEndTrackId !== trackId
    ) {
      if (pendingSqueezeForcedEnd === null) {
        pendingSqueezeForcedEnd = setTimeout(() => {
          pendingSqueezeForcedEnd = null;
          if (get(activeBackend) !== "squeeze") return;
          const st = get(currentTime);
          const du = get(duration);
          if (du > 0 && st >= du - 0.1 && get(isPlaying)) {
            console.warn(
              "[Player] Forced end via squeeze watchdog (LMS stuck at track end)",
            );
            // Record the play for the track that's about to be stopped.
            const track = get(currentTrack);
            if (track) {
              const durationPlayed = Math.floor(st);
              if (durationPlayed > 5) {
                void recordTrackPlay(
                  track.id,
                  track.album_id ?? null,
                  durationPlayed,
                );
                incrementPlayCount(track.id);
              }
            }
            // Stop the LMS player so it transitions to "Stopped" — the
            // next poll will then drive isPlaying to false normally.
            squeezeStop(mac).catch(console.error);
            // Mark this track so we don't fire the watchdog again for it.
            lastForcedEndTrackId = trackId;
          }
        }, 1500);
      }
    } else {
      clearPendingSqueezeForcedEnd();
      // Only reset the forced-end flag when the current track actually
      // changes — otherwise a restart of the same track would let the
      // watchdog fire a second time and double-count the play.
      const currentTrackId = info.current_track?.id ?? null;
      if (currentTrackId !== lastForcedEndTrackId) {
        lastForcedEndTrackId = null;
      }
    }
  } catch {
    // Player may have disconnected
  }
}

// ── Session persistence ───────────────────────────────────────────────────────
//
// When the user has been playing music through a Squeeze player and
// then either closes the app or the Eversolo drops its TCP/HTTP
// connection to Audion, we want the next time that same device comes
// back online to pick up roughly where they left off — same queue,
// same current track. Exact minute-precise position isn't required,
// so we only persist track IDs + start index + shuffle/repeat.

export interface SqueezeSession {
  mac: string;
  trackIds: number[];
  startIndex: number;
  shuffle: boolean;
  repeat: "none" | "one" | "all";
  wasPlaying: boolean;
  savedAt: number;
}

const SESSION_KEY_PREFIX = "rlist_squeeze_session_";
const SESSION_VERSION = 1;

// MACs we've already auto-restored in this app session, so we don't
// replay the same queue every time the discovery poll re-selects them.
const restoredMacs = new Set<string>();
// True for ~2s after a programmatic restore so the subscribers below
// don't immediately overwrite the freshly-restored session with the
// still-empty local queue.
let suppressSaveUntil = 0;

export function saveSqueezeSession(mac: string): void {
  if (typeof window === "undefined") return;
  if (Date.now() < suppressSaveUntil) return;
  if (get(activeBackend) !== "squeeze") return;
  if (get(activeSqueezePlayer) !== mac) return;

  const q = get(queue);
  const idx = get(queueIndex);
  if (q.length === 0) return;
  if (idx < 0 || idx >= q.length) return;

  const session: SqueezeSession = {
    mac,
    trackIds: q.map((t) => t.id),
    startIndex: idx,
    shuffle: get(shuffle),
    repeat: get(repeat),
    wasPlaying: get(isPlaying),
    savedAt: Date.now(),
  };

  try {
    localStorage.setItem(
      SESSION_KEY_PREFIX + mac,
      JSON.stringify({ v: SESSION_VERSION, ...session }),
    );
  } catch (e) {
    console.warn("[SQUEEZE] Failed to save session:", e);
  }
}

export function loadSqueezeSession(mac: string): SqueezeSession | null {
  if (typeof window === "undefined") return null;
  try {
    const raw = localStorage.getItem(SESSION_KEY_PREFIX + mac);
    if (!raw) return null;
    const parsed = JSON.parse(raw);
    if (parsed?.v !== SESSION_VERSION || parsed.mac !== mac) return null;
    return parsed as SqueezeSession;
  } catch {
    return null;
  }
}

export function clearSqueezeSession(mac: string): void {
  if (typeof window === "undefined") return;
  localStorage.removeItem(SESSION_KEY_PREFIX + mac);
}

let sessionSaveTimeout: ReturnType<typeof setTimeout> | null = null;
function scheduleSessionSave(mac: string): void {
  if (sessionSaveTimeout) clearTimeout(sessionSaveTimeout);
  sessionSaveTimeout = setTimeout(() => {
    sessionSaveTimeout = null;
    saveSqueezeSession(mac);
  }, 1500);
}

/**
 * Restore the saved session for `mac` if there is one. Safe to call
 * multiple times: subsequent calls for a MAC that's already been
 * restored in this app session are a no-op. Returns true if a
 * restore was actually attempted.
 */
export async function restoreSqueezeSessionIfAny(mac: string): Promise<boolean> {
  if (restoredMacs.has(mac)) return false;
  const session = loadSqueezeSession(mac);
  if (!session) return false;
  if (!session.trackIds || session.trackIds.length === 0) return false;

  const safeIndex = Math.min(
    Math.max(0, session.startIndex),
    session.trackIds.length - 1,
  );

  // Populate the local queue/index with the saved tracks so subsequent
  // user actions (play next album, click a specific track, etc.) keep
  // the right context — the squeeze player's internal queue is the
  // source of truth for next/prev, but the local queue drives any
  // playTrack call that goes through the squeeze branch.
  // Also keeps the queue consistent for the auto-save subscribers.
  let tracks: Track[] = [];
  try {
    const fetched = await Promise.all(
      session.trackIds.map((id) => getTrackById(id).catch(() => null)),
    );
    tracks = fetched.filter((t): t is Track => t !== null);
  } catch (e) {
    console.warn("[SQUEEZE] Track fetch during restore failed:", e);
  }
  if (tracks.length === 0) {
    // Library changed under us; nothing useful to restore.
    clearSqueezeSession(mac);
    return false;
  }

  // Suppress the auto-save subscribers for ~2s so they don't immediately
  // clobber the session we just loaded.
  suppressSaveUntil = Date.now() + 2000;

  try {
    queue.set(tracks);
    queueIndex.set(safeIndex);
    shuffle.set(session.shuffle);
    repeat.set(session.repeat);
    await squeezePlay(mac, session.trackIds, safeIndex);
    restoredMacs.add(mac);
    console.log(
      `[SQUEEZE] Restored session for ${mac}: ${tracks.length} tracks, start=${safeIndex}, shuffle=${session.shuffle}`,
    );
    return true;
  } catch (e) {
    console.warn("[SQUEEZE] Failed to restore session:", e);
    return false;
  }
}

// Auto-save: subscribe to the player-state stores that drive the
// squeeze session. Only fires when in squeeze mode and a player is
// active; the save function itself double-checks before writing.
//
// IMPORTANT: these subscribers MUST NOT be set up at module-load time.
// `squeeze.ts` and `player.ts` import each other, so during module
// evaluation the bindings from player.ts are still undefined here —
// calling .subscribe() on them would throw and brick the whole app
// (black screen). Defer all of this to a runtime init function that
// `+page.svelte` calls after both modules are fully loaded.
let persistenceInitialized = false;

export function initSqueezeSessionPersistence(): void {
  if (persistenceInitialized) return;
  persistenceInitialized = true;

  currentTrack.subscribe(() => {
    const mac = get(activeSqueezePlayer);
    if (mac && get(activeBackend) === "squeeze") scheduleSessionSave(mac);
  });
  queue.subscribe(() => {
    const mac = get(activeSqueezePlayer);
    if (mac && get(activeBackend) === "squeeze") scheduleSessionSave(mac);
  });
  queueIndex.subscribe(() => {
    const mac = get(activeSqueezePlayer);
    if (mac && get(activeBackend) === "squeeze") scheduleSessionSave(mac);
  });
  isPlaying.subscribe(() => {
    const mac = get(activeSqueezePlayer);
    if (mac && get(activeBackend) === "squeeze") scheduleSessionSave(mac);
  });

  // Auto-restore: whenever a Squeeze player gets selected, try to bring
  // back its last session. Skips MACs we've already handled this run.
  activeSqueezePlayer.subscribe((mac) => {
    if (mac) {
      void restoreSqueezeSessionIfAny(mac);
    }
  });

  // Final flush on tab/app close — localStorage is synchronous so this
  // captures whatever the debounced timer hasn't fired yet.
  if (typeof window !== "undefined") {
    window.addEventListener("beforeunload", () => {
      const mac = get(activeSqueezePlayer);
      if (mac && get(activeBackend) === "squeeze") {
        saveSqueezeSession(mac);
      }
    });
  }
}
