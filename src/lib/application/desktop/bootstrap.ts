import { hostname } from "@tauri-apps/plugin-os";
import { get } from "svelte/store";
import { createDesktopAdapter } from "./adapter";
import { connectHostBridge, createDesktopLibraryAccess } from "./bridge";
import { invoke } from "@tauri-apps/api/core";
import { setMigrationStatus, type ApplicationHandle } from "../bootstrap";
import { initAudioBackend, cleanupPlayer } from "./player-runtime";
import { appSettings } from "$lib/stores/settings";
import { equalizer } from "$lib/stores/equalizer";
import { initializeSleepTimer } from "$lib/stores/sleepTimer";
import { initializeSqueeze, startGlobalSqueezeDiscovery } from "$lib/stores/squeeze";
import { wsStore } from "$lib/stores/websocket";
import { initializeFromPersistedState, setupAutoSave } from "$lib/stores/persist";
import { initializePinned, pinnedItems } from "$lib/stores/pinned";
import { initializeCustomArtwork } from "$lib/stores/customArtwork";
import { initializePlaylistCovers } from "$lib/stores/playlistCovers";
import { initializeLyricsPreferences, destroyLyricsSync } from "$lib/stores/lyrics";
import { initDiscordPresence, disposeDiscordPresence } from "$lib/stores/discordPresence";
import { initSync, destroySync } from "$lib/stores/sync";
import { loadLikedTracks } from "$lib/stores/liked";
import { loadLikedAlbums } from "$lib/stores/liked-albums";
import { loadListenLaterAlbums } from "$lib/stores/listen-later";
import { loadLibrary, loadPlaylists, refreshLibrarySilently } from "$lib/stores/library";
import { pluginStore } from "$lib/stores/plugin-store";
import { migrateCoversToFiles, startWatcher, squeezeStartServer } from "$lib/api/tauri";
import { listen } from "@tauri-apps/api/event";
let adapter: ReturnType<typeof createDesktopAdapter> | undefined;
let hostBridge: ReturnType<typeof connectHostBridge> | undefined;
/** One bridge per shared adapter; settings opt-in never creates another lane. */
export async function prepareHostBridge(): Promise<void> {
    if (!adapter || stopped) throw new Error("Desktop is not ready");
    hostBridge ??= connectHostBridge(adapter.port, adapter.attachAuthority, adapter.coordinator).catch(error => { hostBridge = undefined; throw error; });
    await hostBridge;
}
export async function releaseHostBridge(): Promise<void> {
    const current = hostBridge;
    hostBridge = undefined;
    if (current) await (await current).dispose();
}
let owners = 0;
let startup: Promise<void> | undefined;
let stops: (() => void)[] = [];
let pluginLoad: number | undefined;
let pluginStartup: Promise<void> | undefined;
let shutdown: Promise<void> = Promise.resolve();
let stopped = false;
let migrationTimer: ReturnType<typeof setTimeout> | undefined;
async function start(): Promise<void> {
    stopped = false;
    await appSettings.initialize();
    equalizer.initialize();
    stops.push(initializePinned(), initializeCustomArtwork(), initializePlaylistCovers(), initializeLyricsPreferences());
    initializeFromPersistedState();
    const pcName = await hostname().catch(() => null);
    adapter = createDesktopAdapter(createDesktopLibraryAccess(() => [...get(pinnedItems).albums]), pcName);
    stops.push(setupAutoSave(), initializeSleepTimer(() => adapter!.pauseForTimer()), initializeSqueeze(), wsStore.initialize());
    await initAudioBackend();
    initDiscordPresence();
    stops.push(disposeDiscordPresence, destroySync, destroyLyricsSync);
    if (get(appSettings).autoScanLibrary)
        void startWatcher().catch(console.warn);
    stops.push(await listen("watcher-files-changed", () => { void refreshLibrarySilently(); }));
    await Promise.all([loadLikedTracks(), loadLikedAlbums(), loadListenLaterAlbums(), initSync()]);
    if (localStorage.getItem("covers_migrated") !== "true") {
        setMigrationStatus("Migrating cover images to file storage...");
        try {
            const result = await migrateCoversToFiles();
            if (result.errors.length === 0) {
                localStorage.setItem("covers_migrated", "true");
                setMigrationStatus(`Successfully migrated ${result.tracks_migrated} track covers and ${result.albums_migrated} album covers`);
            }
            else {
                setMigrationStatus(`Migration completed with ${result.errors.length} errors. Check console for details.`);
            }
            migrationTimer = setTimeout(() => setMigrationStatus(null), result.errors.length === 0 ? 3000 : 5000);
        }
        catch (error) {
            console.error("[Migration] Failed:", error);
            setMigrationStatus("Migration failed. Please try again from settings.");
            migrationTimer = setTimeout(() => setMigrationStatus(null), 5000);
        }
    }
    try {
        await Promise.all([loadLibrary(), loadPlaylists()]);
    }
    catch (error) {
        console.error("Failed to load library:", error);
    }
    try {
        await squeezeStartServer();
        await startGlobalSqueezeDiscovery();
    }
    catch (error) {
        console.warn("[SQUEEZE] Auto-start failed:", error);
    }
    pluginLoad = requestIdleCallback(() => {
        if (!stopped)
            pluginStartup = pluginStore.init().catch(console.error);
    });
    // A WebView reload leaves native listener state alive but invalidates its
    // lease. Fresh app startup is Off and does not touch protected storage.
    try {
        const status = await invoke<{ enabled: boolean }>("control_host_enable", { request: { action: "status" } });
        if (status?.enabled) await prepareHostBridge();
    } catch (error) { console.warn("Controller hosting unavailable", error); }
}
async function stop(): Promise<void> {
    stopped = true;
    if (migrationTimer)
        clearTimeout(migrationTimer);
    setMigrationStatus(null);
    if (pluginLoad !== undefined)
        cancelIdleCallback(pluginLoad);
    try { await releaseHostBridge(); } catch (error) { console.warn("Controller bridge release unavailable", error); }
    await pluginStartup;
    pluginStartup = undefined;
    await adapter?.dispose();
    adapter = undefined;
    stops.splice(0).reverse().forEach(dispose => dispose());
    cleanupPlayer();
    await pluginStore.dispose();
    startup = undefined;
}
/** Shared desktop resources remain alive until their last handle is released. */
export async function bootstrapDesktop(): Promise<ApplicationHandle> {
    // Reserve synchronously: awaiting even a resolved promise must not open
    // a gap where the previous handle can become the last owner.
    owners += 1;
    try {
        await shutdown;
        startup ??= start();
        await startup;
    }
    catch (error) {
        if (--owners === 0) {
            // Publish the teardown barrier before any cleanup callback can re-enter.
            shutdown = shutdown.then(stop);
            await shutdown;
        }
        throw error;
    }
    let disposed = false;
    return {
        port: adapter!.port,
        async dispose() {
            if (disposed)
                return;
            disposed = true;
            if (--owners === 0) {
                // Publish the teardown barrier before any cleanup callback can re-enter.
                shutdown = shutdown.then(stop);
                await shutdown;
            }
        },
    };
}
