<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { createLanSettings } from "$lib/application/desktop/lan-settings";
  const settings = createLanSettings({
    invoke,
    prepare: async () => (await import("$lib/application/desktop/bootstrap")).prepareHostBridge(),
    release: async () => (await import("$lib/application/desktop/bootstrap")).releaseHostBridge(),
  });
  let address = "";
  let port = 9010;
  let control = true;
  let administration = false;
  let copyMessage = "";
  let expiry: ReturnType<typeof setTimeout> | undefined;
  onMount(() => { void settings.refresh(); });
  onDestroy(() => { clearTimeout(expiry); settings.clearInvitation(); });
  async function invite() {
    copyMessage = "";
    await settings.createInvitation();
    clearTimeout(expiry);
    expiry = setTimeout(() => settings.clearInvitation(), 300_000);
  }
  async function copyInvitation() {
    if (!$settings.invitation) return;
    try { await navigator.clipboard.writeText($settings.invitation.encoded); copyMessage = "Invitation copied. It expires after 5 minutes."; }
    catch { copyMessage = "Clipboard unavailable. Scan the QR code instead."; }
  }
</script>
<section class="lan-settings" aria-labelledby="lan-heading">
  <h2 id="lan-heading">LAN controller</h2>
  <div class="host-row">
    <div>
      <h3>Control this PC from your phone</h3>
      <p>Playback and your library stay on this PC. Your phone is a remote, not an audio output.</p>
    </div>
    <button class="host-switch" type="button" role="switch" aria-checked={$settings.host?.enabled ?? false} disabled={$settings.busy || !$settings.host || (!$settings.host.enabled && !address.trim())} aria-describedby="lan-unavailable" on:click={() => $settings.host?.enabled ? settings.disable() : settings.enable(address.trim(), port)}>
      <span class="switch-track" aria-hidden="true"><span></span></span><span>{$settings.busy ? "Working…" : $settings.host?.enabled ? "On" : "Off"}</span>
    </button>
  </div>
  <p id="lan-unavailable" class="availability" aria-live="polite">{$settings.error ? "Host state could not be confirmed. Refresh devices to check native availability." : !$settings.host ? "Checking native host availability…" : $settings.host.enabled ? ($settings.host.ready ? `Listening on ${$settings.host.endpoint}. This PC owns playback.` : "Listener is running, but the desktop coordinator is not ready. Disable and enable hosting to reconnect.") : "Hosting is off. No LAN controller listener is running."}</p>
  {#if $settings.error}<p role="alert">{$settings.error}</p>{/if}
  <div class="connection-fields">
    <label>Private network interface
      <input type="text" bind:value={address} placeholder="192.168.1.20" disabled={$settings.busy || $settings.host?.enabled} aria-describedby="lan-network-note" />
    </label>
    <label>Port<input type="number" min="1" max="65535" bind:value={port} disabled={$settings.busy || $settings.host?.enabled} /></label>
  </div>
  <p id="lan-network-note">Enter this PC’s private IPv4 address. Local network only. Port 9000 remains reserved for Squeeze. No cloud relay, account, or router port forwarding.</p>
  <div class="pairing-section">
    <h3>Pair a phone</h3>
    <p>Invitations expire after 5 minutes and require approval on this PC.</p>
    {#if $settings.invitation}
      <p>Compare this SHA-256 certificate fingerprint on your phone before approving:</p>
      <code class="fingerprint">{$settings.invitation.fingerprint}</code>
      <img class="invitation-qr" alt="Short-lived pairing invitation QR code" width="200" height="200" src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent($settings.invitation.qrSvg)}`} />
    {:else}<div class="invitation-empty">No active invitation. Enable hosting to create one.</div>{/if}
    <div class="actions"><button type="button" disabled={$settings.busy || !$settings.host?.ready} on:click={invite}>Create invitation</button><button type="button" disabled={$settings.busy || !$settings.invitation} on:click={copyInvitation}>Copy invitation</button></div>
    {#if copyMessage}<p aria-live="polite">{copyMessage}</p>{/if}
  </div>
  <div class="pairing-section">
    <h3>Pending approval</h3>
    <button type="button" disabled={$settings.busy} on:click={() => settings.refresh()}>Refresh devices</button>
    {#if !$settings.host?.pending}<p>Device inventory is not loaded. Enable hosting first.</p>{:else if !$settings.host.pending.length}<p>No pending requests.</p>{/if}
    <div class="grants" aria-label="Device permissions">
      <label><input type="checkbox" bind:checked={control} disabled={$settings.busy} /> Control playback</label>
      <label><input type="checkbox" bind:checked={administration} disabled={$settings.busy} /> Allow administration</label>
    </div>
    {#each $settings.host?.pending ?? [] as pending (pending.id)}
      <div class="device-row"><p>{pending.name} — {pending.expiresInSeconds} seconds remaining at last refresh</p><button type="button" disabled={$settings.busy || (!control && !administration)} on:click={() => settings.approve(pending.id, { control, administration })}>Approve {pending.name}</button></div>
    {/each}
  </div>
  <div class="pairing-section">
    <h3>Paired devices</h3>
    {#if !$settings.host?.paired}<p>Device inventory is not loaded.</p>{:else if !$settings.host.paired.length}<p>No paired devices.</p>{/if}
    {#each $settings.host?.paired ?? [] as device (device.id)}
      <div class="device-row"><p>{device.name} — {device.grants.administration ? "Administration" : "Playback control"}</p><button type="button" disabled={$settings.busy} on:click={() => settings.revoke(device.id)}>Revoke {device.name}</button></div>
    {/each}
  </div>
</section>

<style>
  .lan-settings { margin-bottom: var(--spacing-xl); color: var(--text-primary); }
  h2 { font-size: 0.75rem; font-weight: 700; text-transform: uppercase; letter-spacing: 0.12em; color: var(--text-subdued); margin: 0 0 var(--spacing-sm); }
  h3 { font-size: 13px; font-weight: 500; margin: 0; }
  p { font-size: 12px; line-height: 1.5; color: var(--text-secondary); margin: var(--spacing-xs) 0 var(--spacing-sm); text-wrap: pretty; }
  .host-row { display: flex; align-items: center; justify-content: space-between; gap: var(--spacing-md); }
  .host-row p { max-width: 52ch; }
  button, input { font: inherit; }
  button { padding: var(--spacing-sm) var(--spacing-md); border-radius: var(--radius-sm); border: 1px solid var(--border-color); background: var(--bg-surface); color: var(--text-secondary); font-size: 12px; }
  button:disabled, input:disabled { cursor: not-allowed; }
  button:disabled { opacity: 0.6; }
  .host-switch { display: flex; align-items: center; gap: var(--spacing-sm); border: 0; background: transparent; padding: var(--spacing-xs); flex-shrink: 0; }
  .switch-track { display: flex; align-items: center; width: 32px; height: 18px; padding: 3px; border-radius: 12px; background: var(--bg-highlight); }
  .switch-track span { width: 12px; height: 12px; border-radius: 50%; background: var(--text-subdued); }
  .host-switch[aria-checked="true"] .switch-track { background: var(--accent-primary); }
  .host-switch[aria-checked="true"] .switch-track span { transform: translateX(14px); background: var(--bg-base); }
  .availability { border-left: 2px solid var(--text-subdued); padding: var(--spacing-sm) var(--spacing-md); background: var(--bg-surface); }
  .connection-fields { display: grid; grid-template-columns: minmax(0, 2fr) minmax(0, 1fr); gap: var(--spacing-md); margin-top: var(--spacing-md); }
  label { display: grid; gap: var(--spacing-xs); color: var(--text-secondary); font-size: 12px; }
  input[type="text"], input[type="number"] { min-width: 0; width: 100%; padding: var(--spacing-sm); border: 1px solid var(--border-color); border-radius: var(--radius-sm); background: var(--bg-base); color: var(--text-primary); box-sizing: border-box; }
  button:not(:disabled):hover { border-color: var(--text-subdued); }
  button:focus-visible, input:focus-visible { outline: 2px solid var(--text-primary); outline-offset: 2px; }
  button { min-height: 40px; }
  .device-row { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--spacing-sm); margin-top: var(--spacing-sm); }
  .device-row p { margin: 0; }
  .invitation-qr { display: block; max-width: 100%; margin: var(--spacing-sm) 0; }
  .pairing-section { margin-top: var(--spacing-lg); }
  .invitation-empty { padding: var(--spacing-md); margin: var(--spacing-sm) 0; border: 1px dashed var(--border-color); border-radius: var(--radius-sm); color: var(--text-subdued); font-size: 12px; }
  .actions, .grants { display: flex; flex-wrap: wrap; gap: var(--spacing-sm); }
  .grants { gap: var(--spacing-md); margin-bottom: var(--spacing-sm); }
  .grants label { display: flex; align-items: center; gap: var(--spacing-xs); }
  @media (max-width: 480px) { .connection-fields { grid-template-columns: minmax(0, 1fr); } }
  .fingerprint { display:block; overflow-wrap:anywhere; font-size:12px; line-height:1.6; }
</style>
