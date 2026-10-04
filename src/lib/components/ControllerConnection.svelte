<script lang="ts">
  import { controllerState, pairedHostIds, connectController, suspendController, forgetController, pairController, updateControllerEndpoint } from "$lib/application/controller/bootstrap";
  let actionError="";
  let endpoints: Record<string, string> = {};
  async function updateAddress(host: string) {
    await run(() => updateControllerEndpoint(host, (endpoints[host] ?? "").trim()));
  }
  const labels={disconnected:"Connect to your PC",connecting:"Connecting to your PC…",connected:"Connected to your PC",unavailable:"PC unavailable",pairing:"Approve this phone on your PC",pairing_required:"Pair with your PC again",permission_required:"Permission required",protocol_error:"Incompatible PC connection"};
  async function run(action:()=>Promise<void>){actionError="";try{await action();}catch{actionError="The operation could not be completed. Please try again.";}}
</script>
<section class="connection" aria-labelledby="controller-heading">
  <div class="connection-heading"><span class="eyebrow">AUDION · DESKTOP CONTROLLER</span><h1 id="controller-heading">{labels[$controllerState.status]}</h1></div>
  <p class="explanation">Your PC plays the music. This phone controls it over your local network.</p>
  <div class="status" role="status" aria-live="polite"><span class:online={$controllerState.ready} class="indicator"></span><span>{$controllerState.ready ? "PC state confirmed" : $controllerState.status === "pairing" ? "Complete pairing on the PC. Keep both devices on the same network." : "Playback stays on your PC while disconnected."}</span></div>
  {#if $controllerState.pairingFingerprint}
    <p class="address-note">Compare this SHA-256 certificate fingerprint with the PC before approving:</p>
    <code class="fingerprint">{$controllerState.pairingFingerprint}</code>
  {/if}
  {#if $controllerState.snapshot}<p class="now-playing">{$controllerState.snapshot.playback.track?.title ?? "Nothing playing"}</p>{/if}
  {#if $controllerState.error}<p class="error" role="alert">{$controllerState.error.message}</p>{/if}
  {#if actionError}<p class="error" role="alert">{actionError}</p>{/if}
  <div class="actions">
    <button class="primary" disabled={$controllerState.status === "pairing"} on:click={()=>run(pairController)}>Pair a PC</button>
    {#if $controllerState.currentHostId && !$controllerState.ready && $controllerState.status !== "pairing"}<button on:click={()=>run(()=>connectController($controllerState.currentHostId!))}>Reconnect</button>{/if}
    {#if $controllerState.ready || $controllerState.status === "pairing"}<button on:click={suspendController}>{$controllerState.status === "pairing" ? "Cancel" : "Disconnect"}</button>{/if}
  </div>
  {#if $pairedHostIds.length}
    <h2>Paired PCs</h2>
    <ul>{#each $pairedHostIds as host}
      <li>
        <div class="host-actions">
          <button class="host" aria-pressed={$controllerState.currentHostId===host} on:click={()=>run(()=>connectController(host))}>PC <span>{host.slice(0,8)}</span></button>
          <button aria-label={`Forget PC ${host.slice(0,8)}`} on:click={()=>run(()=>forgetController(host))}>Forget</button>
        </div>
        <details>
          <summary>Update address</summary>
          <form on:submit|preventDefault={()=>updateAddress(host)}>
            <label>Private IPv4 address and port
              <input required type="text" bind:value={endpoints[host]} placeholder="192.168.1.20:12345" autocomplete="off" spellcheck={false} />
            </label>
            <p class="address-note">Use the address shown on this PC. Its saved identity and certificate stay unchanged. Only an authenticated connection confirms the update.</p>
            <button type="submit" disabled={$controllerState.status === "connecting"}>Save and connect</button>
          </form>
        </details>
      </li>
    {/each}</ul>
  {/if}
</section>
<style>
  .connection{width:100%;max-width:560px;margin:auto;padding:var(--spacing-xl);padding-top:calc(var(--spacing-xl) + var(--safe-area-top));overflow:auto;}
  .eyebrow{color:var(--text-secondary);font-size:0.75rem;letter-spacing:0.08em;}
  h1{font-size:2rem;line-height:1.15;letter-spacing:-0.025em;margin-top:var(--spacing-md);text-wrap:balance;}
  .explanation{color:var(--text-secondary);line-height:1.6;margin-top:var(--spacing-md);}
  .status{display:flex;gap:var(--spacing-sm);align-items:center;margin:var(--spacing-xl) 0;line-height:1.5;color:var(--text-secondary);}
  .indicator{width:8px;height:8px;border-radius:var(--radius-full);background:var(--text-subdued);flex-shrink:0;}.indicator.online{background:var(--accent-primary);}
  .now-playing{font-weight:600;margin-bottom:var(--spacing-lg);}.error{color:var(--error-color);margin:var(--spacing-md) 0;line-height:1.5;}
  .actions{display:flex;gap:var(--spacing-sm);flex-wrap:wrap;}button{min-height:44px;padding:var(--spacing-sm) var(--spacing-md);border-radius:var(--radius-md);color:var(--text-primary);background:var(--bg-surface);border:1px solid transparent;cursor:pointer;font:inherit;}
  button:hover:not(:disabled){background:var(--bg-highlight);}button:focus-visible{outline:2px solid var(--accent-primary);outline-offset:3px;}button:active:not(:disabled){background:var(--bg-press);}button:disabled{opacity:0.5;cursor:default;}
  button.primary{background:var(--accent-primary);color:var(--bg-base);font-weight:600;}button.primary:hover:not(:disabled){background:var(--accent-hover);}
  h2{font-size:1rem;margin-top:var(--spacing-xl);margin-bottom:var(--spacing-sm);}ul{list-style:none;padding:0;}li{margin-top:var(--spacing-md);}.host-actions{display:flex;gap:var(--spacing-sm);}.host{flex:1;text-align:left;}.host span{color:var(--text-secondary);font-variant-numeric:tabular-nums;}.host[aria-pressed="true"]{border-color:var(--accent-primary);}
  summary{cursor:pointer;padding:var(--spacing-sm) 0;min-height:44px;}
  form,label{display:grid;gap:var(--spacing-sm);}input{min-width:0;min-height:44px;padding:var(--spacing-sm);font:inherit;background:var(--bg-base);color:var(--text-primary);border:1px solid var(--border-color);border-radius:var(--radius-sm);}
  input:focus-visible,summary:focus-visible{outline:2px solid var(--accent-primary);outline-offset:3px;}
  .address-note{color:var(--text-secondary);font-size:.875rem;line-height:1.5;}
  .fingerprint{display:block;overflow-wrap:anywhere;font-size:.875rem;line-height:1.6;margin-bottom:var(--spacing-md);}
</style>
