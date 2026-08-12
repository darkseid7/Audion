<script lang="ts">
  import { onMount, createEventDispatcher } from "svelte";
  import type { Album } from "$lib/api/tauri";
  import {
    getArtistMusicBrainzInfo,
    getSimilarArtistsMb,
    getReleaseDetailMb,
    refreshReleaseDetailMb,
    getAlbumCoverSrc,
    type MbArtistInfo,
    type MbSimilarArtist,
    type MbReleaseDetail,
    type MbReleaseTrack,
  } from "$lib/api/tauri";
  import { goToArtistDetail } from "$lib/stores/view";
  import { addToast } from "$lib/stores/toast";
  import MediaCard from "./MediaCard.svelte";
  import { _, locale } from "svelte-i18n";

  export let album: Album;
  export let open: boolean = false;

  const dispatch = createEventDispatcher<{ close: void }>();

  type TabId = "artist" | "album";
  let activeTab: TabId = "artist";

  // Artist tab state
  let artistInfo: MbArtistInfo | null = null;
  let similarArtists: MbSimilarArtist[] = [];
  let artistLoading = false;
  let artistError: string | null = null;

  // Album tab state
  let releaseDetail: MbReleaseDetail | null = null;
  let albumLoading = false;
  let albumError: string | null = null;
  let coverFailed = false;

  // Cover fallback: use the locally-stored album cover.
  $: localCover = getAlbumCoverSrc(album, "high");

  function formatDuration(ms: number | null | undefined): string {
    if (!ms || !isFinite(ms) || ms < 0) return "—";
    const total = Math.floor(ms / 1000);
    const m = Math.floor(total / 60);
    const s = total % 60;
    return `${m}:${s.toString().padStart(2, "0")}`;
  }

  function formatTotalDuration(ms: number | null | undefined): string {
    if (!ms) return "";
    const total = Math.floor(ms / 1000);
    const h = Math.floor(total / 3600);
    const m = Math.floor((total % 3600) / 60);
    if (h > 0) return `${h} h ${m} min`;
    return `${m} min`;
  }

  function mbReleaseUrl(mbid: string): string {
    return `https://musicbrainz.org/release/${mbid}`;
  }
  function mbRecordingUrl(mbid: string): string {
    return `https://musicbrainz.org/recording/${mbid}`;
  }

  async function loadArtist() {
    artistLoading = true;
    artistError = null;
    try {
      const artistName = album.artist || "Unknown Artist";
      const [info, similar] = await Promise.all([
        getArtistMusicBrainzInfo(artistName),
        getSimilarArtistsMb(artistName),
      ]);
      artistInfo = info;
      similarArtists = similar;
    } catch (e) {
      artistError = (e as Error)?.message || "Failed to load artist info";
      artistInfo = null;
      similarArtists = [];
    } finally {
      artistLoading = false;
    }
  }

  async function loadAlbum(refresh = false) {
    albumLoading = true;
    albumError = null;
    coverFailed = false;
    try {
      const fn = refresh ? refreshReleaseDetailMb : getReleaseDetailMb;
      releaseDetail = await fn(album.name, album.artist || "");
    } catch (e) {
      albumError = (e as Error)?.message || "Failed to load album info";
      releaseDetail = null;
    } finally {
      albumLoading = false;
    }
  }

  // Load data when modal opens; reload the active tab each time it changes.
  $: if (open) {
    if (activeTab === "artist" && !artistInfo && !artistLoading && !artistError) {
      loadArtist();
    } else if (activeTab === "album" && !releaseDetail && !albumLoading && !albumError) {
      loadAlbum(false);
    }
  }

  function switchTab(tab: TabId) {
    activeTab = tab;
    if (tab === "artist" && !artistInfo && !artistLoading && !artistError) {
      loadArtist();
    } else if (tab === "album" && !releaseDetail && !albumLoading && !albumError) {
      loadAlbum(false);
    }
  }

  async function handleRefresh() {
    if (activeTab === "artist") {
      artistInfo = null;
      similarArtists = [];
      artistError = null;
      await loadArtist();
    } else {
      await loadAlbum(true);
    }
    addToast($_("album.infoRefreshed"), "success");
  }

  function handleClose() {
    dispatch("close");
  }

  function handleBackdropClick(e: MouseEvent) {
    if ((e.target as HTMLElement).classList.contains("modal-backdrop")) {
      handleClose();
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && open) handleClose();
  }

  // Persist the last-opened tab so reopening the modal feels natural.
  onMount(() => {
    try {
      const saved = localStorage.getItem("audion_album_info_tab");
      if (saved === "artist" || saved === "album") activeTab = saved;
    } catch {}
  });
  $: try {
    if (open) localStorage.setItem("audion_album_info_tab", activeTab);
  } catch {}
</script>

<svelte:window on:keydown={handleKeydown} />

<!-- svelte-ignore a11y-click-events-have-key-events a11y-no-static-element-interactions -->
{#if open}
  <div
    class="modal-backdrop"
    on:click={handleBackdropClick}
    role="presentation"
  >
    <div
      class="modal info-modal"
      role="dialog"
      aria-modal="true"
      aria-labelledby="album-info-title"
    >
      <header class="modal-header">
        <div class="modal-title">
          <h2 id="album-info-title">{album.name || $_("album.unknownAlbum")}</h2>
          <p class="subtitle">
            <button
              class="artist-link"
              type="button"
              on:click={() => {
                handleClose();
                if (album.artist) goToArtistDetail(album.artist);
              }}
            >
              {album.artist || $_("album.unknownArtist")}
            </button>
            {#if releaseDetail?.year}
              <span class="dim"> · {releaseDetail.year}</span>
            {/if}
          </p>
        </div>
        <div class="header-actions">
          <button
            class="icon-btn refresh-btn"
            type="button"
            on:click={handleRefresh}
            title={$_("album.infoRefresh")}
            aria-label={$_("album.infoRefresh")}
          >
            <svg viewBox="0 0 24 24" width="18" height="18" fill="currentColor">
              <path d="M17.65 6.35A7.96 7.96 0 0 0 12 4a8 8 0 1 0 7.74 10h-2.08A6 6 0 1 1 12 6c1.66 0 3.14.69 4.22 1.78L13 11h7V4l-2.35 2.35z"/>
            </svg>
          </button>
          <button
            class="icon-btn close-btn"
            type="button"
            on:click={handleClose}
            aria-label={$_("album.closeInfo")}
          >
            ✕
          </button>
        </div>
      </header>

      <nav class="tabs" role="tablist">
        <button
          class="tab"
          class:active={activeTab === "artist"}
          role="tab"
          aria-selected={activeTab === "artist"}
          type="button"
          on:click={() => switchTab("artist")}
        >
          {$_("album.tabArtist")}
        </button>
        <button
          class="tab"
          class:active={activeTab === "album"}
          role="tab"
          aria-selected={activeTab === "album"}
          type="button"
          on:click={() => switchTab("album")}
        >
          {$_("album.tabAlbum")}
        </button>
      </nav>

      <div class="modal-body">
        {#if activeTab === "artist"}
          <section class="tab-panel" role="tabpanel">
            {#if artistLoading}
              <div class="loading">
                <div class="spinner"></div>
                <span>{$_("artist.lookingUp")}</span>
              </div>
            {:else if artistError}
              <p class="error-msg">{artistError}</p>
            {:else if artistInfo}
              {#if artistInfo.genres.length > 0}
                <section class="info-section">
                  <h3 class="info-heading">{$_("artist.genres")}</h3>
                  <div class="genre-pills">
                    {#each artistInfo.genres as genre}
                      <span class="genre-pill">{genre}</span>
                    {/each}
                  </div>
                </section>
              {/if}

              {#if artistInfo.bio}
                <section class="info-section">
                  <h3 class="info-heading">{$_("artist.about")}</h3>
                  <p class="bio-text">{artistInfo.bio}</p>
                </section>
              {/if}

              {#if artistInfo.wikipedia_url}
                <a
                  class="ext-link"
                  href={artistInfo.wikipedia_url}
                  target="_blank"
                  rel="noopener noreferrer"
                >
                  <svg viewBox="0 0 24 24" fill="currentColor" width="14" height="14">
                    <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-1 15v-4H7l5-8v4h4l-5 8z"/>
                  </svg>
                  {$_("artist.readOnWikipedia")}
                </a>
              {/if}

              {#if artistInfo.disambiguation}
                <p class="disambiguation">({artistInfo.disambiguation})</p>
              {/if}

              {#if similarArtists.length > 0}
                <section class="info-section">
                  <h3 class="info-heading">{$_("artist.relatedArtists")}</h3>
                  <div class="similar-grid">
                    {#each similarArtists as a}
                      <MediaCard
                        variant="round"
                        primaryText={a.name}
                        secondaryText={a.relation_type}
                        isPinned={false}
                        on:click={() => {
                          handleClose();
                          goToArtistDetail(a.name);
                        }}
                      >
                        <svelte:fragment slot="cover">
                          <div class="artist-initial-sm">
                            {a.name.charAt(0).toUpperCase()}
                          </div>
                        </svelte:fragment>
                        <svelte:fragment slot="extra-info">
                          {#if a.in_library}
                            <span class="in-library-dot" title="In your library">•</span>
                          {/if}
                        </svelte:fragment>
                      </MediaCard>
                    {/each}
                  </div>
                </section>
              {/if}

              <p class="attribution">{$_("album.dataFromMusicBrainz")}</p>
            {:else}
              <p class="dim">{$_("album.noInfoFound")}</p>
            {/if}
          </section>
        {:else}
          <section class="tab-panel" role="tabpanel">
            {#if albumLoading}
              <div class="loading">
                <div class="spinner"></div>
                <span>{$_("album.loadingInfo")}</span>
              </div>
            {:else if albumError}
              <p class="error-msg">{albumError}</p>
            {:else if releaseDetail}
              <div class="album-grid">
                <div class="album-cover">
                  {#if releaseDetail.cover_url_500 && !coverFailed}
                    <img
                      src={releaseDetail.cover_url_500}
                      alt={releaseDetail.title}
                      on:error={() => (coverFailed = true)}
                    />
                  {:else}
                    <img src={localCover} alt={releaseDetail.title} />
                  {/if}
                </div>
                <dl class="meta-grid">
                  {#if releaseDetail.year}
                    <dt>{$_("album.metaYear")}</dt>
                    <dd>{releaseDetail.year}{#if releaseDetail.original_year && releaseDetail.original_year !== releaseDetail.year}
                      <span class="dim"> ({$_("album.originalYear", { default: "original" })}: {releaseDetail.original_year})</span>
                    {/if}</dd>
                  {/if}
                  {#if releaseDetail.label}
                    <dt>{$_("album.metaLabel")}</dt>
                    <dd>{releaseDetail.label}</dd>
                  {/if}
                  {#if releaseDetail.catalog_number}
                    <dt>{$_("album.metaCatalog")}</dt>
                    <dd>{releaseDetail.catalog_number}</dd>
                  {/if}
                  {#if releaseDetail.country}
                    <dt>{$_("album.metaCountry")}</dt>
                    <dd>{releaseDetail.country}</dd>
                  {/if}
                  {#if releaseDetail.format}
                    <dt>{$_("album.metaFormat")}</dt>
                    <dd>{releaseDetail.format}</dd>
                  {/if}
                  {#if releaseDetail.packaging}
                    <dt>{$_("album.metaPackaging")}</dt>
                    <dd>{releaseDetail.packaging}</dd>
                  {/if}
                  {#if releaseDetail.barcode}
                    <dt>{$_("album.metaBarcode")}</dt>
                    <dd class="mono">{releaseDetail.barcode}</dd>
                  {/if}
                  {#if releaseDetail.language}
                    <dt>{$_("album.metaLanguage")}</dt>
                    <dd>{releaseDetail.language}</dd>
                  {/if}
                  {#if releaseDetail.release_type}
                    <dt>{$_("album.metaType")}</dt>
                    <dd>{releaseDetail.release_type}</dd>
                  {/if}
                  {#if releaseDetail.track_count > 0}
                    <dt>{$_("album.metaTrackCount")}</dt>
                    <dd>{releaseDetail.track_count}{#if releaseDetail.total_duration_ms}
                      <span class="dim"> · {formatTotalDuration(releaseDetail.total_duration_ms)}</span>
                    {/if}</dd>
                  {/if}
                </dl>
              </div>

              <section class="album-story">
                <h3 class="story-heading">
                  <svg viewBox="0 0 24 24" width="16" height="16" fill="currentColor" aria-hidden="true">
                    <path d="M14 2H6c-1.1 0-2 .9-2 2v16c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V8l-6-6zM6 20V4h7v5h5v11H6z"/>
                  </svg>
                  {$_("album.story")}
                </h3>
                {#if releaseDetail.wiki_extract}
                  <p class="story-text">{releaseDetail.wiki_extract}</p>
                {:else}
                  <p class="story-empty">{$_("album.noStory")}</p>
                {/if}
              </section>

              {#if releaseDetail.wikipedia_url}
                <a
                  class="ext-link"
                  href={releaseDetail.wikipedia_url}
                  target="_blank"
                  rel="noopener noreferrer"
                >
                  <svg viewBox="0 0 24 24" fill="currentColor" width="14" height="14">
                    <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-1 15v-4H7l5-8v4h4l-5 8z"/>
                  </svg>
                  {$_("album.readOnWikipedia")}
                </a>
              {/if}

              {#if releaseDetail.tracks.length > 0}
                <section class="info-section">
                  <h3 class="info-heading">
                    {$_("album.tracklist")}
                    <span class="dim">({releaseDetail.tracks.length})</span>
                  </h3>
                  <table class="tracklist">
                    <thead>
                      <tr>
                        <th class="col-pos">#</th>
                        <th class="col-title">{$_("album.colTitle")}</th>
                        <th class="col-len">{$_("album.colLength")}</th>
                        <th class="col-mb"></th>
                      </tr>
                    </thead>
                    <tbody>
                      {#each releaseDetail.tracks as track}
                        <tr>
                          <td class="col-pos">
                            {#if track.disc_number > 1}
                              {track.disc_number}.{track.position}
                            {:else}
                              {track.position}
                            {/if}
                          </td>
                          <td class="col-title">
                            <div class="track-title">{track.title}</div>
                            {#if track.artist_credit}
                              <div class="track-credit dim">{track.artist_credit}</div>
                            {/if}
                          </td>
                          <td class="col-len">{formatDuration(track.length_ms)}</td>
                          <td class="col-mb">
                            <a
                              class="ext-link"
                              href={mbRecordingUrl(track.mbid)}
                              target="_blank"
                              rel="noopener noreferrer"
                              title={$_("album.viewOnMusicBrainz")}
                              aria-label={$_("album.viewOnMusicBrainz")}
                            >
                              <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor">
                                <path d="M14 3v2h3.59l-9.83 9.83 1.41 1.41L19 6.41V10h2V3h-7zM19 19H5V5h7V3H5c-1.11 0-2 .9-2 2v14c0 1.1.89 2 2 2h14c1.1 0 2-.9 2-2v-7h-2v7z"/>
                              </svg>
                            </a>
                          </td>
                        </tr>
                      {/each}
                    </tbody>
                  </table>
                </section>
              {/if}

              <div class="album-footer">
                <a
                  class="ext-link"
                  href={mbReleaseUrl(releaseDetail.mbid)}
                  target="_blank"
                  rel="noopener noreferrer"
                >
                  {$_("album.viewReleaseOnMusicBrainz")}
                </a>
                <span class="attribution">{$_("album.dataFromMusicBrainz")}</span>
              </div>
            {:else}
              <p class="dim">{$_("album.noInfoFound")}</p>
            {/if}
          </section>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.7);
    backdrop-filter: blur(6px);
    z-index: 1000;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
  }
  .modal.info-modal {
    background: var(--bg-elevated, #181818);
    border-radius: 12px;
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.6);
    max-width: 720px;
    width: 100%;
    max-height: calc(100vh - 48px);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border: 1px solid var(--border-color, rgba(255, 255, 255, 0.08));
  }
  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 20px 24px 12px;
    flex-shrink: 0;
    border-bottom: 1px solid var(--border-color, rgba(255, 255, 255, 0.06));
  }
  .modal-title h2 {
    font-size: 18px;
    font-weight: 700;
    margin: 0;
    line-height: 1.3;
  }
  .modal-title .subtitle {
    margin: 4px 0 0;
    font-size: 13px;
    color: var(--text-2, #b3b3b3);
  }
  .header-actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .icon-btn {
    background: transparent;
    border: 0;
    color: var(--text-2, #b3b3b3);
    cursor: pointer;
    padding: 8px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: background 0.15s, color 0.15s;
    line-height: 1;
    font-size: 16px;
  }
  .icon-btn:hover {
    background: var(--bg-highlight, #3e3e3e);
    color: var(--text, #fff);
  }
  .tabs {
    display: flex;
    gap: 4px;
    padding: 8px 24px 0;
    border-bottom: 1px solid var(--border-color, rgba(255, 255, 255, 0.06));
    flex-shrink: 0;
  }
  .tab {
    background: transparent;
    border: 0;
    color: var(--text-2, #b3b3b3);
    cursor: pointer;
    padding: 10px 14px;
    font-size: 14px;
    font-weight: 500;
    border-bottom: 2px solid transparent;
    margin-bottom: -1px;
    transition: color 0.15s, border-color 0.15s;
  }
  .tab:hover { color: var(--text, #fff); }
  .tab.active {
    color: var(--text, #fff);
    border-bottom-color: var(--accent-primary, #1DB954);
  }
  .modal-body {
    overflow-y: auto;
    padding: 20px 24px 24px;
    flex: 1 1 auto;
  }
  .tab-panel {
    display: flex;
    flex-direction: column;
    gap: 20px;
  }
  .loading {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 32px;
    color: var(--text-2, #b3b3b3);
  }
  .spinner {
    width: 20px;
    height: 20px;
    border: 2px solid var(--bg-highlight, #3e3e3e);
    border-top-color: var(--accent-primary, #1DB954);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg); } }
  .error-msg {
    color: var(--error-color, #f15e6c);
    padding: 16px;
    background: rgba(241, 94, 108, 0.08);
    border-radius: 8px;
  }
  .info-section { display: flex; flex-direction: column; gap: 10px; }
  .info-heading {
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-2, #b3b3b3);
    margin: 0;
  }
  .genre-pills { display: flex; flex-wrap: wrap; gap: 6px; }
  .genre-pill {
    padding: 4px 10px;
    border-radius: 999px;
    background: var(--bg-surface, #282828);
    color: var(--text, #fff);
    font-size: 12px;
  }
  .bio-text {
    font-size: 14px;
    line-height: 1.6;
    color: var(--text, #fff);
    margin: 0;
  }
  .disambiguation {
    font-size: 12px;
    color: var(--text-3, #6a6a6a);
    font-style: italic;
    margin: 0;
  }
  .ext-link {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--accent-primary, #1DB954);
    text-decoration: none;
    font-size: 13px;
    transition: opacity 0.15s;
  }
  .ext-link:hover { opacity: 0.85; text-decoration: underline; }
  .similar-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(120px, 1fr));
    gap: 12px;
  }
  .artist-initial-sm {
    width: 100%; aspect-ratio: 1;
    display: flex; align-items: center; justify-content: center;
    background: linear-gradient(135deg, #2a2a2a, #3a3a3a);
    border-radius: 50%;
    font-size: 28px;
    font-weight: 600;
    color: var(--text-2, #b3b3b3);
  }
  .in-library-dot { color: var(--accent-primary, #1DB954); margin-left: 4px; }
  .attribution {
    font-size: 11px;
    color: var(--text-3, #6a6a6a);
    text-align: right;
    margin: 0;
  }
  .album-grid {
    display: grid;
    grid-template-columns: 140px 1fr;
    gap: 20px;
    align-items: start;
  }
  .album-cover {
    aspect-ratio: 1;
    border-radius: 8px;
    overflow: hidden;
    background: var(--bg-surface, #282828);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
  }
  .album-cover img {
    width: 100%; height: 100%; object-fit: cover; display: block;
  }
  .meta-grid {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 8px 14px;
    margin: 0;
    font-size: 13px;
  }
  .meta-grid dt {
    color: var(--text-2, #b3b3b3);
    font-weight: 500;
  }
  .meta-grid dd {
    margin: 0;
    color: var(--text, #fff);
  }
  .meta-grid dd.mono { font-family: ui-monospace, "SF Mono", Menlo, monospace; }
  .dim { color: var(--text-3, #6a6a6a); }

  .album-story {
    background: var(--bg-surface, #282828);
    border-radius: 10px;
    padding: 16px 18px;
    border-left: 3px solid var(--accent-primary, #1DB954);
  }
  .story-heading {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 1px;
    color: var(--accent-primary, #1DB954);
    margin: 0 0 10px;
  }
  .story-text {
    font-size: 15px;
    line-height: 1.7;
    color: var(--text, #fff);
    margin: 0;
  }
  .story-empty {
    font-size: 13px;
    color: var(--text-3, #6a6a6a);
    font-style: italic;
    margin: 0;
  }
  .tracklist {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
  }
  .tracklist thead th {
    text-align: left;
    padding: 8px 6px;
    font-weight: 500;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-2, #b3b3b3);
    border-bottom: 1px solid var(--border-color, rgba(255, 255, 255, 0.06));
  }
  .tracklist thead th.col-pos { width: 48px; }
  .tracklist thead th.col-len { width: 56px; text-align: right; }
  .tracklist thead th.col-mb { width: 28px; }
  .tracklist tbody td {
    padding: 8px 6px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.04);
    vertical-align: top;
  }
  .tracklist tbody tr:last-child td { border-bottom: none; }
  .tracklist .col-pos { color: var(--text-2, #b3b3b3); font-variant-numeric: tabular-nums; }
  .tracklist .col-len { text-align: right; color: var(--text-2, #b3b3b3); font-variant-numeric: tabular-nums; }
  .tracklist .col-mb { text-align: right; }
  .track-title { color: var(--text, #fff); }
  .track-credit { font-size: 11px; margin-top: 2px; }
  .album-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding-top: 8px;
    border-top: 1px solid var(--border-color, rgba(255, 255, 255, 0.06));
  }
  .artist-link {
    background: none;
    border: 0;
    color: var(--text-2, #b3b3b3);
    cursor: pointer;
    padding: 0;
    font: inherit;
    text-decoration: none;
  }
  .artist-link:hover { color: var(--text, #fff); text-decoration: underline; }

  @media (max-width: 600px) {
    .modal-backdrop { padding: 0; }
    .modal.info-modal { max-height: 100vh; border-radius: 0; }
    .album-grid { grid-template-columns: 100px 1fr; gap: 14px; }
  }
</style>
