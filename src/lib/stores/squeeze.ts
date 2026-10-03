import { writable, get } from "svelte/store";
import {
  squeezeGetPlayerState,
  squeezeGetPlayers,
  squeezeStop,
  squeezeIsRunning,
  squeezeDisconnectPlayer,
  getTrackCoverSrc,
  getTrackById,
  type SqueezePlayerInfo,
} from "$lib/api/tauri";
import { playbackStateWriter } from "$lib/stores/playback-state";
const { currentTrack, isPlaying, currentTime, duration, volume, activeBackend, shuffle, repeat, queueIndex } = playbackStateWriter;
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

type SqueezePollOwner = Readonly<{ mac: string; epoch: number }>;

let squeezePollEpoch = 0;
let pollInterval: ReturnType<typeof setInterval> | null = null;
let volumeCooldownUntil = 0;

let discoveryInterval: ReturnType<typeof setInterval> | null = null;
let discoveryEpoch = 0;
let discoveryStarting: number | null = null;

/**
 * Idempotent. Starts a 1 Hz poll that keeps `discoveredSqueezePlayers`
 * in sync with the backend.
 *
 * The poll NEVER boots the Squeeze server and NEVER auto-selects a
 * player: a Squeeze connection is established only by explicit user
 * action (Start server / Control / Play Here). Auto-starting the server
 * on app boot made the Eversolo auto-reconnect to a server the user
 * never asked to run, and auto-selection silently re-established a
 * "connection" the user had cut — both surfaced as broken playback
 * after reopening Audion.
 *
 * The poll keeps running as long as the Squeeze server is up; it stops
 * when `stopGlobalSqueezeDiscovery()` is called (e.g. when the user
 * hits the Stop button in the Connect panel).
 */
export async function startGlobalSqueezeDiscovery(): Promise<void> {
  if (discoveryInterval || discoveryStarting !== null) return;
  const epoch = discoveryEpoch;
  discoveryStarting = epoch;
  try {
    // If the server isn't running there is nothing to discover. Never
    // boot it from here — the Connect panel's Start button is the only
    // entry point for bringing the server up.
    if (!(await squeezeIsRunning().catch(() => false))) return;
    if (epoch !== discoveryEpoch) return;
    // First tick immediately so the UI doesn't have to wait a full
    // second for the initial state.
    await pollSqueezePlayersForEpoch(epoch);
    if (epoch !== discoveryEpoch) return;
    discoveryInterval = setInterval(() => {
      void pollSqueezePlayersForEpoch(epoch);
    }, 1000);
  } finally {
    if (discoveryStarting === epoch) discoveryStarting = null;
  }
}

export function stopGlobalSqueezeDiscovery(): void {
  // Invalidate pending startup checks and player requests before clearing UI.
  discoveryEpoch += 1;
  discoveryStarting = null;
  if (discoveryInterval) {
    clearInterval(discoveryInterval);
    discoveryInterval = null;
  }
  discoveredSqueezePlayers.set([]);
}

/**
 * Disconnect a Squeeze player for real: the backend drops the TCP
 * connection, removes the player entry, and clears its queued stream,
 * so the device is actually cut off — not just hidden in the UI.
 * Playback state is cleared locally as well.
 */
export function disconnectSqueezePlayer(mac: string): void {
  if (get(activeSqueezePlayer) === mac) clearSqueezeTarget();
  squeezeDisconnectPlayer(mac).catch((e) =>
    console.warn("[SQUEEZE] Disconnect failed:", e),
  );
}

/** Release Squeeze control after a device disconnect or successful server stop. */
export function clearSqueezeTarget(): void {
  activeSqueezePlayer.set(null);
  // A stale Squeeze selection must not erase the cloud/local owner's state.
  if (get(activeBackend) !== "squeeze") return;
  activeBackend.set("none");
  isPlaying.set(false);
  currentTrack.set(null);
  currentTime.set(0);
  duration.set(0);
}

/** Test seam: trigger a single discovery tick without waiting for the 1 s interval. */
export async function pollSqueezePlayersOnce(): Promise<void> {
  await pollSqueezePlayersForEpoch(discoveryEpoch);
}

async function pollSqueezePlayersForEpoch(epoch: number): Promise<void> {
  try {
    const players = await squeezeGetPlayers();
    if (epoch !== discoveryEpoch) return;
    discoveredSqueezePlayers.set(players ?? []);
    // No implicit selection: a player only becomes the active target
    // when the user explicitly controls it (Control / Play Here).
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

export function invalidateSqueezePollOwnership(): number {
  squeezePollEpoch += 1;
  return squeezePollEpoch;
}

function captureSqueezePollOwner(mac: string): SqueezePollOwner {
  return Object.freeze({ mac, epoch: squeezePollEpoch });
}

function ownsSqueezePoll(owner: SqueezePollOwner): boolean {
  return (
    owner.epoch === squeezePollEpoch &&
    get(activeSqueezePlayer) === owner.mac &&
    get(activeBackend) === "squeeze"
  );
}

/** Capture the current target generation for an asynchronous Squeeze command. */
export function captureSqueezeTargetOwnership(mac: string): () => boolean {
  const owner = captureSqueezePollOwner(mac);
  return () => ownsSqueezePoll(owner);
}

export function initializeSqueeze(): () => void {
    const unsubscribe = activeSqueezePlayer.subscribe((mac) => {
        invalidateSqueezePollOwnership();
        if (pollInterval) {
            clearInterval(pollInterval);
            pollInterval = null;
        }
        if (mac) {
            void pollSqueezeState(captureSqueezePollOwner(mac));
            pollInterval = setInterval(() => {
                void pollSqueezeState(captureSqueezePollOwner(mac));
            }, 500);
        }
        else {
            squeezePlayerState.set(null);
        }
    });
    return () => {
        unsubscribe();
        invalidateSqueezePollOwnership();
        if (pollInterval) clearInterval(pollInterval);
        pollInterval = null;
        stopGlobalSqueezeDiscovery();
    };
}

type ObservationSink = () => (apply: () => Promise<void>) => Promise<void>;
let observationSink: ObservationSink | undefined;
export function bindSqueezeObservations(sink: ObservationSink | undefined): void { observationSink = sink; }
async function pollSqueezeState(owner: SqueezePollOwner) {
  if (!ownsSqueezePoll(owner)) return;
  const publish = observationSink?.();
  try {
    const info = await squeezeGetPlayerState(owner.mac);
    const apply = async () => {
      if (!ownsSqueezePoll(owner)) return;
      await applySqueezeState(owner, info);
    };
    if (publish) await publish(apply); else await apply();
  } catch {
    // Player may have disconnected; ownership checks prevent stale publication.
  }
}
async function applySqueezeState(owner: SqueezePollOwner, info: SqueezePlayerInfo): Promise<void> {
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
      const sameTrack = currentObj?.id === info.current_track.id && (info.current_queue_index == null || get(queueIndex) === info.current_queue_index);

      // If track not in memory cache, fetch from backend and cache it
      if (!localTrack && !sameTrack) {
        try {
          const fetched = await getTrackById(info.current_track.id);
          if (!ownsSqueezePoll(owner)) return;
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
        if (!sameTrack && prevTrack) {
          const durationPlayed = Math.floor(prevElapsed);
          if (durationPlayed > 5) {
            if (ownsSqueezePoll(owner)) {
              void recordTrackPlay(
                prevTrack.id,
                prevTrack.album_id ?? null,
                durationPlayed,
              );
              incrementPlayCount(prevTrack.id);
            }
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

    if (info.current_queue_index != null) queueIndex.set(info.current_queue_index);

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
        const watchdogOwner = owner;
        const publishWatchdog = observationSink?.();
        pendingSqueezeForcedEnd = setTimeout(() => {
          pendingSqueezeForcedEnd = null;
          const finish = async () => {
          if (!ownsSqueezePoll(watchdogOwner)) return;
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
            // Claim this completion even if stopping fails: its play was already recorded.
            lastForcedEndTrackId = trackId;
            await squeezeStop(watchdogOwner.mac);
          }
          };
          void (publishWatchdog ? publishWatchdog(finish) : finish()).catch(console.error);
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
}

/** Activate a player as the Squeeze control target. */
export function commitSqueezeTarget(mac: string): void {
  activeRemoteDevice.set(null);
  activeBackend.set("squeeze");
  activeSqueezePlayer.set(mac);
}

/** Test-only reset seam for module-level polling resources. */
export function resetSqueezePollingForTests(): void {
  invalidateSqueezePollOwnership();
  if (pollInterval) {
    clearInterval(pollInterval);
    pollInterval = null;
  }
  clearPendingSqueezeForcedEnd();
  lastForcedEndTrackId = null;
}


let selectTarget: ((mac: string) => Promise<void>) | undefined;
export function bindSqueezeSelection(select: typeof selectTarget): void { selectTarget = select; }
/** Desktop UI selection is serialized once desktop bootstrap registers its owner. */
export function activateSqueezeTarget(mac: string): void | Promise<void> {
  if (selectTarget) return selectTarget(mac);
  commitSqueezeTarget(mac);
}
