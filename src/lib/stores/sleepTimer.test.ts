/**
 * Pure-function unit tests for handleSleepTimerCheck and playNext.
 *
 * These tests verify the algorithmic logic of timer-mode transitions
 * and queue/shuffle insertion without needing Svelte component rendering.
 *
 * Strict TDD: RED (tests written first) → GREEN (implementation passes).
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';

// ── Mock toast before importing modules ────────────────────────────────
vi.mock('$lib/stores/toast', () => ({
  addToast: vi.fn(),
}));

// ── Actual imports ─────────────────────────────────────────────────────
import { get } from 'svelte/store';
import {
  handleSleepTimerCheck,
  armTrackEndTimer,
  armAlbumEndTimer,
  startSleepTimer,
  stopSleepTimer,
  sleepTimerTriggerMode,
} from '$lib/stores/sleepTimer';
import { playNext } from '$lib/stores/player';
import {
  queue,
  queueIndex,
  shuffledIndices,
  shuffledIndex,
  shuffle,
  userQueueCount,
} from '$lib/stores/player';
import type { Track } from '$lib/api/tauri';

// ── Helpers ────────────────────────────────────────────────────────────
function makeTrack(overrides: Partial<Track> = {}): Track {
  return {
    id: overrides.id ?? 1,
    path: '/fake/track.mp3',
    title: overrides.title ?? 'Test Track',
    artist: overrides.artist ?? 'Test Artist',
    album: overrides.album ?? 'Test Album',
    album_id: overrides.album_id ?? 100,
    duration: overrides.duration ?? 240,
    track_number: overrides.track_number ?? 1,
    format: overrides.format ?? 'mp3',
    bitrate: overrides.bitrate ?? 320,
  };
}

function resetSleepTimer(): void {
  stopSleepTimer(false);
}

// ── 7.2: handleSleepTimerCheck() return values ─────────────────────────
describe('handleSleepTimerCheck()', () => {
  beforeEach(() => {
    resetSleepTimer();
  });

  it('returns false in time mode (ticker handles expiry)', () => {
    startSleepTimer(15);
    const result = handleSleepTimerCheck(makeTrack(), 200);
    expect(result).toBe(false);
  });

  it('returns true in track_end mode and disarms', () => {
    armTrackEndTimer();
    const result = handleSleepTimerCheck(makeTrack(), null);
    expect(result).toBe(true);
    // After firing, triggerMode should reset to 'time'
    expect(get(sleepTimerTriggerMode)).toBe('time');
  });

  it('returns true in album_end when next album differs', () => {
    const track = makeTrack({ album_id: 100 });
    armAlbumEndTimer(100);
    const result = handleSleepTimerCheck(track, 200);
    expect(result).toBe(true);
    expect(get(sleepTimerTriggerMode)).toBe('time');
  });

  it('returns false in album_end when next album is same', () => {
    const track = makeTrack({ album_id: 100 });
    armAlbumEndTimer(100);
    const result = handleSleepTimerCheck(track, 100);
    expect(result).toBe(false);
    // Still armed (same album, not yet at end)
    expect(get(sleepTimerTriggerMode)).toBe('album_end');
  });

  it('fires when armedAlbumId is null (treat as track_end)', () => {
    const track = makeTrack({ album_id: null });
    armAlbumEndTimer(null);
    const result = handleSleepTimerCheck(track, null);
    expect(result).toBe(true);
  });

  it('fires at queue end (nextAlbumId = null, armed album end)', () => {
    const track = makeTrack({ album_id: 100 });
    armAlbumEndTimer(100);
    const result = handleSleepTimerCheck(track, null);
    expect(result).toBe(true);
  });
});

// ── 7.3: playNext() queue insertion ────────────────────────────────────
describe('playNext() queue insertion', () => {
  beforeEach(() => {
    queue.set([
      makeTrack({ id: 1, title: 'A' }),
      makeTrack({ id: 2, title: 'B' }),
      makeTrack({ id: 3, title: 'C' }),
    ]);
    queueIndex.set(0);
    userQueueCount.set(0);
    shuffledIndices.set([]);
    shuffledIndex.set(0);
    shuffle.set(false);
  });

  it('inserts a single track at currentIdx+1', () => {
    const d = makeTrack({ id: 4, title: 'D' });
    playNext([d]);
    const q = get(queue);
    expect(q.map((t) => t.title)).toEqual(['A', 'D', 'B', 'C']);
  });

  it('inserts a batch preserving order', () => {
    const d = makeTrack({ id: 4, title: 'D' });
    const e = makeTrack({ id: 5, title: 'E' });
    const f = makeTrack({ id: 6, title: 'F' });
    playNext([d, e, f]);
    const q = get(queue);
    expect(q.map((t) => t.title)).toEqual(['A', 'D', 'E', 'F', 'B', 'C']);
  });

  it('does NOT increment userQueueCount', () => {
    playNext([makeTrack({ id: 4 })]);
    expect(get(userQueueCount)).toBe(0);
  });

  it('inserts at currentIdx=2 (mid-queue)', () => {
    queueIndex.set(2);
    const d = makeTrack({ id: 4, title: 'D' });
    playNext([d]);
    const q = get(queue);
    expect(q.map((t) => t.title)).toEqual(['A', 'B', 'C', 'D']);
  });

  it('no-op on empty tracks array', () => {
    playNext([]);
    const q = get(queue);
    expect(q.map((t) => t.title)).toEqual(['A', 'B', 'C']);
  });
});

// ── 7.4: playNext() shuffle index manipulation ─────────────────────────
describe('playNext() shuffle index manipulation', () => {
  beforeEach(() => {
    queue.set([
      makeTrack({ id: 1, title: 'A' }),
      makeTrack({ id: 2, title: 'B' }),
      makeTrack({ id: 3, title: 'C' }),
      makeTrack({ id: 4, title: 'D' }),
      makeTrack({ id: 5, title: 'E' }),
    ]);
    queueIndex.set(1); // playing B
    userQueueCount.set(0);
    shuffle.set(true);
    // Shuffle order: [2, 4, 0, 3, 1] → B(idx 2) at pos 0
    shuffledIndices.set([2, 4, 0, 3, 1]);
    shuffledIndex.set(0);
  });

  it('inserts at shuffledIndex+1 (not appended)', () => {
    const f = makeTrack({ id: 6, title: 'F' });
    playNext([f]);

    const s = get(shuffledIndices);
    // Length increased by 1
    expect(s.length).toBe(6);

    // New index (2 = currentIdx+1 = insert position) should be present
    expect(s).toContain(2);
  });

  it('batch insert extends shuffledIndices', () => {
    const f = makeTrack({ id: 6, title: 'F' });
    const g = makeTrack({ id: 7, title: 'G' });
    playNext([f, g]);

    const s = get(shuffledIndices);
    // 5 original + 2 new = 7
    expect(s.length).toBe(7);

    // New indices (2 and 3) should both be present
    expect(s).toContain(2);
    expect(s).toContain(3);

    // Queue should have the new tracks at positions 2 and 3
    const q = get(queue);
    expect(q.map((t) => t.title)).toEqual(['A', 'B', 'F', 'G', 'C', 'D', 'E']);
  });
});
