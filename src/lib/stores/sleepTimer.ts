import { derived, get, writable } from 'svelte/store';
import { pause, isPlaying } from './player';
import { addToast } from './toast';
import type { Track } from '$lib/api/tauri';

const STORAGE_KEY = 'audion_sleep_timer';
const TICK_INTERVAL_MS = 1000;

export const SLEEP_TIMER_PRESETS = [15, 30, 45, 60] as const;

export type TriggerMode = 'time' | 'track_end' | 'album_end';

interface SleepTimerState {
    endsAt: number | null;
    lastDurationMinutes: number;
    triggerMode: TriggerMode;
    armedAlbumId: number | null;
}

function getDefaultState(): SleepTimerState {
    return {
        endsAt: null,
        lastDurationMinutes: 30,
        triggerMode: 'time',
        armedAlbumId: null,
    };
}

function loadState(): SleepTimerState {
    if (typeof window === 'undefined') return getDefaultState();

    try {
        const raw = localStorage.getItem(STORAGE_KEY);
        if (!raw) return getDefaultState();

        const parsed = JSON.parse(raw) as Partial<SleepTimerState>;

        // Validate triggerMode — graceful v1→v2 default
        const validModes: TriggerMode[] = ['time', 'track_end', 'album_end'];
        const triggerMode: TriggerMode = validModes.includes(
            parsed.triggerMode as TriggerMode,
        )
            ? (parsed.triggerMode as TriggerMode)
            : 'time';

        return {
            endsAt:
                typeof parsed.endsAt === 'number' && Number.isFinite(parsed.endsAt)
                    ? parsed.endsAt
                    : null,
            lastDurationMinutes:
                typeof parsed.lastDurationMinutes === 'number' &&
                Number.isFinite(parsed.lastDurationMinutes) &&
                parsed.lastDurationMinutes > 0
                    ? parsed.lastDurationMinutes
                    : 30,
            triggerMode,
            armedAlbumId:
                typeof parsed.armedAlbumId === 'number' &&
                Number.isFinite(parsed.armedAlbumId)
                    ? parsed.armedAlbumId
                    : null,
        };
    } catch (error) {
        console.error('[SleepTimer] Failed to load state:', error);
        return getDefaultState();
    }
}

function saveState(state: SleepTimerState): void {
    if (typeof window === 'undefined') return;

    try {
        localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
    } catch (error) {
        console.error('[SleepTimer] Failed to save state:', error);
    }
}

const initialState = loadState();
const endsAt = writable<number | null>(initialState.endsAt);
const lastDurationMinutes = writable<number>(initialState.lastDurationMinutes);
const triggerMode = writable<TriggerMode>(initialState.triggerMode);
const armedAlbumId = writable<number | null>(initialState.armedAlbumId);
const now = writable<number>(Date.now());

let tickHandle: ReturnType<typeof setInterval> | null = null;
let expiring = false;

function persist(): void {
    saveState({
        endsAt: get(endsAt),
        lastDurationMinutes: get(lastDurationMinutes),
        triggerMode: get(triggerMode),
        armedAlbumId: get(armedAlbumId),
    });
}

async function expireTimer(): Promise<void> {
    if (expiring) return;

    expiring = true;
    stopSleepTimer(false);

    try {
        if (get(isPlaying)) {
            await pause();
            addToast('Sleep timer expired — playback paused', 'info');
        } else {
            addToast('Sleep timer expired', 'info');
        }
    } catch (error) {
        console.error('[SleepTimer] Failed to pause playback on expiry:', error);
    } finally {
        expiring = false;
    }
}

function startTicker(): void {
    if (tickHandle) return;

    tickHandle = setInterval(() => {
        const end = get(endsAt);
        if (!end) {
            stopTicker();
            return;
        }

        const current = Date.now();
        now.set(current);

        if (current >= end) {
            void expireTimer();
        }
    }, TICK_INTERVAL_MS);
}

function stopTicker(): void {
    if (tickHandle) {
        clearInterval(tickHandle);
        tickHandle = null;
    }
}

export const sleepTimerEndsAt = {
    subscribe: endsAt.subscribe,
};

export const sleepTimerLastDurationMinutes = {
    subscribe: lastDurationMinutes.subscribe,
};

export const sleepTimerActive = derived(endsAt, ($endsAt) => $endsAt !== null);

export const sleepTimerRemainingMs = derived(
    [endsAt, now],
    ([$endsAt, $now]) => {
        if (!$endsAt) return 0;
        return Math.max(0, $endsAt - $now);
    }
);

export function startSleepTimer(minutes: number): void {
    if (!Number.isFinite(minutes) || minutes <= 0) return;

    const normalizedMinutes = Math.round(minutes);
    const nextEndsAt = Date.now() + normalizedMinutes * 60_000;

    endsAt.set(nextEndsAt);
    lastDurationMinutes.set(normalizedMinutes);
    triggerMode.set('time');
    now.set(Date.now());
    startTicker();
    persist();
}

export function stopSleepTimer(showToast = true): void {
    const wasActive = get(endsAt) !== null;
    endsAt.set(null);
    triggerMode.set('time');
    armedAlbumId.set(null);
    now.set(Date.now());
    stopTicker();
    persist();

    if (showToast && wasActive) {
        addToast('Sleep timer cancelled', 'info');
    }
}

// Re-arm a track-end timer. Clears any active time countdown, sets
// triggerMode to 'track_end', and waits for handleTrackEnd/handleGaplessAdvance.
export function armTrackEndTimer(): void {
    stopTicker();
    endsAt.set(null);
    triggerMode.set('track_end');
    armedAlbumId.set(null);
    now.set(Date.now());
    persist();
}

// Re-arm an album-end timer. Clears any active time countdown, stores the
// target album ID, and fires at the first track-transition where the next
// track's album_id differs from armedAlbumId.
export function armAlbumEndTimer(albumId: number | null): void {
    stopTicker();
    endsAt.set(null);
    triggerMode.set('album_end');
    armedAlbumId.set(albumId);
    now.set(Date.now());
    persist();
}

// Returns true when the timer is in track_end or album_end mode.
// Used to prevent re-arming a time-based timer while waiting for
// a track/album transition.
export function isTimerModeTrackOrAlbumEnd(): boolean {
    return get(triggerMode) !== 'time';
}

// Returns true ONLY when the timer is in album_end mode.
// Used by toggleShuffle() — shuffle doesn't affect track_end,
// only album_end detection becomes unreliable when shuffling.
export function isTimerModeAlbumEnd(): boolean {
    return get(triggerMode) === 'album_end';
}

export function restartSleepTimerWithLastDuration(): void {
    startSleepTimer(get(lastDurationMinutes));
}

// Shared handler called by BOTH handleTrackEnd() and handleGaplessAdvance()
// in player.ts. Returns true if the timer fired (caller must return early
// without advancing the queue).
//
// nextAlbumId is the album_id of the track that would play after the
// current transition. Pass null if there is no next track (queue end).
// The caller computes this via _advanceQueueIndex(true) dry-run to avoid
// a circular dependency on player.ts internals.
export function handleSleepTimerCheck(
    _prevTrack: Track | null,
    nextAlbumId: number | null,
): boolean {
    const mode = get(triggerMode);

    // Time mode: ticker handles expiry, nothing to do here
    if (mode === 'time') return false;

    // track_end: fire immediately — pause + toast + stop
    if (mode === 'track_end') {
        stopTicker();
        endsAt.set(null);
        triggerMode.set('time');
        persist();
        void pause();
        addToast('Sleep timer: end of track reached', 'info');
        return true;
    }

    // album_end: compare next track's album_id with armedAlbumId
    if (mode === 'album_end') {
        const armedId = get(armedAlbumId);

        // null album_id → treat as track_end and fire immediately
        if (armedId === null) {
            stopTicker();
            endsAt.set(null);
            triggerMode.set('time');
            armedAlbumId.set(null);
            persist();
            void pause();
            addToast('Sleep timer: end of track reached', 'info');
            return true;
        }

        // Queue end (nextAlbumId is null) or different album → fire
        if (nextAlbumId === null || nextAlbumId !== armedId) {
            stopTicker();
            endsAt.set(null);
            triggerMode.set('time');
            armedAlbumId.set(null);
            persist();
            void pause();
            addToast('Sleep timer: end of album reached', 'info');
            return true;
        }

        // Same album — continue playing, don't disarm
    }

    return false;
}

// Replay the stored state if the timer was still active on page load
if (initialState.endsAt && initialState.endsAt > Date.now()) {
    startTicker();
} else if (initialState.endsAt && initialState.endsAt <= Date.now()) {
    endsAt.set(null);
    triggerMode.set('time');
    armedAlbumId.set(null);
    persist();
}

// Export the writable stores (read-only subscriptions) for use in
// FullScreenPlayer mode selectors and other UI components.
export const sleepTimerTriggerMode = {
    subscribe: triggerMode.subscribe,
};

export const sleepTimerArmedAlbumId = {
    subscribe: armedAlbumId.subscribe,
};
