<!-- Native host commands arrive in Task 6. Do not enable these controls or
     persist frontend hosting state before the protected native host exists. -->
<section class="lan-settings" aria-labelledby="lan-heading">
  <h2 id="lan-heading">LAN controller</h2>
  <div class="host-row">
    <div>
      <h3>Control this PC from your phone</h3>
      <p>Playback and your library stay on this PC. Your phone is a remote, not an audio output.</p>
    </div>
    <button class="host-switch" type="button" role="switch" aria-checked="false" disabled aria-describedby="lan-unavailable">
      <span class="switch-track" aria-hidden="true"><span></span></span><span>Off</span>
    </button>
  </div>
  <p id="lan-unavailable" class="availability">Native hosting is not available yet. No network listener is running.</p>
  <div class="connection-fields">
    <label>Private network interface
      <select disabled aria-describedby="lan-network-note"><option>Not selected</option></select>
    </label>
    <label>Port<input type="number" min="1" max="65535" placeholder="Not configured" disabled /></label>
  </div>
  <p id="lan-network-note">Local network only. No cloud relay, account, or router port forwarding.</p>
  <div class="pairing-section">
    <h3>Pair a phone</h3>
    <p>Invitations expire after 5 minutes and require approval on this PC.</p>
    <div class="invitation-empty" aria-label="Pairing QR code unavailable">No invitation — hosting is off.</div>
    <div class="actions"><button type="button" disabled>Create invitation</button><button type="button" disabled>Copy invitation</button></div>
  </div>
  <div class="pairing-section">
    <h3>Pending approval</h3><p>No pending requests.</p>
    <div class="grants" aria-label="Device permissions">
      <label><input type="checkbox" disabled /> Control playback</label>
      <label><input type="checkbox" disabled /> Administration</label>
    </div>
    <button type="button" disabled>Approve device</button>
  </div>
  <div class="pairing-section">
    <h3>Paired devices</h3><p>No paired devices are available while hosting is unavailable.</p>
    <button type="button" disabled>Revoke device</button>
  </div>
</section>

<style>
  .lan-settings { margin-bottom: var(--spacing-xl); color: var(--text-primary); }
  h2 { font-size: 0.75rem; font-weight: 700; text-transform: uppercase; letter-spacing: 0.12em; color: var(--text-subdued); margin: 0 0 var(--spacing-sm); }
  h3 { font-size: 13px; font-weight: 500; margin: 0; }
  p { font-size: 12px; line-height: 1.5; color: var(--text-secondary); margin: var(--spacing-xs) 0 var(--spacing-sm); text-wrap: pretty; }
  .host-row { display: flex; align-items: center; justify-content: space-between; gap: var(--spacing-md); }
  .host-row p { max-width: 52ch; }
  button, input, select { font: inherit; }
  button { padding: var(--spacing-sm) var(--spacing-md); border-radius: var(--radius-sm); border: 1px solid var(--border-color); background: var(--bg-surface); color: var(--text-secondary); font-size: 12px; }
  button:disabled, input:disabled, select:disabled { cursor: not-allowed; }
  button:disabled { opacity: 0.6; }
  .host-switch { display: flex; align-items: center; gap: var(--spacing-sm); border: 0; background: transparent; padding: var(--spacing-xs); flex-shrink: 0; }
  .switch-track { display: flex; align-items: center; width: 32px; height: 18px; padding: 3px; border-radius: 12px; background: var(--bg-highlight); }
  .switch-track span { width: 12px; height: 12px; border-radius: 50%; background: var(--text-subdued); }
  .availability { border-left: 2px solid var(--text-subdued); padding: var(--spacing-sm) var(--spacing-md); background: var(--bg-surface); }
  .connection-fields { display: grid; grid-template-columns: minmax(0, 2fr) minmax(0, 1fr); gap: var(--spacing-md); margin-top: var(--spacing-md); }
  label { display: grid; gap: var(--spacing-xs); color: var(--text-secondary); font-size: 12px; }
  select, input[type="number"] { min-width: 0; width: 100%; padding: var(--spacing-sm); border: 1px solid var(--border-color); border-radius: var(--radius-sm); background: var(--bg-base); color: var(--text-subdued); box-sizing: border-box; }
  .pairing-section { margin-top: var(--spacing-lg); }
  .invitation-empty { padding: var(--spacing-md); margin: var(--spacing-sm) 0; border: 1px dashed var(--border-color); border-radius: var(--radius-sm); color: var(--text-subdued); font-size: 12px; }
  .actions, .grants { display: flex; flex-wrap: wrap; gap: var(--spacing-sm); }
  .grants { gap: var(--spacing-md); margin-bottom: var(--spacing-sm); }
  .grants label { display: flex; align-items: center; gap: var(--spacing-xs); }
  @media (max-width: 480px) { .connection-fields { grid-template-columns: minmax(0, 1fr); } }
</style>
