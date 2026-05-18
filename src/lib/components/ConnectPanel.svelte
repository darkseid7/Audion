<script lang="ts">
  import { fade, fly, slide } from "svelte/transition";
  import {
    wsStore,
    type RemoteDevice,
    activeRemoteDevice,
  } from "$lib/stores/websocket";
  import {
    currentTrack,
    isPlaying,
    transferPlayback,
    sendRemoteCommand,
    activeBackend,
  } from "$lib/stores/player";
  import { isLoggedIn } from "$lib/stores/sync";
  import { appSettings } from "$lib/stores/settings";
  import {
    tracks as libraryTracks,
    getTrackByIdSync,
  } from "$lib/stores/library";
  import { getTrackCoverSrc } from "$lib/api/tauri";
  import {
    lmsDiscoverServers,
    lmsConnect,
    lmsDisconnect,
    lmsIsConnected,
    lmsGetPlayers,
    lmsGetPlayerStatus,
    lmsPlay,
    lmsPause,
    lmsResume,
    lmsStop,
    lmsNext,
    lmsPrevious,
    lmsSetVolume,
    lmsPlayTracks,
    lmsSubscribeStatus,
    lmsUnsubscribeStatus,
    type LmsServer,
    type LmsPlayer,
    type LmsPlayerStatus,
  } from "$lib/api/tauri";
  import { get } from "svelte/store";
  import { createEventDispatcher, onMount, onDestroy } from "svelte";

  const dispatch = createEventDispatcher();

  // ── LMS Client state ──────────────────────────────────────────────────────
  let lmsConnected = false;
  let lmsServers: LmsServer[] = [];
  let lmsPlayers: LmsPlayer[] = [];
  let lmsPolling: ReturnType<typeof setInterval> | null = null;
  let discovering = false;
  let activeLmsPlayer: string | null = null;
  let playerStatus: LmsPlayerStatus | null = null;
  let manualHost = "";
  let manualPort = "9000";
  let showManualEntry = false;

  onMount(async () => {
    try {
      lmsConnected = await lmsIsConnected();
      if (lmsConnected) {
        startLmsPolling();
      }
    } catch {}
  });

  onDestroy(() => {
    if (lmsPolling) clearInterval(lmsPolling);
  });

  function startLmsPolling() {
    if (lmsPolling) return;
    pollLmsPlayers();
    lmsPolling = setInterval(pollLmsPlayers, 2000);
  }

  function stopLmsPolling() {
    if (lmsPolling) {
      clearInterval(lmsPolling);
      lmsPolling = null;
    }
  }

  async function pollLmsPlayers() {
    try {
      lmsPlayers = await lmsGetPlayers();
      if (activeLmsPlayer) {
        playerStatus = await lmsGetPlayerStatus(activeLmsPlayer);
      }
    } catch {}
  }

  async function discoverServers() {
    discovering = true;
    try {
      lmsServers = await lmsDiscoverServers();
    } catch (e) {
      console.error("LMS discovery error:", e);
    }
    discovering = false;
  }

  async function connectToServer(server: LmsServer) {
    try {
      await lmsConnect(server.host, server.json_port);
      lmsConnected = true;
      startLmsPolling();
    } catch (e) {
      console.error("LMS connect error:", e);
    }
  }

  async function connectManual() {
    if (!manualHost) return;
    try {
      await lmsConnect(manualHost, parseInt(manualPort) || 9000);
      lmsConnected = true;
      startLmsPolling();
    } catch (e) {
      console.error("LMS connect error:", e);
    }
  }

  async function disconnectLms() {
    try {
      await lmsUnsubscribeStatus();
      await lmsDisconnect();
    } catch {}
    lmsConnected = false;
    lmsPlayers = [];
    activeLmsPlayer = null;
    playerStatus = null;
    stopLmsPolling();
    activeBackend.set("none");
  }

  function selectLmsPlayer(player: LmsPlayer) {
    if (activeLmsPlayer === player.player_id) {
      activeLmsPlayer = null;
      playerStatus = null;
      activeBackend.set("none");
    } else {
      activeLmsPlayer = player.player_id;
      activeBackend.set("lms" as any);
      activeRemoteDevice.set(null);
      lmsSubscribeStatus(player.player_id).catch(() => {});
    }
  }

  async function handleLmsCommand(playerId: string, cmd: string) {
    try {
      switch (cmd) {
        case "play": await lmsResume(playerId); break;
        case "pause": await lmsPause(playerId); break;
        case "stop": await lmsStop(playerId); break;
        case "next": await lmsNext(playerId); break;
        case "previous": await lmsPrevious(playerId); break;
      }
    } catch (e) {
      console.error("LMS command error:", e);
    }
  }

  async function handleLmsVolume(playerId: string, e: Event) {
    const target = e.target as HTMLInputElement;
    const vol = parseInt(target.value);
    try {
      await lmsSetVolume(playerId, vol);
    } catch {}
  }

  async function playOnLmsPlayer(playerId: string) {
    const $library = get(libraryTracks);
    const $current = get(currentTrack);
    if (!$current) return;

    const trackIds = $library
      .filter((t: any) => t.path)
      .map((t: any) => t.id);

    const currentIndex = trackIds.indexOf($current.id);
    if (currentIndex === -1) return;

    // Send a window of up to 100 tracks starting from current
    const queueWindow = trackIds.slice(currentIndex, currentIndex + 100);

    try {
      await lmsPlayTracks(playerId, queueWindow, 0);
    } catch (e) {
      console.error("LMS play error:", e);
    }
  }

  // Deduplication and sorting (active device first)
  $: devices = $wsStore.devices
    .filter(
      (device, index, self) =>
        index === self.findIndex((t) => t.deviceId === device.deviceId),
    )
    .sort((a, b) => {
      if (a.deviceId === $activeRemoteDevice) return -1;
      if (b.deviceId === $activeRemoteDevice) return 1;
      return 0;
    });

  function close() {
    dispatch("close");
  }

  function handleTransfer(device: RemoteDevice) {
    if (device.playerState) {
      transferPlayback(device.playerState);
      close();
    }
  }

  function handleRemoteCommand(deviceId: string, command: string) {
    sendRemoteCommand(deviceId, command);
  }

  function toggleControl(device: RemoteDevice) {
    if (
      $activeBackend === "remote" &&
      $activeRemoteDevice === device.deviceId
    ) {
      activeBackend.set("none");
      activeRemoteDevice.set(null);
    } else {
      activeBackend.set("remote");
      activeRemoteDevice.set(device.deviceId);

      if (device.playerState && device.playerState.track) {
        const remoteTrack = device.playerState.track;
        const remotePlaying = device.playerState.isPlaying;
        const remoteTrackId = Number(remoteTrack.id);

        let localTrack: any = getTrackByIdSync(remoteTrackId);
        if (!localTrack) {
          const $library = get(libraryTracks);
          localTrack = $library.find(
            (t) =>
              t.title === remoteTrack.title && t.artist === remoteTrack.artist,
          );
        }

        currentTrack.set({
          ...remoteTrack,
          ...(localTrack || {}),
          id: remoteTrackId,
          track_cover: localTrack
            ? getTrackCoverSrc(localTrack)
            : remoteTrack.coverUrl,
        } as any);

        isPlaying.set(remotePlaying);
      }
    }
  }
</script>

<div
  class="connect-overlay"
  on:click|self={close}
  on:keydown|self={(e) => e.key === "Escape" && close()}
  transition:fade={{ duration: 250 }}
  role="presentation"
>
  <div
    class="connect-panel glass"
    in:fly={{ y: 30, duration: 400, opacity: 0 }}
    out:fly={{ y: 20, duration: 200, opacity: 0 }}
  >
    <header>
      <div class="title-wrap">
        <h2>Connect to a device</h2>
        <div class="sync-pill" class:online={$wsStore.connected}>
          <div class="dot"></div>
          <span>{$wsStore.connected ? "Cloud Active" : "Offline"}</span>
        </div>
      </div>
      <button class="close-btn" on:click={close} aria-label="Close">
        <svg
          viewBox="0 0 24 24"
          width="20"
          height="20"
          fill="none"
          stroke="currentColor"
          stroke-width="2.5"
        >
          <path d="M18 6L6 18M6 6l12 12" />
        </svg>
      </button>
    </header>

    <div class="session-section">
      <div class="status-card" class:remote={$activeBackend === "remote"}>
        <div class="device-icon-glow">
          <svg viewBox="0 0 24 24" width="24" height="24" fill="currentColor">
            <path
              d="M20 18c1.1 0 1.99-.9 1.99-2L22 6c0-1.1-.9-2-2-2H4c-1.1 0-2 .9-2 2v10c0 1.1.9 2 2 2H0v2h24v-2h-4zM4 6h16v10H4V6z"
            />
          </svg>
        </div>
        <div class="status-info">
          {#if $activeBackend === "remote"}
            <span class="label">Controlling Remote</span>
            <span class="value">Active Session</span>
          {:else}
            <span class="label">Local Playback</span>
            <span class="value">This Device</span>
          {/if}
        </div>
        {#if $isPlaying || $activeBackend === "remote"}
          <div class="playing-indicator">
            <span></span><span></span><span></span>
          </div>
        {/if}
      </div>
    </div>

    <!-- LMS Client Section -->
    <div class="device-section">
      <div class="section-header">
        <span>LMS Players</span>
        <div class="line"></div>
        {#if lmsConnected}
          <button class="squeeze-toggle active" on:click={disconnectLms}>
            Disconnect
          </button>
        {:else}
          <button
            class="squeeze-toggle"
            on:click={discoverServers}
            disabled={discovering}
          >
            {discovering ? '...' : 'Discover'}
          </button>
        {/if}
      </div>

      {#if lmsConnected}
        <div class="device-grid">
          {#if lmsPlayers.length === 0}
            <div class="empty-state compact" in:fade>
              <p>No players found</p>
              <span>Ensure your Squeeze players are connected to LMS.</span>
            </div>
          {:else}
            {#each lmsPlayers as player (player.player_id)}
              <div
                class="device-card"
                class:active={activeLmsPlayer === player.player_id}
                in:fly={{ y: 20, duration: 300 }}
              >
                <div class="card-main">
                  <div class="platform-icon squeeze-icon">
                    <svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor">
                      <path d="M12 3v10.55c-.59-.34-1.27-.55-2-.55C7.79 13 6 14.79 6 17s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z"/>
                    </svg>
                  </div>
                  <div class="card-details">
                    <span class="device-name">{player.name}</span>
                    {#if activeLmsPlayer === player.player_id && playerStatus?.current_track}
                      <div class="track-info">
                        <span class="dot" class:playing={playerStatus.mode === 'playing'}></span>
                        <span class="track-text">{playerStatus.current_track.title} — {playerStatus.current_track.artist}</span>
                      </div>
                    {:else}
                      <span class="idle-text">{player.is_playing ? 'Playing' : player.connected ? 'Ready' : 'Offline'}</span>
                    {/if}
                  </div>

                  {#if activeLmsPlayer === player.player_id && playerStatus}
                    <div class="mini-controls">
                      <button class="icon-btn" on:click|stopPropagation={() => handleLmsCommand(player.player_id, 'previous')}>
                        <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor"><path d="M6 6h2v12H6zm3.5 6l8.5 6V6z"/></svg>
                      </button>
                      <button class="icon-btn highlight" on:click|stopPropagation={() => handleLmsCommand(player.player_id, playerStatus?.mode === 'playing' ? 'pause' : 'play')}>
                        {#if playerStatus.mode === 'playing'}
                          <svg viewBox="0 0 24 24" width="16" height="16" fill="currentColor"><path d="M6 19h4V5H6v14zm8-14v14h4V5h-4z"/></svg>
                        {:else}
                          <svg viewBox="0 0 24 24" width="16" height="16" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>
                        {/if}
                      </button>
                      <button class="icon-btn" on:click|stopPropagation={() => handleLmsCommand(player.player_id, 'next')}>
                        <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor"><path d="M6 18l8.5-6L6 6v12zM16 6v12h2V6h-2z"/></svg>
                      </button>
                    </div>
                  {/if}
                </div>

                {#if activeLmsPlayer === player.player_id && playerStatus}
                  <!-- Volume slider -->
                  <div class="squeeze-volume">
                    <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor" opacity="0.5">
                      <path d="M3 9v6h4l5 5V4L7 9H3zm13.5 3c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02z"/>
                    </svg>
                    <input
                      type="range"
                      min="0" max="100"
                      value={playerStatus.volume}
                      on:input={(e) => handleLmsVolume(player.player_id, e)}
                      class="volume-slider"
                    />
                  </div>
                {/if}

                <div class="card-actions">
                  <button
                    class="btn secondary"
                    class:active={activeLmsPlayer === player.player_id}
                    on:click={() => selectLmsPlayer(player)}
                  >
                    {activeLmsPlayer === player.player_id ? 'Disconnect' : 'Control'}
                  </button>
                  <button
                    class="btn primary"
                    on:click={() => playOnLmsPlayer(player.player_id)}
                  >
                    Play Here
                  </button>
                </div>
              </div>
            {/each}
          {/if}
        </div>
      {:else}
        <div class="device-grid">
          {#if lmsServers.length > 0}
            {#each lmsServers as server (server.uuid || server.host)}
              <div class="device-card" in:fly={{ y: 20, duration: 300 }}>
                <div class="card-main">
                  <div class="platform-icon squeeze-icon">
                    <svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor">
                      <path d="M20 4H4c-1.1 0-2 .9-2 2v12c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V6c0-1.1-.9-2-2-2zm0 14H4V6h16v12z"/>
                    </svg>
                  </div>
                  <div class="card-details">
                    <span class="device-name">{server.name}</span>
                    <span class="idle-text">{server.host}:{server.json_port}</span>
                  </div>
                </div>
                <div class="card-actions">
                  <button class="btn primary" style="grid-column: span 2" on:click={() => connectToServer(server)}>
                    Connect
                  </button>
                </div>
              </div>
            {/each}
          {:else}
            <div class="empty-state compact" in:fade>
              <p>No LMS servers found</p>
              <span>Click Discover or enter server address manually.</span>
            </div>
          {/if}

          <!-- Manual entry -->
          <div class="manual-entry">
            <button class="text-btn" on:click={() => showManualEntry = !showManualEntry}>
              {showManualEntry ? 'Hide' : 'Manual connection'}
            </button>
            {#if showManualEntry}
              <div class="manual-form" transition:slide={{ duration: 200 }}>
                <input type="text" bind:value={manualHost} placeholder="IP address (e.g. 192.168.1.100)" class="manual-input" />
                <input type="text" bind:value={manualPort} placeholder="Port" class="manual-input port" />
                <button class="btn primary" on:click={connectManual} disabled={!manualHost}>Connect</button>
              </div>
            {/if}
          </div>
        </div>
      {/if}
    </div>

    <div class="device-section">
      <div class="section-header">
        <span>Cloud Devices</span>
        <div class="line"></div>
      </div>

      <div class="device-grid">
        {#if devices.length === 0}
          <div class="empty-state" in:fade>
            <div class="empty-icon">
              <svg
                viewBox="0 0 24 24"
                width="32"
                height="32"
                fill="none"
                stroke="currentColor"
                stroke-width="1.5"
              >
                <path
                  d="M12 18.5a6.5 6.5 0 100-13 6.5 6.5 0 000 13zM12 9v3m0 3h.01"
                  stroke-linecap="round"
                />
              </svg>
            </div>
            <p>No other devices discovered</p>
            <span>Ensure Audion is open on your other devices.</span>
          </div>
        {:else if !$appSettings.remoteControlEnabled}
          <div class="empty-state" in:fade>
            <div class="empty-icon">
              <svg
                viewBox="0 0 24 24"
                width="32"
                height="32"
                fill="none"
                stroke="currentColor"
                stroke-width="1.5"
              >
                <path
                  d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"
                  stroke-linecap="round"
                />
              </svg>
            </div>
            <p>Remote Control Disabled</p>
            <span
              >Enable "Remote Control & Device Connection" in settings to use
              this feature.</span
            >
          </div>
        {:else}
          {#each devices as device (device.deviceId)}
            <div
              class="device-card"
              class:active={$activeRemoteDevice === device.deviceId}
              in:fly={{ y: 20, duration: 300 }}
            >
              <div class="card-main">
                <div class="platform-icon">
                  {#if device.deviceName
                    .toLowerCase()
                    .includes("phone") || device.deviceName
                      .toLowerCase()
                      .includes("android") || device.deviceName
                      .toLowerCase()
                      .includes("iphone")}
                    <svg
                      viewBox="0 0 24 24"
                      width="20"
                      height="20"
                      fill="currentColor"
                      ><path
                        d="M17 1.01L7 1c-1.1 0-2 .9-2 2v18c0 1.1.9 2 2 2h10c1.1 0 2-.9 2-2V3c0-1.1-.9-1.99-2-1.99zM17 19H7V5h10v14z"
                      /></svg
                    >
                  {:else}
                    <svg
                      viewBox="0 0 24 24"
                      width="20"
                      height="20"
                      fill="currentColor"
                      ><path
                        d="M20 18c1.1 0 1.99-.9 1.99-2L22 6c0-1.1-.9-2-2-2H4c-1.1 0-2 .9-2 2v10c0 1.1.9 2 2 2H0v2h24v-2h-4zM4 6h16v10H4V6z"
                      /></svg
                    >
                  {/if}
                </div>
                <div class="card-details">
                  <span class="device-name">{device.deviceName}</span>
                  {#if device.playerState?.track}
                    <div class="track-info">
                      <span
                        class="dot"
                        class:playing={device.playerState.isPlaying}
                      ></span>
                      <span class="track-text"
                        >{device.playerState.track.title}</span
                      >
                    </div>
                  {:else}
                    <span class="idle-text">Ready to stream</span>
                  {/if}
                </div>

                {#if device.playerState?.track}
                  <div class="mini-controls">
                    <button
                      class="icon-btn"
                      on:click|stopPropagation={() =>
                        handleRemoteCommand(device.deviceId, "previous")}
                    >
                      <svg
                        viewBox="0 0 24 24"
                        width="14"
                        height="14"
                        fill="currentColor"
                        ><path d="M6 6h2v12H6zm3.5 6l8.5 6V6z" /></svg
                      >
                    </button>
                    <button
                      class="icon-btn highlight"
                      on:click|stopPropagation={() =>
                        handleRemoteCommand(
                          device.deviceId,
                          device.playerState?.isPlaying ? "pause" : "play",
                        )}
                    >
                      {#if device.playerState?.isPlaying}
                        <svg
                          viewBox="0 0 24 24"
                          width="16"
                          height="16"
                          fill="currentColor"
                          ><path d="M6 19h4V5H6v14zm8-14v14h4V5h-4z" /></svg
                        >
                      {:else}
                        <svg
                          viewBox="0 0 24 24"
                          width="16"
                          height="16"
                          fill="currentColor"><path d="M8 5v14l11-7z" /></svg
                        >
                      {/if}
                    </button>
                    <button
                      class="icon-btn"
                      on:click|stopPropagation={() =>
                        handleRemoteCommand(device.deviceId, "next")}
                    >
                      <svg
                        viewBox="0 0 24 24"
                        width="14"
                        height="14"
                        fill="currentColor"
                        ><path d="M6 18l8.5-6L6 6v12zM16 6v12h2V6h-2z" /></svg
                      >
                    </button>
                  </div>
                {/if}
              </div>

              {#if device.playerState?.track}
                <div class="card-actions">
                  <button
                    class="btn secondary"
                    class:active={$activeRemoteDevice === device.deviceId}
                    on:click={() => toggleControl(device)}
                  >
                    {$activeRemoteDevice === device.deviceId
                      ? "Stop Control"
                      : "Remote Control"}
                  </button>
                  <button
                    class="btn primary"
                    on:click={() => handleTransfer(device)}
                  >
                    Play Here
                  </button>
                </div>
              {/if}
            </div>
          {/each}
        {/if}
      </div>
    </div>

    <footer class="glass-footer">
      <div class="footer-content">
        {#if !$isLoggedIn}
          <div class="warning-box">
            <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor"
              ><path
                d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm1 15h-2v-2h2v2zm0-4h-2V7h2v6z"
              /></svg
            >
            <span>Guest mode: Sync limited to local network</span>
          </div>
        {/if}
        <div class="server-status">
          <span class="status-msg">{$wsStore.statusText}</span>
          {#if !$wsStore.connected}
            <button class="text-btn" on:click={() => wsStore.connect()}
              >Retry Connection</button
            >
          {/if}
        </div>
      </div>
    </footer>
  </div>
</div>

<style>
  .connect-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.4);
    backdrop-filter: blur(12px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 2000;
  }

  .glass {
    background: rgba(22, 22, 22, 0.75);
    backdrop-filter: blur(20px);
    border: 1px solid rgba(255, 255, 255, 0.1);
    box-shadow:
      0 20px 50px rgba(0, 0, 0, 0.5),
      inset 0 1px 1px rgba(255, 255, 255, 0.05);
  }

  .connect-panel {
    width: 480px;
    max-width: 92vw;
    border-radius: 28px;
    padding: 24px;
    display: flex;
    flex-direction: column;
    gap: 24px;
    max-height: 85vh;
  }

  header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
  }

  .title-wrap h2 {
    font-size: 1.5rem;
    font-weight: 850;
    margin: 0 0 8px 0;
    letter-spacing: -0.02em;
    background: linear-gradient(to bottom, #fff, #999);
    -webkit-background-clip: text;
    -webkit-text-fill-color: transparent;
  }

  .sync-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    background: rgba(255, 255, 255, 0.05);
    border-radius: 99px;
    font-size: 0.7rem;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    color: #888;
  }

  .sync-pill.online {
    color: var(--accent-primary);
    background: color-mix(in srgb, var(--accent-primary), transparent 90%);
  }

  .sync-pill .dot {
    width: 6px;
    height: 6px;
    background: currentColor;
    border-radius: 50%;
  }

  .close-btn {
    background: rgba(255, 255, 255, 0.05);
    border: none;
    color: #999;
    width: 36px;
    height: 36px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    transition: 0.2s;
  }

  .close-btn:hover {
    background: rgba(255, 255, 255, 0.15);
    color: white;
    transform: rotate(90deg);
  }

  .status-card {
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid rgba(255, 255, 255, 0.05);
    border-radius: 20px;
    padding: 16px;
    display: flex;
    align-items: center;
    gap: 16px;
    position: relative;
    overflow: hidden;
  }

  .status-card.remote {
    background: color-mix(in srgb, var(--accent-primary), transparent 92%);
    border-color: color-mix(in srgb, var(--accent-primary), transparent 80%);
  }

  .device-icon-glow {
    width: 48px;
    height: 48px;
    background: rgba(255, 255, 255, 0.05);
    color: white;
    border-radius: 14px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .remote .device-icon-glow {
    background: var(--accent-primary);
    color: black;
    box-shadow: 0 0 20px color-mix(in srgb, var(--accent-primary), transparent 70%);
  }

  .status-info {
    display: flex;
    flex-direction: column;
  }

  .status-info .label {
    font-size: 0.75rem;
    color: #777;
    font-weight: 600;
  }

  .remote .status-info .label {
    color: var(--accent-primary);
  }

  .status-info .value {
    font-size: 1.1rem;
    font-weight: 700;
    color: white;
  }

  .playing-indicator {
    margin-left: auto;
    display: flex;
    align-items: flex-end;
    gap: 3px;
    height: 18px;
  }

  .playing-indicator span {
    width: 3px;
    background: var(--accent-primary);
    animation: bar-up 0.6s infinite alternate;
  }

  @keyframes bar-up {
    from {
      height: 4px;
    }
    to {
      height: 18px;
    }
  }
  .playing-indicator span:nth-child(2) {
    animation-delay: 0.2s;
  }
  .playing-indicator span:nth-child(3) {
    animation-delay: 0.4s;
  }

  .section-header {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 16px;
  }

  .section-header span {
    font-size: 0.75rem;
    font-weight: 800;
    text-transform: uppercase;
    color: #555;
    letter-spacing: 0.08em;
  }

  .section-header .line {
    flex: 1;
    height: 1px;
    background: linear-gradient(to right, #222, transparent);
  }

  .device-grid {
    display: flex;
    flex-direction: column;
    gap: 12px;
    overflow-y: auto;
    padding-right: 4px;
  }

  .device-grid::-webkit-scrollbar {
    width: 4px;
  }
  .device-grid::-webkit-scrollbar-thumb {
    background: rgba(255, 255, 255, 0.1);
    border-radius: 10px;
  }

  .device-card {
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid rgba(255, 255, 255, 0.05);
    border-radius: 20px;
    padding: 16px;
    transition: 0.3s;
  }

  .device-card:hover {
    background: rgba(255, 255, 255, 0.06);
    transform: translateY(-2px);
  }

  .device-card.active {
    background: color-mix(in srgb, var(--accent-primary), transparent 95%);
    border-color: color-mix(in srgb, var(--accent-primary), transparent 70%);
  }

  .card-main {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-bottom: 16px;
  }

  .platform-icon {
    width: 40px;
    height: 40px;
    border-radius: 12px;
    background: rgba(255, 255, 255, 0.04);
    display: flex;
    align-items: center;
    justify-content: center;
    color: #888;
  }

  .active .platform-icon {
    color: var(--accent-primary);
    background: color-mix(in srgb, var(--accent-primary), transparent 90%);
  }

  .card-details {
    flex: 1;
    min-width: 0;
  }

  .device-name {
    display: block;
    font-weight: 700;
    font-size: 1rem;
    color: white;
    margin-bottom: 2px;
  }

  .track-info {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 0.8rem;
    color: #666;
  }

  .track-info .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: #444;
  }

  .track-info .dot.playing {
    background: var(--accent-primary);
    box-shadow: 0 0 8px var(--accent-primary);
  }

  .track-text {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .idle-text {
    font-size: 0.75rem;
    color: #444;
  }

  .mini-controls {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .icon-btn {
    background: transparent;
    border: none;
    color: #666;
    width: 28px;
    height: 28px;
    border-radius: 8px;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    transition: 0.2s;
  }

  .icon-btn:hover {
    background: rgba(255, 255, 255, 0.05);
    color: white;
  }
  .icon-btn.highlight {
    border: 1px solid rgba(255, 255, 255, 0.1);
  }

  .card-actions {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }

  .btn {
    padding: 10px;
    border-radius: 12px;
    font-size: 0.85rem;
    font-weight: 700;
    cursor: pointer;
    transition: 0.2s;
    display: flex;
    align-items: center;
    justify-content: center;
    border: none;
  }

  .btn.primary {
    background: var(--accent-primary);
    color: black;
  }
  .btn.primary:hover {
    transform: scale(1.02);
    filter: brightness(1.1);
  }

  .btn.secondary {
    background: rgba(255, 255, 255, 0.05);
    color: white;
    border: 1px solid rgba(255, 255, 255, 0.1);
  }
  .btn.secondary:hover {
    background: rgba(255, 255, 255, 0.1);
  }
  .btn.secondary.active {
    background: rgba(255, 255, 255, 0.15);
    border-color: white;
  }

  footer {
    margin-top: auto;
    padding-top: 16px;
    border-top: 1px solid rgba(255, 255, 255, 0.05);
  }

  .warning-box {
    background: rgba(255, 215, 0, 0.05);
    border: 1px solid rgba(255, 215, 0, 0.15);
    color: #daa520;
    padding: 8px 12px;
    border-radius: 12px;
    font-size: 0.7rem;
    font-weight: 600;
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 12px;
  }

  .server-status {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 0.7rem;
    color: #444;
  }

  .text-btn {
    background: transparent;
    border: none;
    color: var(--accent-primary);
    font-weight: 700;
    cursor: pointer;
    font-size: 0.7rem;
  }

  .empty-state {
    padding: 40px 20px;
    text-align: center;
    color: #444;
  }

  .empty-icon {
    margin-bottom: 12px;
    opacity: 0.3;
  }

  .empty-state p {
    margin: 0 0 4px 0;
    color: #888;
    font-weight: 700;
  }

  /* Squeeze-specific styles */
  .squeeze-toggle {
    padding: 4px 12px;
    border-radius: 8px;
    font-size: 0.7rem;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    cursor: pointer;
    border: 1px solid rgba(255, 255, 255, 0.1);
    background: rgba(255, 255, 255, 0.05);
    color: #888;
    transition: 0.2s;
  }
  .squeeze-toggle:hover {
    background: rgba(255, 255, 255, 0.1);
    color: white;
  }
  .squeeze-toggle.active {
    background: color-mix(in srgb, var(--accent-primary), transparent 85%);
    border-color: color-mix(in srgb, var(--accent-primary), transparent 60%);
    color: var(--accent-primary);
  }
  .squeeze-toggle:disabled {
    opacity: 0.5;
    cursor: wait;
  }

  .squeeze-icon {
    background: color-mix(in srgb, var(--accent-primary), transparent 90%) !important;
    color: var(--accent-primary) !important;
  }

  .squeeze-volume {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 4px;
    margin-bottom: 12px;
  }

  .volume-slider {
    flex: 1;
    height: 4px;
    -webkit-appearance: none;
    appearance: none;
    background: rgba(255, 255, 255, 0.1);
    border-radius: 2px;
    outline: none;
  }
  .volume-slider::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--accent-primary);
    cursor: pointer;
  }

  .empty-state.compact {
    padding: 20px;
  }
  .empty-state.compact p {
    font-size: 0.85rem;
  }

  .manual-entry {
    padding-top: 8px;
    text-align: center;
  }

  .manual-form {
    display: flex;
    gap: 8px;
    margin-top: 8px;
    align-items: center;
  }

  .manual-input {
    flex: 1;
    padding: 8px 12px;
    border-radius: 10px;
    border: 1px solid rgba(255, 255, 255, 0.1);
    background: rgba(255, 255, 255, 0.05);
    color: white;
    font-size: 0.85rem;
    outline: none;
  }
  .manual-input:focus {
    border-color: var(--accent-primary);
  }
  .manual-input.port {
    max-width: 70px;
  }

  @media (max-width: 480px) {
    .card-actions {
      grid-template-columns: 1fr;
    }
    .card-main {
      flex-wrap: wrap;
    }
    .mini-controls {
      order: 3;
      width: 100%;
      justify-content: space-around;
      padding-top: 8px;
      border-top: 1px solid rgba(255, 255, 255, 0.03);
    }
  }
</style>
