<script lang="ts">
    import { onMount } from "svelte";
    import { get } from "svelte/store";
    import { register, unregister } from "@tauri-apps/plugin-global-shortcut";
    import { desktopEffectsEnabled } from "$lib/application/bootstrap";
    import { togglePlay, nextTrack, previousTrack } from "$lib/stores/player";

    onMount(() => {
        if (!get(desktopEffectsEnabled)) return;
        let active = true;
        const owned = new Set<string>();
        const shortcuts = [
            ["MediaPlayPause", togglePlay],
            ["MediaTrackNext", nextTrack],
            ["MediaTrackPrevious", previousTrack],
        ] as const;
        const release = (key: string) => {
            if (owned.delete(key)) void unregister(key).catch(console.error);
        };
        const unsubscribe = desktopEffectsEnabled.subscribe(enabled => {
            if (!enabled) { active = false; owned.forEach(release); }
        });
        void (async () => {
            try {
                for (const [key, action] of shortcuts) {
                    if (!active || !get(desktopEffectsEnabled)) break;
                    await register(key, event => {
                        if (active && get(desktopEffectsEnabled) && event.state === "Pressed") void action();
                    });
                    owned.add(key);
                    if (!active || !get(desktopEffectsEnabled)) release(key);
                }
            } catch (error) {
                owned.forEach(release);
                console.error("Failed to register global shortcuts:", error);
            }
        })();
        return () => { active = false; unsubscribe(); owned.forEach(release); };
    });
</script>
