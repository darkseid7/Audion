<script context="module" lang="ts">
  import type { NavigationRow } from "$lib/application/presentation/types";
  export function navigateRow(row: NavigationRow, navigate: (id: string) => void): void {
    if (row.availability.enabled) navigate(row.id);
  }
</script>
<script lang="ts">
  import type { NavigationSection } from "$lib/application/presentation/types";
  export let sections: readonly NavigationSection[] = [];
  export let variant: "expanded" | "compact" | "library" = "expanded";
  export let hasPlayer = false;
  export let collapsible = false;
  export let collapsed = false;
  export let onNavigate: (id: string) => void = () => {};
  $: unavailableRows = sections.flatMap(section => section.rows).filter(row => !row.availability.enabled);
  const icons: Record<string, string> = {
  "home": "M10 20v-6h4v6h5v-8h3L12 3 2 12h3v8z",
  "albums": "M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 14.5c-2.49 0-4.5-2.01-4.5-4.5S9.51 7.5 12 7.5s4.5 2.01 4.5 4.5-2.01 4.5-4.5 4.5zm0-5.5c-.55 0-1 .45-1 1s.45 1 1 1 1-.45 1-1-.45-1-1-1z",
  "liked-songs": "M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z",
  "listen-later": "M12 1.75A10.25 10.25 0 1 0 22.25 12 10.26 10.26 0 0 0 12 1.75zm0 18.5A8.25 8.25 0 1 1 20.25 12 8.26 8.26 0 0 1 12 20.25zm.75-13.25h-1.5v6l5 3 .75-1.23-4.25-2.52z",
  "recently-played": "M12 2a10 10 0 1 0 10 10A10 10 0 0 0 12 2zm0 18a8 8 0 1 1 8-8 8 8 0 0 1-8 8zm.5-13H11v6l5.25 3.15.75-1.23-4.5-2.67z",
  "discover": "M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-1 17.93c-3.95-.49-7-3.85-7-7.93 0-.62.08-1.21.21-1.79L9 15v1c0 1.1.9 2 2 2v1.93zm6.9-2.54c-.26-.81-1-1.39-1.9-1.39h-1v-3c0-.55-.45-1-1-1H8v-2h2c.55 0 1-.45 1-1V7h2c1.1 0 2-.9 2-2v-.41c2.93 1.19 5 4.06 5 7.41 0 2.08-.8 3.97-2.1 5.39z",
  "tracks": "M12 3v10.55c-.59-.34-1.27-.55-2-.55-2.21 0-4 1.79-4 4s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z",
  "artists": "M12 12c2.21 0 4-1.79 4-4s-1.79-4-4-4-4 1.79-4 4 1.79 4 4 4zm0 2c-2.67 0-8 1.34-8 4v2h16v-2c0-2.66-5.33-4-8-4z",
  "playlists": "M19 9H5V7h14v2zm0 4H5v-2h14v2zm-8 4H5v-2h6v2zM17 6v8.18c-.31-.11-.65-.18-1-.18-1.66 0-3 1.34-3 3s1.34 3 3 3 3-1.34 3-3V8h3V6h-5z",
  "plugins": "M20.5 11H19V7c0-1.1-.9-2-2-2h-4V3.5C13 2.12 11.88 1 10.5 1S8 2.12 8 3.5V5H4c-1.1 0-1.99.9-1.99 2v3.8H3.5c1.49 0 2.7 1.21 2.7 2.7s-1.21 2.7-2.7 2.7H2V20c0 1.1.9 2 2 2h3.8v-1.5c0-1.49 1.21-2.7 2.7-2.7s2.7 1.21 2.7 2.7V22H17c1.1 0 2-.9 2-2v-4h1.5c1.38 0 2.5-1.12 2.5-2.5S21.88 11 20.5 11z",
  "settings": "M19.14 12.94c.04-.31.06-.63.06-.94 0-.31-.02-.63-.06-.94l2.03-1.58c.18-.14.23-.41.12-.61l-1.92-3.32c-.12-.22-.37-.29-.59-.22l-2.39.96c-.5-.38-1.03-.7-1.62-.94l-.36-2.54c-.04-.24-.24-.41-.48-.41h-3.84c-.24 0-.43.17-.47.41l-.36 2.54c-.59.24-1.13.57-1.62.94l-2.39-.96c-.22-.08-.47 0-.59.22L2.74 8.87c-.12.21-.08.47.12.61l2.03 1.58c-.04.31-.06.63-.06.94s.02.63.06.94l-2.03 1.58c-.18.14-.23.41-.12.61l1.92 3.32c.12.22.37.29.59.22l2.39-.96c.5.38 1.03.7 1.62.94l.36 2.54c.05.24.24.41.48.41h3.84c.24 0 .44-.17.47-.41l.36-2.54c.59-.24 1.13-.56 1.62-.94l2.39.96c.22.08.47 0 .59-.22l1.92-3.32c.12-.22.07-.47-.12-.61l-2.01-1.58zM12 15.6c-1.98 0-3.6-1.62-3.6-3.6s1.62-3.6 3.6-3.6 3.6 1.62 3.6 3.6-1.62 3.6-3.6 3.6z",
  "library": "M20 2H8c-1.1 0-2 .9-2 2v12c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V4c0-1.1-.9-2-2-2zm-2 5h-3v5.5a2.5 2.5 0 0 1-5 0 2.5 2.5 0 0 1 2.5-2.5c.57 0 1.08.19 1.5.51V5h4v2zM4 6H2v14c0 1.1.9 2 2 2h14v-2H4V6z",
  "actions": "M7 2v11h3v9l7-12h-4l4-8z"
};
</script>
{#if variant === "expanded"}
  <aside class="sidebar" class:collapsed={collapsible && collapsed}>
    <div class="sidebar-header"><div class="logo"><img src="/logo.png" alt="Audion Logo" width="32" height="32" /><span class="logo-text">Audion</span><slot name="header" /></div>
      {#if collapsible}<button class="sidebar-toggle" aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"} title={collapsed ? "Expand sidebar" : "Collapse sidebar"} aria-expanded={!collapsed} on:click={() => collapsed = !collapsed}><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="20" height="20" aria-hidden="true"><path d={collapsed ? "m9 6 6 6-6 6" : "m15 6-6 6 6 6"} /></svg></button>{/if}
    </div>
    <nav class="sidebar-nav" aria-label="Music library">
      <slot name="top" />
      {#each sections as section (section.id)}
        <section class="nav-section">
          <h3 class="nav-section-title">{section.label}</h3>
          <ul class="nav-list">
            {#each section.rows as row (row.id)}
              <li><button class="nav-item" class:active={row.active} aria-label={row.label} aria-current={row.active ? "page" : undefined} disabled={!row.availability.enabled} title={!row.availability.enabled ? `${row.label}: ${row.availability.reason}` : (collapsible && collapsed ? row.label : undefined)} on:click={() => navigateRow(row, onNavigate)}>
                {#if row.icon === "listenbrainz"}
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="24" height="24" aria-hidden="true"><circle cx="11" cy="11" r="8" /><line x1="21" y1="21" x2="16.65" y2="16.65" /><line x1="11" y1="8" x2="11" y2="14" /><line x1="8" y1="11" x2="14" y2="11" /></svg>
                {:else}<svg viewBox="0 0 24 24" fill="currentColor" width="24" height="24" aria-hidden="true"><path d={icons[row.icon]} /></svg>{/if}
                <span>{row.label}</span>
                {#if row.count !== undefined}<span class="nav-count">{row.count}</span>{/if}
                {#if !row.availability.enabled}<span class="unavailable-reason">{row.availability.reason}</span>{/if}
              </button></li>
            {/each}
            {#if section.id === "playlists"}<slot name="playlists" />{/if}
          </ul>
        </section>
      {/each}
      <slot name="community" />
      {#if unavailableRows.length}<details class="availability-disclosure"><summary>Unavailable views</summary><ul>{#each unavailableRows as row (row.id)}{#if !row.availability.enabled}<li>{row.label}: {row.availability.reason}</li>{/if}{/each}</ul></details>{/if}
    </nav>
    {#if $$slots.footer}<div class="sidebar-footer"><slot name="footer" /></div>{/if}
  </aside>
{:else if variant === "compact"}
  <div class="compact-navigation">
  {#if unavailableRows.length}<details class="availability-disclosure"><summary>Unavailable views</summary><ul>{#each unavailableRows as row (row.id)}{#if !row.availability.enabled}<li>{row.label}: {row.availability.reason}</li>{/if}{/each}</ul></details>{/if}
  <nav class="bottom-nav" class:has-player={hasPlayer} aria-label="Music library">
    {#each sections as section (section.id)}{#each section.rows as row (row.id)}
      <button class="nav-item" class:active={row.active} aria-label={row.label} aria-current={row.active ? "page" : undefined} disabled={!row.availability.enabled} title={!row.availability.enabled ? `${row.label}: ${row.availability.reason}` : (collapsible && collapsed ? row.label : undefined)} on:click={() => navigateRow(row, onNavigate)}>
        <svg class="nav-icon" viewBox="0 0 24 24" fill="currentColor" width="24" height="24" aria-hidden="true"><path d={icons[row.icon]} /></svg><span>{row.label}</span>
        {#if !row.availability.enabled}<span class="unavailable-reason">{row.availability.reason}</span>{/if}
      </button>
    {/each}{/each}
    <slot name="extensions" />
  </nav>
  </div>
{:else}
  <nav class="mobile-library-tabs-wrapper" aria-label="Library sections"><div class="mobile-library-tabs">
    {#each sections as section (section.id)}{#each section.rows as row (row.id)}
      <button class="lib-tab" class:active={row.active} aria-current={row.active ? "page" : undefined} disabled={!row.availability.enabled} title={!row.availability.enabled ? `${row.label}: ${row.availability.reason}` : (collapsible && collapsed ? row.label : undefined)} on:click={() => navigateRow(row, onNavigate)}>{row.label}</button>
    {/each}{/each}
  </div>{#if unavailableRows.length}<details class="availability-disclosure"><summary>Unavailable views</summary><ul>{#each unavailableRows as row (row.id)}{#if !row.availability.enabled}<li>{row.label}: {row.availability.reason}</li>{/if}{/each}</ul></details>{/if}</nav>
{/if}
<style>
  :global {

    .sidebar {
        width: var(--sidebar-width);
        height: 100%;
        background-color: var(--bg-base);
        display: flex;
        flex-direction: column;
        border-right: 1px solid var(--border-color);
    }

    .sidebar .sidebar-header {
        padding: var(--spacing-md);
        padding-top: var(--spacing-lg);
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
    }

    .sidebar .logo {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        color: var(--accent-primary);
    }

    .sidebar .logo-text {
        font-size: 1.5rem;
        font-weight: 700;
        letter-spacing: -0.5px;
    }

    .sidebar .update-badge {
        font-size: 0.6rem;
        font-weight: 800;
        color: var(--accent-primary);
        background-color: var(--accent-subtle);
        border: 1px solid var(--accent-primary);
        padding: 1px 8px;
        border-radius: 12px;
        margin-left: var(--spacing-sm);
        cursor: pointer;
        user-select: none;
        white-space: nowrap;
        text-transform: uppercase;
        letter-spacing: 0.05em;
        margin-top: 2px;
        transition: all 0.2s ease;
        animation: glow 3s infinite ease-in-out;
    }

    .sidebar .update-badge:hover {
        background-color: var(--accent-primary);
        color: var(--bg-base);
        transform: translateY(-1px);
        box-shadow: 0 2px 8px var(--accent-subtle);
    }

    @keyframes glow {
        0%,
        100% {
            box-shadow: 0 0 2px transparent;
        }
        50% {
            box-shadow: 0 0 8px var(--accent-subtle);
        }
    }

    .sidebar .sidebar-nav {
        flex: 1;
        overflow-y: auto;
        overscroll-behavior-y: contain;
        padding: var(--spacing-md);
    }

    .sidebar .nav-section {
        margin-bottom: var(--spacing-xl);
    }

    .sidebar .nav-section-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
    }

    .sidebar .nav-section-title {
        font-size: 0.6875rem;
        font-weight: 700;
        text-transform: uppercase;
        letter-spacing: 0.12em;
        color: var(--text-subdued);
        margin-bottom: var(--spacing-md);
        padding-left: var(--spacing-md);
    }

    .sidebar .nav-list {
        list-style: none;
        display: flex;
        flex-direction: column;
        gap: 2px;
    }

    .sidebar .nav-item {
        display: flex;
        align-items: center;
        gap: var(--spacing-md);
        width: 100%;
        padding: 12px var(--spacing-md);
        border-radius: var(--radius-md);
        color: var(--text-secondary);
        transition: all var(--transition-fast);
        text-align: left;
        font-size: 0.9375rem;
        position: relative;
    }

    .sidebar .nav-item:hover {
        color: var(--text-primary);
        background-color: rgba(255, 255, 255, 0.1);
    }

    .sidebar .nav-item.active {
        color: var(--text-primary);
        background-color: var(--bg-surface);
        font-weight: 500;
    }

    .sidebar .nav-item.playing {
        background-color: var(--accent-subtle);
        color: var(--text-primary);
    }

    .sidebar .nav-item svg,
.sidebar .nav-item img {
        flex-shrink: 0;
        opacity: 0.7;
    }

    .sidebar .resonate-icon {
        width: 24px;
        height: 24px;
        object-fit: contain;
    }

    .sidebar .nav-item.active svg {
        opacity: 1;
        color: var(--accent-primary);
    }

    .sidebar .nav-count {
        margin-left: auto;
        font-size: 0.75rem;
        color: var(--text-subdued);
    }

    .sidebar .playlist-item {
        padding-left: var(--spacing-md);
    }

    .sidebar .playing-indicator {
        display: flex;
        align-items: center;
        justify-content: center;
        gap: 2px;
        width: 24px;
        height: 24px;
        flex-shrink: 0;
    }

    .sidebar .playing-indicator .bar {
        width: 3px;
        height: 12px;
        background-color: var(--accent-primary);
        animation: equalizer 0.8s ease-in-out infinite;
    }

    .sidebar .playing-indicator .bar:nth-child(2) {
        animation-delay: 0.2s;
    }

    .sidebar .playing-indicator .bar:nth-child(3) {
        animation-delay: 0.4s;
    }

    @keyframes equalizer {
        0%,
        100% {
            height: 4px;
        }
        50% {
            height: 14px;
        }
    }

    .sidebar .nav-item.playing .nav-count {
        color: var(--accent-primary);
        font-weight: 600;
    }

    .sidebar .sidebar-footer {
        padding: var(--spacing-md);
        border-top: 1px solid var(--border-color);
    }

    .sidebar .add-folder-btn {
        display: flex;
        align-items: center;
        justify-content: center;
        gap: var(--spacing-sm);
        width: 100%;
        padding: var(--spacing-sm) var(--spacing-md);
        background-color: var(--bg-surface);
        color: var(--text-primary);
        border-radius: var(--radius-md);
        font-weight: 500;
        transition: all var(--transition-fast);
    }

    .sidebar .add-folder-btn:hover:not(:disabled) {
        background-color: var(--bg-highlight);
    }

    .sidebar .add-folder-btn:disabled {
        opacity: 0.7;
        cursor: wait;
    }

    .sidebar .scan-error {
        margin-top: var(--spacing-sm);
        font-size: 0.75rem;
        color: var(--error-color);
        text-align: center;
    }

    .sidebar .plugin-slot {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-sm);
        margin-bottom: var(--spacing-md);
    }

    .sidebar .animate-spin {
        animation: spin 1s linear infinite;
    }

    @keyframes spin {
        from {
            transform: rotate(0deg);
        }
        to {
            transform: rotate(360deg);
        }
    }

    .sidebar .playlist-name {
        flex: 1;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-size: 0.8125rem;
    }

    .sidebar .pinned-indicator-sidebar {
        color: var(--accent-primary);
        display: flex;
        align-items: center;
        margin-left: var(--spacing-xs);
        opacity: 0.8;
    }

    .sidebar .playlist-item:hover .pinned-indicator-sidebar {
        opacity: 1;
    }

    /* Mobile: sidebar fills its container (the drawer) */
    @media (max-width: 768px) {
        .sidebar {
            width: 100%;
            border-right: none;
            height: 100%;
        }

        .sidebar .sidebar-header {
            padding-top: var(--spacing-md);
        }

        .sidebar .nav-item {
            padding: 14px var(--spacing-md);
            min-height: 48px;
        }
    }
    .sidebar .playlist-icon-container {
        width: 24px;
        height: 24px;
        display: flex;
        align-items: center;
        justify-content: center;
        flex-shrink: 0;
        overflow: hidden;
        border-radius: 4px;
    }

    .sidebar .sidebar-playlist-art {
        width: 100%;
        height: 100%;
        object-fit: cover;
    }


    .bottom-nav {
        position: fixed;
        bottom: 0;
        left: 0;
        width: 100%;
        height: calc(60px + env(safe-area-inset-bottom));
        background-color: var(--bg-base);
        border-top: 1px solid var(--border-color);
        display: flex;
        justify-content: space-around;
        align-items: flex-start;
        padding-top: 6px;
        padding-bottom: env(safe-area-inset-bottom);
        padding-left:var(--safe-area-left);
        padding-right:var(--safe-area-right);
        z-index: 1000;
        -webkit-tap-highlight-color: transparent;
        user-select: none;
    }

    .bottom-nav .nav-item {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        color: var(--text-subdued);
        text-align: center;
        font-size: 10px;
        font-weight: 500;
        gap: 2px;
        padding: 4px 12px;
        border-radius: var(--radius-sm);
        transition: color var(--transition-fast);
        background: none;
        border: none;
        cursor: pointer;
        min-width: 64px;
        min-height: 48px;
        -webkit-tap-highlight-color: transparent;
    }

    .bottom-nav .nav-item:active {
        transform: scale(0.92);
    }

    .bottom-nav .nav-item.active {
        color: var(--text-primary);
    }

    .bottom-nav .nav-item.active .nav-icon {
        color: var(--text-primary);
    }

    .bottom-nav .nav-icon {
        display: block;
        width: 24px;
        height: 24px;
    }

    .bottom-nav .plugin-slot {
        display: none; /* Hidden by default, plugins can override */
    }

    .mobile-library-tabs-wrapper {
        flex-shrink: 0;
        padding: var(--spacing-md) var(--spacing-md) 0;
        background-color: var(--bg-base);
    }

    .mobile-library-tabs {
        display: flex;
        gap: 8px;
        overflow-x: auto;
        scrollbar-width: none;
        -webkit-overflow-scrolling: touch;
        -webkit-tap-highlight-color: transparent;
        user-select: none;
    }

    .mobile-library-tabs::-webkit-scrollbar {
        display: none;
    }

    .lib-tab {
        flex-shrink: 0;
        padding: 8px 16px;
        border-radius: var(--radius-full);
        font-size: 0.8125rem;
        font-weight: 600;
        color: var(--text-primary);
        background-color: rgba(255, 255, 255, 0.07);
        border: none;
        cursor: pointer;
        transition: all var(--transition-fast);
        -webkit-tap-highlight-color: transparent;
        white-space: nowrap;
    }

    .lib-tab.active {
        background-color: var(--accent-primary);
        color: var(--bg-base);
    }
    .lib-tab:active:not(.active) {
        background-color: rgba(255, 255, 255, 0.12);
    }
  }
  .unavailable-reason { position: absolute; width: 1px; height: 1px; padding: 0; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .sidebar button:disabled, .bottom-nav button:disabled { opacity: .5; cursor: not-allowed; }
  .availability-disclosure { color: var(--text-secondary); font-size: .8125rem; padding: 8px; background: var(--bg-base); }
  .availability-disclosure summary { cursor: pointer; min-height: 44px; display: flex; align-items: center; }
  .availability-disclosure ul { margin: 0; padding: 8px 16px; }
  .availability-disclosure li { margin-bottom: 8px; }
  .compact-navigation .availability-disclosure { border-top: 1px solid var(--border-subtle); }
  .compact-navigation .availability-disclosure[open] { max-height: 40dvh; overflow: auto; }
  summary:focus-visible { outline: 2px solid var(--accent-primary); outline-offset: 2px; }
  button:focus-visible { outline: 2px solid var(--accent-primary); outline-offset: 2px; }
  .sidebar-toggle { flex-shrink: 0; margin-left: auto; min-width: 44px; min-height: 44px; display: flex; align-items: center; justify-content: center; color: var(--text-secondary); border-radius: var(--radius-md); }
  .sidebar-toggle:hover { color: var(--text-primary); background: var(--bg-surface); }
  :global(.sidebar.collapsed) { width: 72px; }
  :global(.sidebar.collapsed .sidebar-header) { flex-direction: column; padding: 12px 8px; gap: 4px; }
  :global(.sidebar.collapsed .sidebar-toggle) { margin-left: 0; }
  :global(.sidebar.collapsed .sidebar-nav) { padding: 8px; }
  :global(.sidebar.collapsed .nav-item) { justify-content: center; min-height: 44px; padding: 12px; }
  :global(.sidebar.collapsed .nav-section) { margin-bottom: 16px; }
  :global(.sidebar.collapsed .logo-text), :global(.sidebar.collapsed .nav-section-title), :global(.sidebar.collapsed .nav-item span), :global(.sidebar.collapsed .sidebar-footer span) { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  :global(.sidebar.collapsed .sidebar-footer) { padding: 8px; }
  :global(.sidebar.collapsed .add-folder-btn) { min-width: 44px; padding: 12px; justify-content: center; }
  :global(.sidebar.collapsed .availability-disclosure) { font-size: .6875rem; padding: 0; overflow-wrap: anywhere; }
  :global(.sidebar.collapsed .availability-disclosure ul) { padding: 4px; }
</style>
