import { writable, get } from 'svelte/store';
import {
    squeezeGetPlayerState,
    getTrackCoverSrc,
    type SqueezePlayerInfo,
} from '$lib/api/tauri';
import {
    currentTrack,
    isPlaying,
    currentTime,
    duration,
    volume,
    activeBackend,
    shuffle,
    repeat,
} from '$lib/stores/player';
import { getTrackByIdSync, incrementPlayCount } from '$lib/stores/library';
import { recordTrackPlay } from '$lib/stores/activity';

export const activeSqueezePlayer = writable<string | null>(null);
export const squeezePlayerState = writable<SqueezePlayerInfo | null>(null);

let pollInterval: ReturnType<typeof setInterval> | null = null;
let volumeCooldownUntil = 0;

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
    if (get(activeBackend) !== 'squeeze') return;

    try {
        const info = await squeezeGetPlayerState(mac);
        squeezePlayerState.set(info);

        const playing = info.state === 'Playing';
        if (get(isPlaying) !== playing) isPlaying.set(playing);

        const prevTrack = get(currentTrack);
        const prevElapsed = get(currentTime);
        const elapsed = info.elapsed_ms / 1000;
        currentTime.set(elapsed);

        if (info.current_track) {
            const trackDur = info.current_track.duration;
            if (get(duration) !== trackDur) duration.set(trackDur);

            const currentObj = get(currentTrack);
            const localTrack = getTrackByIdSync(info.current_track.id);
            const sameTrack = currentObj?.id === info.current_track.id;
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

        const rep = info.repeat === 'Off' ? 'none' : info.repeat === 'One' ? 'one' : 'all';
        if (get(repeat) !== rep) repeat.set(rep);
    } catch {
        // Player may have disconnected
    }
}
