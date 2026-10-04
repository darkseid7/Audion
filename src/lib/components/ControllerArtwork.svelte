<script lang="ts">
 import { onDestroy } from "svelte";
 import { controllerState } from "$lib/application/controller/bootstrap";
 import { getApplicationPort } from "$lib/application/port";
 import type { ArtworkReference, ArtworkHandle } from "$lib/application/types";
 export let reference: ArtworkReference | undefined = undefined;
 export let alt = "";
 let src = "", key = "", displayScope = "", generation = 0, handle: ArtworkHandle | undefined, abort: AbortController | undefined;
 $: identity = $controllerState.ready && $controllerState.snapshot && reference ? JSON.stringify([$controllerState.currentHostId, $controllerState.snapshot.hostId, $controllerState.snapshot.hostEpoch, $controllerState.grants?.control, $controllerState.snapshot.revisions.libraryRevision, reference.resourceId, reference.revision, reference.presentationKey]) : "";
 $: if (identity !== key) { key = identity; void load(reference, identity); }
 async function load(ref: ArtworkReference | undefined, identity: string) {
  const ticket = ++generation; abort?.abort(); abort = undefined; handle?.dispose(); handle = undefined;
  const snapshot = $controllerState.snapshot;
  const scope = identity && ref && snapshot ? JSON.stringify([$controllerState.currentHostId, snapshot.hostId, snapshot.hostEpoch, $controllerState.grants?.control, ref.presentationKey ?? ref.resourceId]) : "";
  // Keep already-decoded pixels, never the old media capability, during replacement.
  if (!scope || scope !== displayScope) src = "";
  displayScope = scope;
  if (!identity || !ref) return;
  if (ref.revision !== snapshot?.revisions.libraryRevision) return;
  abort = new AbortController();
  try {
   const result = await getApplicationPort().resolveArtwork(ref, abort.signal);
   if (ticket !== generation) { result.dispose(); return; }
   handle = result;
   // A resolved Blob is not yet a decoded image. Stage it before replacing visible pixels.
   const image = new Image(); image.src = result.src; await image.decode();
   if (ticket !== generation) result.dispose(); else src = result.src;
  } catch { if (ticket === generation) { handle?.dispose(); handle = undefined; src = ""; } /* Missing artwork is a display-only failure. */ }
 }
 onDestroy(() => { generation++; abort?.abort(); handle?.dispose(); src = ""; });
</script>
{#if src}<img {src} {alt} loading="lazy" />{:else}<div class="art-placeholder" aria-hidden="true"><svg viewBox="0 0 24 24" fill="currentColor"><path d="M12 3v12.2A3 3 0 1 0 14 18V7h6V3z" /></svg></div>{/if}
<style>img,.art-placeholder{width:100%;height:100%;object-fit:cover;border-radius:inherit}.art-placeholder{display:grid;place-items:center;background:var(--bg-highlight);color:var(--text-muted)}svg{width:35%;height:35%}</style>
