<script lang="ts">
 import { onDestroy } from "svelte";
 import { controllerState } from "$lib/application/controller/bootstrap";
 import { getApplicationPort } from "$lib/application/port";
 import type { ArtworkReference, ArtworkHandle } from "$lib/application/types";
 export let reference: ArtworkReference | undefined = undefined;
 export let alt = "";
 let src = "", key = "", generation = 0, handle: ArtworkHandle | undefined;
 $: identity = $controllerState.ready && reference ? `${$controllerState.currentHostId}/${$controllerState.snapshot?.hostEpoch}/${$controllerState.snapshot?.revisions.libraryRevision}/${reference.resourceId}/${reference.revision}` : "";
 $: if (identity !== key) { key = identity; void load(reference, identity); }
 async function load(ref: ArtworkReference | undefined, identity: string) {
  const ticket = ++generation; handle?.dispose(); handle = undefined; src = "";
  if (!identity || !ref) return;
  try { const result = await getApplicationPort().resolveArtwork(ref); if (ticket !== generation) result.dispose(); else { handle = result; src = result.src; } } catch { /* Missing artwork is a display-only failure. */ }
 }
 onDestroy(() => { generation++; handle?.dispose(); });
</script>
{#if src}<img {src} {alt} loading="lazy" />{:else}<div class="art-placeholder" aria-hidden="true"><svg viewBox="0 0 24 24" fill="currentColor"><path d="M12 3v12.2A3 3 0 1 0 14 18V7h6V3z" /></svg></div>{/if}
<style>img,.art-placeholder{width:100%;height:100%;object-fit:cover;border-radius:inherit}.art-placeholder{display:grid;place-items:center;background:var(--bg-highlight);color:var(--text-muted)}svg{width:35%;height:35%}</style>
