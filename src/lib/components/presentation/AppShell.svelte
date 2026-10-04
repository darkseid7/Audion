<script lang="ts">
  export let layout: "compact" | "expanded" = "expanded";
  export let safeArea = false;
  export let reserveBottomNavigation = false;
</script>
<div class="shared-app-shell" class:safe-area={safeArea} data-layout={layout}>
  <slot name="header" />
  <slot name="status" />
  <div class="app-layout" class:compact={layout === "compact"}>
    <div class="desktop-sidebar"><slot name="sidebar" /></div>
    <div class="shell-content"><slot /></div>
    <slot name="panels" />
  </div>
  <slot name="feedback" />
  <slot name="player" />
  {#if layout === "compact"}<div class="bottom-navigation" class:reserved={reserveBottomNavigation}><slot name="bottom-navigation" /></div>{/if}
</div>
<style>
  .shared-app-shell { display: flex; flex-direction: column; flex: 1; width: 100%; height: 100%; min-height: 0; min-width: 0; overflow: hidden; background: var(--bg-base); color: var(--text-primary); }
  .safe-area { padding: var(--safe-area-top) var(--safe-area-right) var(--safe-area-bottom) var(--safe-area-left); }
  .app-layout { flex: 1; display: flex; overflow: hidden; min-height: 0; min-width: 0; }
  .desktop-sidebar { display: flex; flex-shrink: 0; }
  .compact .desktop-sidebar { display: none; }
  .shell-content { display: flex; flex: 1; flex-direction: column; min-width: 0; min-height: 0; overflow: hidden; }
  .bottom-navigation { flex-shrink: 0; }
  .reserved :global(.bottom-nav) { position: static; height: auto; min-height: calc(60px + env(safe-area-inset-bottom)); }
  .reserved { min-height: calc(60px + env(safe-area-inset-bottom)); }
</style>
