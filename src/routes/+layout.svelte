<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { get } from "svelte/store";
  import { invoke } from "@tauri-apps/api/core";
  import { applicationMode, bootstrapApplication, defaultBootstrapLoaders, desktopEffectsEnabled, migrationStatus, type ApplicationHandle } from "$lib/application/bootstrap";
  import type { ApplicationMode } from "$lib/application/types";
  import { theme } from "$lib/stores/theme";
  import { isAndroid, isTauri, initPlatformDetection } from "$lib/api/tauri";
  import { initMobileDetection, isMobile, mobileSearchOpen } from "$lib/stores/mobile";
  import { handleControllerBack } from "$lib/application/controller-ui";
  import { lyricsVisible } from "$lib/stores/lyrics";
  import { isMobileSidebarOpen } from "$lib/stores/mobile";
  import { goBack, navigationHistory } from "$lib/stores/view";
  import { isFullScreen, isQueueVisible, contextMenu, isMiniPlayer } from "$lib/stores/ui";
  import ConfirmDialog from "$lib/components/ConfirmDialog.svelte";
  import PromptDialog from "$lib/components/PromptDialog.svelte";
  import TitleBar from "$lib/components/TitleBar.svelte";
  import ProgressiveScanStatus from "$lib/components/ProgressiveScanStatus.svelte";
  import SyncProgressOverlay from "$lib/components/SyncProgressOverlay.svelte";
  import LoginModal from "$lib/components/LoginModal.svelte";
  import { setupI18n } from "$lib/i18n";
  import { isLoading } from "svelte-i18n";
  import "../app.css";
  let stopMobileDetection: (() => void) | undefined;
  let application: ApplicationHandle | undefined;
  let destroyed = false;
  let ready = false;
  let startupError = "";
  function setupAndroidBackHandler() {
    (window as any).__audionHandleBack = (): boolean => {
      if (get(applicationMode) === "controller") return handleControllerBack();
      // 1. Close context menu if open
      const ctx = get(contextMenu);
      if (ctx.visible) {
        contextMenu.set({ ...ctx, visible: false });
        return true;
      }

      // 2. Close full-screen player
      if (get(isFullScreen)) {
        isFullScreen.set(false);
        return true;
      }

      // 3. Close queue panel
      if (get(isQueueVisible)) {
        isQueueVisible.set(false);
        return true;
      }

      if (get(lyricsVisible)) { lyricsVisible.set(false); return true; }
      if (get(isMobileSidebarOpen)) { isMobileSidebarOpen.set(false); return true; }

      // 4. Close mobile search
      if (get(mobileSearchOpen)) {
        mobileSearchOpen.set(false);
        return true;
      }

      // 5. Navigate back through view history
      const nav = get(navigationHistory);
      if (nav.canGoBack) {
        goBack();
        return true;
      }

      // 6. At root — return false so native side minimizes the app
      return false;
    };
  }

  function cleanupAndroidBackHandler() {
    delete (window as any).__audionHandleBack;
  }


  onMount(async () => {
    theme.initialize();
    setupI18n(localStorage.getItem("audion_language") || undefined);
    stopMobileDetection = initMobileDetection();
    if (!isTauri()) { ready = true; return; }
    try {
      await initPlatformDetection();
      const mode = await invoke<ApplicationMode>("get_application_mode");
      if (destroyed) return;
      if (isAndroid()) setupAndroidBackHandler();
      application = await bootstrapApplication(mode, defaultBootstrapLoaders);
      if (destroyed) { await application.dispose(); return; }
      ready = true;
    } catch (error) { startupError = String(error); }
  });
  function dispose() {
    destroyed = true;
    if (typeof window !== "undefined") cleanupAndroidBackHandler();
    stopMobileDetection?.();
    void application?.dispose();
  }
  onDestroy(dispose);
  if (import.meta.hot) import.meta.hot.dispose(dispose);
</script>

{#if $migrationStatus}<div class="migration-banner" role="status">{$migrationStatus}</div>{/if}
{#if startupError}<p role="alert">{startupError}</p>{/if}
{#if !$isLoading && ready}
  {#if $desktopEffectsEnabled && !$isMobile && !$isMiniPlayer}<TitleBar />{/if}
  <ConfirmDialog />
  <PromptDialog />
  {#if $desktopEffectsEnabled}
    <ProgressiveScanStatus />
    <SyncProgressOverlay />
    <LoginModal />
  {/if}
  <div class="app-content" class:desktop-titlebar={$desktopEffectsEnabled && !$isMobile && !$isMiniPlayer}><slot /></div>
{/if}
<style>
  .migration-banner { position: fixed; top: 48px; left: 0; right: 0; background: var(--bg-secondary); color: var(--text-primary); padding: 0.75rem 1rem; text-align: center; z-index: 999; }
  .app-content { height: 100vh; height: 100dvh; width: 100%; overflow: hidden; }
  .app-content.desktop-titlebar { padding-top: 48px; }
</style>
