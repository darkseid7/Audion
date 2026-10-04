<script lang="ts">
 import { viewActions } from "$lib/application/view-actions";
 export let actions = viewActions;
 $: outcomes = actions.outcomes;
 $: admissionError = actions.admissionError;
</script>
{#if $outcomes.length || $admissionError}
 <section class="action-feedback" aria-label="PC action outcomes" aria-live="polite">
  {#each $outcomes as outcome (outcome.id)}
   <div class:error={outcome.status === "error" || outcome.status === "unknown"}>
    <p role={outcome.status === "error" || outcome.status === "unknown" ? "alert" : "status"}>
     <strong>{outcome.action}</strong> — {outcome.message}
    </p>
    {#if outcome.status !== "pending"}
     <button aria-label={`Dismiss ${outcome.action} outcome`} on:click={() => actions.dismiss(outcome.id)}>Dismiss</button>
    {/if}
   </div>
  {/each}
  {#if $admissionError}<p role="alert">{$admissionError}</p>{/if}
 </section>
{/if}
<style>
 .action-feedback { flex-basis:100%; flex-shrink:0; max-height:160px; overflow:auto; padding:8px 12px; background:var(--bg-highlight); border-radius:var(--radius-sm); }
 .action-feedback div { display:flex; align-items:center; gap:12px; }
 p { flex:1; margin:4px 0; font-size:.8rem; color:var(--text-secondary); }
 .error p { color:var(--error-color); }
 button { border:1px solid var(--border-subtle); border-radius:var(--radius-sm); background:none; color:var(--text-primary); padding:6px; cursor:pointer; }
</style>
