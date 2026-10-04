<script lang="ts">
    import Navigation from "./presentation/Navigation.svelte";
    import { desktopEffectsEnabled } from "$lib/application/bootstrap";
    import { mobileSearchOpen } from "$lib/stores/mobile";
    import {
        currentView,
        goToHome,
        goToTracks,
        goToAlbums,
        goToArtists,
        goToPlaylists,
        goToPlugins,
    } from "$lib/stores/view";
    import { clearSearch } from "$lib/stores/search";
    import { currentTrack } from "$lib/stores/player";
    import { uiSlotManager } from "$lib/plugins/ui-slots";
    import { pluginDrawerOpen } from "$lib/stores/plugin-drawer";
    import { onMount } from "svelte";

    type MobileTab = "home" | "library" | "plugins";

    let pluginSlot: HTMLDivElement;

    // Track which library sub-view was last active
    let lastLibraryView: "tracks" | "albums" | "artists" | "playlists" =
        "tracks";

    $: {
        const type = $currentView.type;
        if (
            type === "tracks" ||
            type === "albums" ||
            type === "artists" ||
            type === "playlists"
        ) {
            lastLibraryView = type as typeof lastLibraryView;
        }
    }

    // Derive active tab from current state
    $: activeTab = deriveActiveTab($currentView.type);

    function deriveActiveTab(viewType: string): MobileTab {
        if (viewType === "home") return "home";
        if (viewType === "plugins" || viewType === "settings") return "plugins";
        return "library";
    }

    function handleTabClick(tab: MobileTab) {
        if (!$desktopEffectsEnabled) return;
        // Close search when switching tabs
        mobileSearchOpen.set(false);
        clearSearch();

        switch (tab) {
            case "home":
                goToHome();
                break;
            case "library":
                // Return to last active library sub-view
                switch (lastLibraryView) {
                    case "albums":
                        goToAlbums();
                        break;
                    case "artists":
                        goToArtists();
                        break;
                    case "playlists":
                        goToPlaylists();
                        break;
                    default:
                        goToTracks();
                        break;
                }
                break;
            case "plugins":
                goToPlugins();
                break;
        }
    }

    onMount(() => {
        if (!$desktopEffectsEnabled) return;
        if (pluginSlot) {
            uiSlotManager.registerContainer("mobile:bottomnav", pluginSlot);
        }
        return () => {
            uiSlotManager.unregisterContainer("mobile:bottomnav");
        };
    });

    $: navigationSections = [{ id: "tabs", label: "", rows: [
        { id: "home", label: "Home", icon: "home" as const, active: activeTab === "home", availability: { enabled: true as const } },
        { id: "library", label: "Library", icon: "library" as const, active: activeTab === "library", availability: { enabled: true as const } },
        { id: "plugins", label: "Plugins", icon: "plugins" as const, active: activeTab === "plugins", availability: { enabled: true as const } },
        { id: "actions", label: "Actions", icon: "actions" as const, active: $pluginDrawerOpen, availability: { enabled: true as const } }
    ] }];
    function navigateShared(id: string): void {
        if (!$desktopEffectsEnabled) return;
        if (id === "actions") pluginDrawerOpen.set(true);
        else handleTabClick(id as MobileTab);
    }
</script>
{#if $desktopEffectsEnabled}

<Navigation variant="compact" sections={navigationSections} hasPlayer={!!$currentTrack} onNavigate={navigateShared}>
    <div slot="extensions" class="plugin-slot" bind:this={pluginSlot}></div>
</Navigation>

{/if}
