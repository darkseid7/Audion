<script lang="ts">
 import type { AlbumSort, AlbumView } from "$lib/application/types";
 export let albumView: AlbumView = "grid";
 export let searchQuery = "";
 export let showOnlyFavorites = false;
 export let albumSort: AlbumSort = "artist-asc";
 export let isSortMenuOpen = false;
 export let sortOptions: {value: AlbumSort;label:string}[] = [];
 export let selectedSortLabel = "";
 export let albumGridBody: HTMLDivElement | undefined = undefined;
 export let labels = {viewMode:"Album view",gridView:"Grid view",listView:"List view"};
 export let selectAlbumView: (value: AlbumView) => void = () => {};
 export let toggleSortMenu: () => void = () => isSortMenuOpen = !isSortMenuOpen;
 export let selectSort: (value: AlbumSort) => void = () => {};
 function chooseSort(value: AlbumSort) { selectSort(value); isSortMenuOpen = false; }
</script>
<div class="albums-grid" class:list-view={albumView === "list"} data-layout={albumView}>
    <div class="albums-toolbar-wrap">
        <div class="albums-toolbar">
            <div class="search-filter-group">
                <div class="album-search">
                    <svg class="search-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true">
                        <circle cx="11" cy="11" r="8" />
                        <line x1="21" y1="21" x2="16.65" y2="16.65" />
                    </svg>
                    <input
                        type="text"
                        class="search-input"
                        placeholder="Buscar álbumes..."
                        bind:value={searchQuery}
                    />
                    {#if searchQuery}
                        <button class="search-clear" on:click={() => (searchQuery = "")} aria-label="Limpiar búsqueda">
                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
                        </button>
                    {/if}
                </div>
                <button
                    class="filter-favorites"
                    class:active={showOnlyFavorites}
                    on:click={() => (showOnlyFavorites = !showOnlyFavorites)}
                    title={showOnlyFavorites ? "Mostrar todos" : "Solo favoritos"}
                    aria-pressed={showOnlyFavorites}
                >
                    <svg viewBox="0 0 24 24" width="18" height="18"
                        fill={showOnlyFavorites ? "currentColor" : "none"}
                        stroke="currentColor" stroke-width="2"
                    >
                        <path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"/>
                    </svg>
                </button>
            </div>
            <div class="toolbar-actions">
                <div class="view-controls" role="group" aria-label={labels.viewMode}>
                    <button
                        type="button"
                        class="view-button"
                        class:active={albumView === "grid"}
                        title={labels.gridView}
                        aria-label={labels.gridView}
                        aria-pressed={albumView === "grid"}
                        on:click={() => selectAlbumView("grid")}
                    >
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18" aria-hidden="true">
                            <rect x="3" y="3" width="7" height="7" rx="1" />
                            <rect x="14" y="3" width="7" height="7" rx="1" />
                            <rect x="3" y="14" width="7" height="7" rx="1" />
                            <rect x="14" y="14" width="7" height="7" rx="1" />
                        </svg>
                    </button>
                    <button
                        type="button"
                        class="view-button"
                        class:active={albumView === "list"}
                        title={labels.listView}
                        aria-label={labels.listView}
                        aria-pressed={albumView === "list"}
                        on:click={() => selectAlbumView("list")}
                    >
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18" aria-hidden="true">
                            <path d="M9 5h12M9 12h12M9 19h12" />
                            <path d="M3 4h2v2H3zM3 11h2v2H3zM3 18h2v2H3z" />
                        </svg>
                    </button>
                </div>
            <div class="sort-group">
                <span class="sort-label">Ordenar por</span>
            <div class="sort-dropdown">
                <button
                    class="sort-trigger"
                    type="button"
                    aria-haspopup="menu"
                    aria-expanded={isSortMenuOpen}
                    on:click={toggleSortMenu}
                >
                    <span>{selectedSortLabel}</span>
                    <svg
                        class:open={isSortMenuOpen}
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        width="16"
                        height="16"
                        aria-hidden="true"
                    >
                        <polyline points="6 9 12 15 18 9" />
                    </svg>
                </button>

                {#if isSortMenuOpen}
                    <div class="sort-menu" role="menu" aria-label="Opciones de orden">
                        {#each sortOptions as option (option.value)}
                            <button
                                type="button"
                                role="menuitemradio"
                                aria-checked={albumSort === option.value}
                                class="sort-menu-item"
                                class:active={albumSort === option.value}
                                on:click={() => chooseSort(option.value)}
                            >
                                {option.label}
                            </button>
                        {/each}
                    </div>
                {/if}
            </div>
            </div>
            </div>
        </div>
    </div>

    <div class="albums-grid-body" bind:this={albumGridBody}><slot /></div>
</div>
<style>
    .albums-grid {
        height: 100%;
        display: flex;
        flex-direction: column;
        min-height: 0;
    }

    .albums-grid-body {
        flex: 1;
        min-height: 0;
    }

    .albums-toolbar-wrap {
        padding: 0 var(--spacing-md);
    }

    .albums-toolbar {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        margin-top: 24px;
    }

    .search-filter-group {
        display: flex;
        align-items: center;
        gap: 6px;
        flex: 1;
        min-width: 0;
        max-width: 400px;
    }

    .album-search {
        display: flex;
        align-items: center;
        gap: 6px;
        flex: 1;
        min-width: 0;
        border: 1px solid var(--border-color);
        background: var(--bg-card);
        border-radius: 8px;
        padding: 0 10px;
        height: 34px;
        transition: border-color 0.15s;
    }

    .album-search:focus-within {
        border-color: var(--accent-primary);
        box-shadow: 0 0 0 2px color-mix(in oklab, var(--accent-primary) 20%, transparent);
    }

    .search-icon {
        color: var(--text-secondary);
        flex-shrink: 0;
    }

    .search-input {
        all: unset;
        flex: 1;
        min-width: 0;
        font-size: 0.8rem;
        color: var(--text-primary);
    }



    .search-clear {
        all: unset;
        cursor: pointer;
        color: var(--text-secondary);
        display: flex;
        align-items: center;
        padding: 2px;
        border-radius: 4px;
    }

    .search-clear:hover {
        color: var(--text-primary);
    }

    .filter-favorites {
        all: unset;
        cursor: pointer;
        display: flex;
        align-items: center;
        justify-content: center;
        width: 34px;
        height: 34px;
        border-radius: 8px;
        border: 1px solid var(--border-color);
        background: var(--bg-card);
        color: var(--text-secondary);
        flex-shrink: 0;
        transition: all 0.15s;
    }

    .filter-favorites:hover {
        color: var(--text-primary);
        border-color: var(--text-secondary);
    }

    .filter-favorites.active {
        color: var(--accent-primary);
        border-color: var(--accent-primary);
        background: color-mix(in oklab, var(--accent-primary) 12%, transparent);
    }

    .toolbar-actions {
        display: flex;
        align-items: center;
        gap: 12px;
        min-width: 0;
    }

    .view-controls {
        box-sizing: border-box;
        height: 34px;
        display: flex;
        flex-shrink: 0;
        padding: 2px;
        border: 1px solid var(--border-color);
        border-radius: 8px;
        background: var(--bg-card);
    }

    .view-button {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 28px;
        height: 28px;
        min-height: 0;
        min-width: 0;
        padding: 0;
        border: 0;
        border-radius: 5px;
        background: transparent;
        color: var(--text-secondary);
        cursor: pointer;
    }

    .view-button:hover {
        background: var(--bg-highlight);
        color: var(--text-primary);
    }

    .view-button.active {
        background: var(--bg-highlight);
        color: var(--accent-primary);
    }

    .view-button:focus-visible {
        outline: 2px solid var(--accent-primary);
        outline-offset: 2px;
    }

    .sort-group {
        display: flex;
        align-items: center;
        gap: 8px;
        flex-shrink: 0;
    }

    .sort-label {
        font-size: 0.8rem;
        color: var(--text-secondary);
    }

    .sort-dropdown {
        position: relative;
        min-width: 190px;
    }

    .sort-trigger {
        box-sizing: border-box;
        height: 34px;
        min-height: 0;
        width: 100%;
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        border: 1px solid var(--border-color);
        background: var(--bg-card);
        color: var(--text-primary);
        border-radius: 8px;
        padding: 0 10px;
        font-size: 0.8rem;
        line-height: 1.2;
        cursor: pointer;
    }

    .sort-trigger:focus {
        outline: none;
        border-color: var(--accent-primary);
        box-shadow: 0 0 0 2px color-mix(in oklab, var(--accent-primary) 20%, transparent);
    }

    .sort-trigger svg {
        color: var(--text-secondary);
        transition: transform 0.18s ease;
    }

    .sort-trigger svg.open {
        transform: rotate(180deg);
    }

    .sort-menu {
        position: absolute;
        top: calc(100% + 6px);
        right: 0;
        min-width: 100%;
        background: var(--bg-surface);
        border: 1px solid var(--border-color);
        border-radius: 10px;
        box-shadow: 0 12px 32px rgba(0, 0, 0, 0.45);
        padding: 6px;
        display: flex;
        flex-direction: column;
        gap: 2px;
        z-index: 40;
        max-height: min(360px, 60vh);
        overflow-y: auto;
        overflow-x: hidden;
        isolation: isolate;
        opacity: 1;
    }

    .sort-menu-item {
        border: 0;
        background: transparent;
        color: var(--text-primary);
        text-align: left;
        border-radius: 8px;
        padding: 8px 10px;
        font-size: 0.8rem;
        cursor: pointer;
    }

    .sort-menu-item:hover {
        background: var(--bg-highlight);
    }

    .sort-menu-item.active {
        background: color-mix(in oklab, var(--accent-primary) 20%, transparent);
        color: var(--accent-primary);
        font-weight: 600;
    }

    @media (max-width: 768px) {
        .albums-toolbar-wrap {
            padding: 0 var(--spacing-sm);
        }

        .albums-toolbar {
            flex-wrap: wrap;
            gap: 8px;
            margin-top: 0;
        }

        .search-filter-group {
            max-width: none;
            width: 100%;
        }

        .sort-label {
            display: none;
        }

        .sort-dropdown {
            width: 100%;
            min-width: 0;
        }

        .toolbar-actions {
            width: 100%;
            gap: 8px;
        }

        .sort-group {
            flex: 1;
            min-width: 0;
        }

        .sort-menu {
            left: 0;
            right: 0;
        }
    }










.search-input::placeholder { color: var(--text-secondary); }
</style>
