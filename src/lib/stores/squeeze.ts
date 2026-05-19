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
import { getTrackByIdSync } from '$lib/stores/library';

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

        const elapsed = info.elapsed_ms / 1000;
        currentTime.set(elapsed);

        if (info.current_track) {
            const trackDur = info.current_track.duration;
            if (get(duration) !== trackDur) duration.set(trackDur);

            const currentObj = get(currentTrack);
            if (!currentObj || currentObj.id !== info.current_track.id) {
                const localTrack = getTrackByIdSync(info.current_track.id);
                if (localTrack) {
                    currentTrack.set({
                        ...localTrack,
                        track_cover: getTrackCoverSrc(localTrack),
                    } as any);
                } else {
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
