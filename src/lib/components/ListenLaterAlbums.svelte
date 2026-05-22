<script lang="ts">
    import { albums as libraryAlbums } from "$lib/stores/library";
    import AlbumGrid from "./AlbumGrid.svelte";
    import {
        listenLaterAlbumIds,
        listenLaterAlbumOrder,
    } from "$lib/stores/listen-later";
    import { _ } from "svelte-i18n";

    $: orderMap = new Map(
        $listenLaterAlbumOrder.map((id, index) => [id, index]),
    );

    $: listenLaterAlbums = $libraryAlbums
        .filter((album) => $listenLaterAlbumIds.has(album.id))
        .sort((a, b) => {
            const aIdx = orderMap.get(a.id) ?? Number.MAX_SAFE_INTEGER;
            const bIdx = orderMap.get(b.id) ?? Number.MAX_SAFE_INTEGER;
            return aIdx - bIdx;
        });
</script>

<div class="listen-later-view">
    <header class="listen-later-header">
        <h1>{$_("listenLater.title", { default: "Escuchar más tarde" })}</h1>
        <span class="count"
            >{$listenLaterAlbumIds.size} {$_("listenLater.albums", { default: "álbumes" })}</span
        >
    </header>

    {#if listenLaterAlbums.length === 0}
        <div class="empty-state">
            <p>{$_("listenLater.emptyTitle", { default: "Aún no tienes álbumes guardados" })}</p>
            <span>{$_("listenLater.emptyDescription", { default: "Desde un álbum o lista de álbumes, usa el menú contextual para añadirlo." })}</span>
        </div>
    {:else}
        <AlbumGrid albums={listenLaterAlbums} />
    {/if}
</div>

<style>
    .listen-later-view {
        height: 100%;
        display: flex;
        flex-direction: column;
        min-height: 0;
    }

    .listen-later-header {
        padding: 24px 24px 12px;
        display: flex;
        align-items: baseline;
        justify-content: space-between;
        gap: 12px;
    }

    .listen-later-header h1 {
        margin: 0;
        font-size: 1.75rem;
    }

    .count {
        color: var(--text-secondary);
        font-size: 0.9rem;
    }

    .empty-state {
        margin: auto;
        text-align: center;
        color: var(--text-secondary);
        padding: 24px;
        max-width: 560px;
    }

    .empty-state p {
        margin: 0 0 8px;
        font-size: 1.1rem;
        color: var(--text-primary);
        font-weight: 600;
    }
</style>
