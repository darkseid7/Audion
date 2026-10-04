<script lang="ts">
 import { controllerOverlay } from "$lib/application/controller-ui";
 import ControllerFeedback from "./ControllerFeedback.svelte";
 import { controllerState } from "$lib/application/controller/bootstrap";
 import { canExecute, sameOutput } from "$lib/application/capabilities";
 import type { ApplicationIntent, ExecutionResult, OutputRef } from "$lib/application/types";
 export let execute: (intent: ApplicationIntent) => Promise<ExecutionResult>;
 export let close: () => void = () => {};
 function select(output: OutputRef) { return execute({ type: "select_output", output }); }
</script>
<div class="connect-overlay" role="presentation" on:click|self={close}>
 <div class="connect-panel" role="dialog" aria-modal="true" tabindex="-1" use:controllerOverlay={{close}} aria-label="PC outputs">
  <header><h2>Connect to a device</h2><button on:click={close} aria-label="Close outputs">×</button></header>
  <p>Playback stays on your PC or its Squeeze player.</p>
  {#if $controllerState.snapshot?.output.kind === "desktop_only"}<p role="status">{$controllerState.snapshot.output.reason}. Choose a supported output on the PC.</p>{/if}
  {#each $controllerState.snapshot?.outputs ?? [] as item}
   <button class="device-card" class:active={$controllerState.snapshot && sameOutput(item.output, $controllerState.snapshot.output)} disabled={!item.available || !canExecute($controllerState,"select_output")} on:click={() => select(item.output)} aria-pressed={$controllerState.snapshot ? sameOutput(item.output,$controllerState.snapshot.output) : false}>
    <svg viewBox="0 0 24 24" width="26" height="26" fill="currentColor"><path d="M20 3H4a2 2 0 0 0-2 2v12h20V5a2 2 0 0 0-2-2M4 5h16v10H4m4 14h8v-2H8z" /></svg>
    <span>{item.name}<small>{item.output.kind === "pc" ? "PC output" : "Squeeze Connect"}{!item.available ? " · Unavailable" : ""}</small></span>
   </button>
  {/each}
  <ControllerFeedback />
  <p class="unavailable" aria-disabled="true">Cloud playback is unavailable on this controller.</p>
 </div>
</div>
<style>.connect-overlay{position:fixed;inset:0;background:#0008;z-index:200;display:grid;place-items:center;padding:var(--spacing-lg)}.connect-panel{background:var(--bg-elevated);border:1px solid var(--border-subtle);border-radius:var(--radius-lg);padding:var(--spacing-lg);width:min(100%,440px)}header{display:flex;align-items:center;justify-content:space-between}h2{font-size:1.25rem}p,small{color:var(--text-secondary);font-size:.85rem}button{color:var(--text-primary);background:var(--bg-highlight);border:1px solid var(--border-subtle);border-radius:var(--radius-md);padding:12px;cursor:pointer}.device-card{display:flex;gap:16px;align-items:center;width:100%;margin-top:12px;text-align:left}.active{border-color:var(--accent-primary);color:var(--accent-primary)}small{display:block;margin-top:4px}.unavailable{margin-top:20px}button:disabled{opacity:.45;cursor:not-allowed}
 .connect-overlay { padding:calc(16px + var(--safe-area-top)) calc(16px + var(--safe-area-right)) calc(16px + var(--safe-area-bottom)) calc(16px + var(--safe-area-left)); }
 .connect-panel { max-height:100%; overflow:auto; overscroll-behavior:contain; min-width:0; }
 button { min-width:44px; min-height:44px; }
 .device-card span { min-width:0; overflow-wrap:anywhere; }
</style>
