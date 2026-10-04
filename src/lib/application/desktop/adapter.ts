import { get } from "svelte/store";
import { pinnedItems } from "$lib/stores/pinned";
import type { DesktopLibraryAccess } from "./bridge";
import type { HostPresentation } from "../types";
import * as player from "./player-runtime";
import * as api from "$lib/api/tauri";
import { activeSqueezePlayer, discoveredSqueezePlayers, commitSqueezeTarget, clearSqueezeTarget, bindSqueezeSelection, bindSqueezeObservations } from "$lib/stores/squeeze";
import { activeRemoteDevice } from "$lib/stores/websocket";
import { registerDesktopPlayer, setPlayerPreconditions } from "$lib/stores/player";
import type { ApplicationIntent, ApplicationPort, ApplicationUpdate, AvailableOutput, DisplayTrack, ExecutionResult, HostSnapshot, OutputRef, SnapshotOutputRef } from "../types";
import { createPlaybackCoordinator, sameOutput, PlaybackFailure, type DesktopPlaybackRuntime, type HostState, type HostStateAccess, type PlaybackSignal, type ResolvedPlayback, type RuntimeResult } from "./playback-coordinator";
const applied: RuntimeResult = { status: "applied" };
const fail = (code: "not_found" | "unsupported" | "output_unavailable", message: string): never => { throw new PlaybackFailure({ code, message, retryable: false }); };
const output = (): SnapshotOutputRef => get(player.activeBackend) === "remote" ? { kind: "desktop_only", reason: "Legacy cloud control is desktop-only" } : get(player.activeBackend) === "squeeze" && get(activeSqueezePlayer) ? { kind: "squeeze", playerId: get(activeSqueezePlayer)! } : { kind: "pc" };
const displayTrack = (track: api.Track): DisplayTrack => ({ id: track.id, title: track.title ?? null, artist: track.artist ?? null, album: track.album ?? null, albumId: track.album_id ?? null, duration: track.duration ?? null, trackNumber: track.track_number ?? null, discNumber: track.disc_number ?? null, quality: { format: track.format ?? null, bitrate: track.bitrate ?? null, badges: [] } });
const capabilities = { playback: true, seek: true, volume: true, shuffle: true, repeat: true, equalizer: false };
const unwrap = async (result: Promise<ExecutionResult>): Promise<void> => { const value = await result; if (value.status === "failed" || value.status === "superseded") throw new PlaybackFailure(value.error, value.status, value.partialEffects); };
/** Desktop authority. Network transports never receive host Track objects or legacy calls. */
export function createDesktopAdapter(localLibrary?: DesktopLibraryAccess, pcName?: string | null) {
  let hostEpoch: string = crypto.randomUUID();
  let hostId = "desktop";
  let sequence = 0;
  const entry = (track: api.Track) => ({ entryId: `${hostEpoch}:${++sequence}`, track });
  let value: HostState = { hostEpoch, revision: 0, revisions: { libraryRevision: 0, queueRevision: 0, outputRevision: 0, settingsRevision: 0 }, selectedOutput: output(), ownershipGeneration: 0, transitionGeneration: 0, queue: get(player.queue).map(entry) };
  const listeners = new Set<(update: ApplicationUpdate) => void>();
  let pendingEntries: HostState["queue"] | undefined;
  let disposed = false;
  let library: DesktopLibraryAccess | undefined;
  let libraryGeneration = 0;
  const currentLibrary = () => {
    if (library?.active?.() === false) { library = undefined; libraryGeneration++; clearTimeout(libraryTimer); }
    return library ?? localLibrary;
  };
  const checkLibrary = (access: DesktopLibraryAccess, generation: number) => {
    if (disposed || currentLibrary() !== access || generation !== libraryGeneration) throw new PlaybackFailure({ code: "resync_required", message: "Library authority changed", retryable: false });
  };
  let libraryTimer: ReturnType<typeof setTimeout> | undefined;
  const presentations = new WeakMap<HostSnapshot, HostPresentation>();
  const media = typeof window !== "undefined" ? window.matchMedia?.("(prefers-reduced-motion: reduce)") : undefined;
  const outputs = (): AvailableOutput[] => [{ output: { kind: "pc" }, name: pcName?.replace(/[\u0000-\u001f\u007f]/g, "").trim().slice(0, 128) || `PC · ${hostId}`, available: true, capabilities }, ...get(discoveredSqueezePlayers).map(device => ({ output: { kind: "squeeze" as const, playerId: device.mac }, name: device.name, available: device.state !== "Disconnected", capabilities }))];
  const snapshot = (): HostSnapshot => {
    const track = get(player.currentTrack);
    const context = get(player.playbackContext);
    const result: HostSnapshot = {
      hostId, hostEpoch, revision: value.revision, revisions: value.revisions,
      output: get(player.activeBackend) === "remote" ? { kind: "desktop_only", reason: "Legacy cloud control is desktop-only" } : value.selectedOutput,
      outputs: outputs(), settings: media ? { reducedMotion: media.matches } : {}, jobs: [],
      capabilities: { queries: (library ?? localLibrary) ? ["snapshot", "outputs", "albums", "album_detail", "album_tracks", "tracks", "artists", "artist_albums", "artist_tracks", "playlists", "playlist_tracks", "liked_tracks", "search", "queue"] : ["snapshot", "outputs"], intents: output().kind === "desktop_only" ? [] : ["play_album", "play_playlist", "play_artist", "play_liked", "play_track", "select_output", "pause", "resume", "next", "previous", "seek", "set_volume", "set_shuffle", "set_repeat", "queue_insert", "queue_append", "queue_entity", "queue_remove", "queue_reorder", "queue_clear_upcoming", "queue_play"] },
      queue: { count: value.queue.length, currentEntryId: value.queue[get(player.queueIndex)]?.entryId ?? null },
      playback: { status: get(player.isPlaying) ? "playing" : track ? "paused" : "stopped", track: track ? displayTrack(track) : null,
        context: context?.type === "album" && context.albumId !== undefined ? { type: "album", albumId: context.albumId, playMode: context.playMode ?? "all" } : context?.type === "playlist" && context.playlistId !== undefined ? { type: "playlist", playlistId: context.playlistId } : context?.type === "artist" && context.artistName ? { type: "artist", artistName: context.artistName } : context?.type === "liked" ? { type: "liked" } : context?.type === "track" && context.trackId !== undefined ? { type: "track", trackId: context.trackId } : context?.type === "queue" ? { type: "queue" } : null,
        position: get(player.currentTime), duration: get(player.duration), volume: get(player.volume), shuffle: get(player.shuffle), repeat: get(player.repeat) },
    };
    presentations.set(result, { queue: value.queue.map(e => ({ entryId: e.entryId, track: displayTrack(e.track) })), pinnedAlbumIds: [...get(pinnedItems).albums] });
    return result;
  };
  const state: HostStateAccess = {
    read: () => value,
    commit(delta) {
      const tracks = get(player.queue);
      const actualOutput = output();
      const outputChanged = !sameOutput(actualOutput, value.selectedOutput);
      const queueChanged = tracks.length !== value.queue.length || tracks.some((track, i) => track !== value.queue[i]?.track) || pendingEntries !== undefined;
      const available = [...value.queue];
      const entries = pendingEntries ?? (queueChanged ? tracks.map(track => { const index = available.findIndex(item => item.track === track); return index < 0 ? entry(track) : available.splice(index, 1)[0]; }) : value.queue);
      pendingEntries = undefined;
      value = { ...value, selectedOutput: actualOutput, ...delta, queue: entries, revision: value.revision + 1, revisions: { ...value.revisions, ...(outputChanged ? { outputRevision: value.revisions.outputRevision + 1 } : {}), ...(queueChanged ? { queueRevision: value.revisions.queueRevision + 1 } : {}), ...delta.revisions } };
      setPlayerPreconditions({ hostEpoch, ...value.revisions });
      const update: ApplicationUpdate = { type: "snapshot", snapshot: snapshot() };
      for (const listener of listeners) { try { listener(update); } catch (error) { console.error("Desktop subscriber failed", error); } }
    },
  };
  const resolveTrack = async (id: number) => await api.getTrackById(id) ?? fail("not_found", `Track ${id} is unavailable`);
  const refreshLibrary = async () => {
    const access = currentLibrary(); if (!access || disposed) return;
    const generation = libraryGeneration;
    const revision = await access.revision();
    checkLibrary(access, generation);
    if (!Number.isSafeInteger(revision) || revision < value.revisions.libraryRevision) throw new PlaybackFailure({ code: "resync_required", message: "Invalid native library revision", retryable: false });
    if (revision !== value.revisions.libraryRevision) state.commit({ revisions: { libraryRevision: revision } });
  };
  const runtime: DesktopPlaybackRuntime = {
    refreshLibrary,
    async validateOutput(target) {
      if (target.kind === "squeeze" && !(await api.squeezeGetPlayers()).some(device => device.mac === target.playerId)) fail("output_unavailable", "Squeeze player is unavailable");
      if (get(player.activeBackend) === "remote") fail("unsupported", "Leave legacy cloud control before selecting a LAN output");
      return applied;
    },
    async resolvePlayback(intent): Promise<ResolvedPlayback> {
      let tracks: api.Track[];
      let context: ResolvedPlayback["context"] = null;
      if (intent.type === "queue_entity") {
        const e=intent.entity;
        const play: ApplicationIntent = e.type === "album" ? {type:"play_album",albumId:e.albumId,playMode:e.playMode} : e.type === "playlist" ? {type:"play_playlist",playlistId:e.playlistId} : e.type === "artist" ? {type:"play_artist",artistName:e.artistName} : {type:"play_liked"};
        return runtime.resolvePlayback(play);
      }
      switch (intent.type) {
        case "play_album":
          tracks = await api.getTracksByAlbum(intent.albumId);
          if (intent.playMode === "liked_only") { const liked = new Set(await api.getLikedTrackIds()); tracks = tracks.filter(track => liked.has(track.id)); }
          context = { type: "album", albumId: intent.albumId, playMode: intent.playMode }; break;
        case "play_playlist": tracks = await api.getPlaylistTracks(intent.playlistId); context = { type: "playlist", playlistId: intent.playlistId }; break;
        case "play_artist": tracks = await api.getTracksByArtist(intent.artistName); context = { type: "artist", artistName: intent.artistName }; break;
        case "play_liked": tracks = await api.getLikedTracks(); context = { type: "liked" }; break;
        case "play_track": tracks = [await resolveTrack(intent.trackId)]; context = { type: "track", trackId: intent.trackId }; break;
        case "queue_insert": case "queue_append": tracks = await Promise.all(intent.trackIds.map(resolveTrack)); break;
        default: return fail("unsupported", "Intent does not resolve tracks");
      }
      const hasExplicitStart = "startTrackId" in intent && intent.startTrackId !== undefined;
      const shuffleEntity = ["play_album", "play_playlist", "play_artist", "play_liked"].includes(intent.type) && get(player.shuffle);
      const startIndex = hasExplicitStart
        ? tracks.findIndex(track => track.id === intent.startTrackId)
        : shuffleEntity ? Math.floor(Math.random() * tracks.length) : 0;
      if (!tracks.length || startIndex < 0) fail("not_found", "Requested playback is unavailable");
      return { tracks, startIndex, context };
    },
    async stopOwnedOutput(target) {
      player.invalidateDesktopObservations();
      if (target.kind === "squeeze") { await api.squeezeStop(target.playerId); player.isPlaying.set(false); }
      else await player.stopLocalOutput();
      return applied;
    },
    async selectOutput(target) {
      if (target.kind === "squeeze") commitSqueezeTarget(target.playerId);
      else { const previous = get(activeSqueezePlayer); if (previous) await api.squeezeDisconnectPlayer(previous); clearSqueezeTarget(); activeRemoteDevice.set(null); player.activeBackend.set("none"); }
      return applied;
    },
    async apply(intent, resolved) {
      player.invalidateDesktopObservations();
      try {
      if (get(player.activeBackend) === "squeeze" && !get(activeSqueezePlayer)) fail("output_unavailable", "No Squeeze player is selected");
      const index = (id: string) => { const found = value.queue.findIndex(item => item.entryId === id); return found < 0 ? fail("not_found", "Queue entry is unavailable") : found; };
      switch (intent.type) {
        case "play_album": case "play_playlist": case "play_artist": case "play_liked": case "play_track":
          if (!resolved) fail("not_found", "Playback was not resolved");
          await player.playTracks(resolved!.tracks, resolved!.startIndex, resolved!.context ?? undefined); pendingEntries = resolved!.tracks.map(entry); break;
        case "pause": await player.pause(); break;
        case "resume": await player.resume(); break;
        case "next": await player.nextTrack(); break;
        case "previous": await player.previousTrack(); break;
        case "seek":
          try { await player.seek(get(player.duration) > 0 ? intent.seconds / get(player.duration) : 0); }
          catch (error) { if (String(error).includes("SQUEEZE_SEEK_PARTIAL:")) throw new PlaybackFailure({code:"execution_failed",message:String(error),retryable:false},"failed",["Previous output stopped"]); throw error; }
          break;
        case "set_volume": await player.setVolume(intent.volume); break;
        case "set_shuffle": if (get(player.shuffle) !== intent.enabled) await player.toggleShuffle(); break;
        case "set_repeat": await player.setRepeatMode(intent.mode); break;
        case "queue_insert":
          if (!resolved) fail("not_found", "Queue tracks were not resolved");
          if (intent.placement === "next") await player.playNext(resolved!.tracks); else await player.addToQueue(resolved!.tracks); break;
        case "queue_append": await player.appendToQueueEnd(resolved!.tracks); break;
        case "queue_entity":
          if (!resolved) fail("not_found", "Queue entity was not resolved");
          if (intent.placement === "next") await player.playNext(resolved!.tracks);
          else if (intent.placement === "end") await player.appendToQueueEnd(resolved!.tracks);
          else await player.addToQueue(resolved!.tracks);
          break;
        case "queue_remove": { const at = index(intent.entryId); const entries = [...value.queue]; entries.splice(at, 1); await player.removeFromQueue(at); pendingEntries = entries; break; }
        case "queue_reorder": {
          const from = index(intent.entryId);
          const before = intent.beforeEntryId === null ? value.queue.length : index(intent.beforeEntryId);
          const to = before > from ? before - 1 : before;
          const entries = [...value.queue]; const [moved] = entries.splice(from, 1); entries.splice(to, 0, moved);
          await player.reorderQueue(from, to); pendingEntries = entries; break;
        }
        case "queue_clear_upcoming": await player.clearUpcoming(); break;
        case "queue_play": await player.playFromQueue(index(intent.entryId)); break;
        case "select_output": return fail("unsupported", "Output selection must use coordinator");
      }
      return applied;
      } finally { player.invalidateDesktopObservations(); }
    },
    async applySignal(signal) { if (signal.kind === "gapless") await player.applyGaplessAdvance(); else await player.applyTrackEnd(); return applied; },
  };
  const coordinator = createPlaybackCoordinator(runtime, state, {
    read: snapshot,
    presentation: snapshot => { const projection = presentations.get(snapshot); if (!projection) throw new Error("Missing atomic host presentation"); return projection; },
    subscribe(listener) {
      const forward = (update: ApplicationUpdate) => { if (update.type === "snapshot") listener(update.snapshot); };
      listeners.add(forward); return () => { listeners.delete(forward); };
    },
  });
  const preconditions = () => ({ hostEpoch, ...value.revisions });
  const execute = (intent: ApplicationIntent) => unwrap(coordinator.executeDesktopIntent!(intent, hostEpoch));
  const local = (operation: () => void | Promise<void>, replaces = false) => unwrap(coordinator.executeLocal(async () => { player.invalidateDesktopObservations(); try { await operation(); return applied; } finally { player.invalidateDesktopObservations(); } }, replaces));
  const entryAt = (index: number) => value.queue[index]?.entryId ?? fail("not_found", "Queue entry is unavailable");
  const unregister = registerDesktopPlayer({
    execute: intent => get(player.activeBackend) === "remote" ? local(async () => {
      switch (intent.type) {
        case "pause": return player.pause(); case "resume": return player.resume(); case "next": return player.nextTrack(); case "previous": return player.previousTrack();
        case "seek": return player.seek(intent.seconds / get(player.duration)); case "set_volume": return player.setVolume(intent.volume);
        case "set_shuffle": if (get(player.shuffle) !== intent.enabled) await player.toggleShuffle(); return;
        case "set_repeat": return player.setRepeatMode(intent.mode);
        case "queue_clear_upcoming": return player.clearUpcoming();
        default: fail("unsupported", "This command is unavailable on legacy cloud output");
      }
    }) : execute(intent),
    addToQueue: tracks => local(() => player.addToQueue(tracks)),
    playNext: tracks => local(() => player.playNext(tracks)),
    appendToQueueEnd: tracks => local(() => player.appendToQueueEnd(tracks)),
    playTrack: (track, skip, start) => local(() => player.playTrack(track, skip, start), true),
    playTracks: (tracks, index, context) => local(() => player.playTracks(tracks, index, context), true),
    removeFromQueue: index => get(player.activeBackend) === "remote" ? local(() => player.removeFromQueue(index)) : execute({ type: "queue_remove", entryId: entryAt(index) }),
    reorderQueue: (from, to) => get(player.activeBackend) === "remote" ? local(() => player.reorderQueue(from, to)) : execute({ type: "queue_reorder", entryId: entryAt(from), beforeEntryId: to >= value.queue.length - 1 ? null : entryAt(to > from ? to + 1 : to) }),
    playFromQueue: index => get(player.activeBackend) === "remote" ? local(() => player.playFromQueue(index), true) : execute({ type: "queue_play", entryId: entryAt(index) }),
    transferPlayback: input => local(() => player.transferPlayback(input), true),
    sendRemoteCommand: (target, command, data) => local(() => player.sendRemoteCommand(target, command, data)),
    shutdownPlayer: () => local(() => player.shutdownPlayer(), true),
    selectThisDevice: () => get(player.activeBackend) === "remote" ? local(() => player.selectThisDevice(), true) : execute({ type: "select_output", output: { kind: "pc" } }),
    toggleRemoteControl: device => local(async () => { if (get(player.activeBackend) !== "remote") { const target = output(); if (target.kind === "desktop_only") fail("unsupported", "Legacy cloud output is desktop-only"); const stopped = await runtime.stopOwnedOutput(target as OutputRef); if (stopped.status !== "applied") throw new PlaybackFailure(stopped.error, stopped.status, stopped.partialEffects); } player.toggleRemoteControl(device); }, true),
  });
  const captureSignal = (kind: PlaybackSignal["kind"]): PlaybackSignal => ({ kind, output: value.selectedOutput, ownershipGeneration: value.ownershipGeneration, transitionGeneration: value.transitionGeneration });
  player.bindPlaybackSignals(() => { const origin = captureSignal("completion"); return kind => coordinator.enqueueSignal({ ...origin, kind }); });
  player.bindDesktopCommands(execute);
  player.bindDesktopTransfers(input => local(() => player.transferPlayback(input), true));
  bindSqueezeObservations((kind = "sample") => {
    const origin = captureSignal("gapless");
    const revision = value.revision;
    return operation => unwrap(coordinator.executeLocal(async () => {
      const current = state.read();
      if ((kind === "sample" && revision !== current.revision) || !sameOutput(origin.output, current.selectedOutput) || origin.ownershipGeneration !== current.ownershipGeneration || origin.transitionGeneration !== current.transitionGeneration) return applied;
      const previousTrack = get(player.currentTrack);
      const previousIndex = get(player.queueIndex);
      const afterConfirmation = await operation();
      if (previousTrack?.id !== get(player.currentTrack)?.id || previousIndex !== get(player.queueIndex)) state.commit({ transitionGeneration: current.transitionGeneration + 1 });
      afterConfirmation?.();
      return applied;
    }));
  });
  bindSqueezeSelection(playerId => execute({ type: "select_output", output: { kind: "squeeze", playerId } }));
  // Server stop/disconnect may release ownership outside an application command.
  // Reconcile that confirmed external effect in the same lane, never resurrect it.
  const reconcileOutput = () => {
    if (disposed || sameOutput(output(), value.selectedOutput)) return;
    void coordinator.executeLocal(async () => {
      const actual = output();
      if (!sameOutput(actual, value.selectedOutput)) state.commit({
        selectedOutput: actual,
        ownershipGeneration: value.ownershipGeneration + 1,
        transitionGeneration: value.transitionGeneration + 1,
        revisions: { outputRevision: value.revisions.outputRevision + 1 },
      });
      return applied;
    });
  };
  const outputSubscriptions = [player.activeBackend.subscribe(reconcileOutput), activeSqueezePlayer.subscribe(reconcileOutput)];
  // Store observations enter the same lane after an in-flight domain operation.
  // Initial store subscription values are already represented by snapshot().
  let observing = false;
  let observationQueued = false;

  let settingsChanged = false;
  let outputsChanged = false;
  let previousOutputs = JSON.stringify(outputs());
  const observe = () => {
    if (!observing || disposed || observationQueued) return;
    observationQueued = true;
    queueMicrotask(() => {
      if (disposed) { observationQueued = false; return; }
      void coordinator.executeLocal(async () => {
        observationQueued = false;
        const revisions = { ...value.revisions };

        if (settingsChanged) revisions.settingsRevision++;
        if (outputsChanged) revisions.outputRevision++;
        settingsChanged = outputsChanged = false;
        state.commit({ revisions });
        return applied;
      });
    });
  };
  const projectionSubscriptions = [player.currentTime, player.duration, player.currentTrack, player.isPlaying, player.volume, player.shuffle, player.repeat, player.playbackContext, player.queueIndex].map(store => store.subscribe(observe));
  projectionSubscriptions.push(discoveredSqueezePlayers.subscribe(() => {
    const current = JSON.stringify(outputs());
    if (observing && current !== previousOutputs) { previousOutputs = current; outputsChanged = true; observe(); }
  }));
  projectionSubscriptions.push(pinnedItems.subscribe(() => { if (observing) observe(); }));
  const observeSettings = () => { settingsChanged = true; observe(); };
  media?.addEventListener("change", observeSettings);
  projectionSubscriptions.push(() => media?.removeEventListener("change", observeSettings));
  observing = true;
  setPlayerPreconditions(preconditions());
  // Local port revisions order desktop commits only. The LAN bridge publishes
  // projections first; Rust allocates snapshot/event/result replay revisions.
  const port: ApplicationPort = {
    execute: (intent, context) => get(player.activeBackend) === "remote"
      ? Promise.resolve({ status: "failed", error: { code: "unsupported", message: "Legacy cloud output is desktop-only", retryable: false }, revision: value.revision, partialEffects: [] })
      : coordinator.execute(intent, context),
    async query(query, signal) {
      signal?.throwIfAborted();
      if (disposed) throw new Error("Desktop adapter disposed");
      if (query.type === "snapshot") return { type: "snapshot", snapshot: snapshot() };
      if (query.type === "outputs") return { type: "outputs", outputs: outputs(), revision: value.revisions.outputRevision };
      const access = currentLibrary();
      if (access && (library || query.type !== "queue")) {
        const generation = libraryGeneration;const result = await access.query(query, signal);checkLibrary(access, generation);
        if (result.type !== "queue" && result.type !== "snapshot" && result.type !== "outputs") {
          const revision = "page" in result ? result.page.revision : result.revision;
          await unwrap(coordinator.executeLocal(async () => {
            checkLibrary(access, generation);
            if (!Number.isSafeInteger(revision) || revision < value.revisions.libraryRevision) throw new PlaybackFailure({ code: "revision_conflict", message: "Library page is stale", retryable: false });
            if (revision !== value.revisions.libraryRevision) state.commit({ revisions: { libraryRevision: revision } });return applied;
          }));
          checkLibrary(access, generation);
        }
        return result;
      }
      if (query.type === "queue") {
        const limit = query.limit ?? 200;let offset = 0;
        const invalid = (code: "invalid_request" | "revision_conflict"): never => { throw new PlaybackFailure({ code, message: "Queue page is unavailable", retryable: false }); };
        if (!Number.isSafeInteger(limit) || limit < 1 || limit > 200) return invalid("invalid_request");
        if (query.cursor) {
          let parts: unknown;try { parts = JSON.parse(atob(query.cursor)); } catch { return invalid("invalid_request"); }
          if (!Array.isArray(parts) || parts.length !== 4 || !Number.isSafeInteger(parts[3]) || parts[3] < 0) return invalid("invalid_request");
          if (parts[0] !== hostEpoch || parts[1] !== value.revisions.queueRevision || parts[2] !== limit) return invalid("revision_conflict");offset = parts[3];
        }
        if (offset > value.queue.length) return invalid("invalid_request");
        return { type: "queue", page: { items: value.queue.slice(offset, offset + limit).map(item => ({ entryId: item.entryId, track: displayTrack(item.track) })), revision: value.revisions.queueRevision, nextCursor: offset + limit < value.queue.length ? btoa(JSON.stringify([hostEpoch, value.revisions.queueRevision, limit, offset + limit])) : null } };
      }
      return fail("unsupported", "Library query adapter is not installed yet");
    },
    subscribe(listener) { listeners.add(listener); listener({ type: "snapshot", snapshot: snapshot() }); return () => { listeners.delete(listener); }; },
    async resolveArtwork(reference, signal) {
      const access = currentLibrary();if (!access) return fail("unsupported", "Artwork resolver is not installed yet");
      const generation = libraryGeneration;const result = await access.artwork(reference, signal);
      try { checkLibrary(access, generation); } catch (error) { result.dispose();throw error; }return result;
    },
  };
  return {
    port, state, coordinator,
    async attachAuthority(authority: { hostId: string; hostEpoch: string; library?: DesktopLibraryAccess }) {
      if (disposed) throw new Error("Desktop adapter disposed");
      // Invalidate queued old-epoch commands synchronously, then drain the same
      // lane before advertising readiness. An in-flight effect is never replayed.
      hostId = authority.hostId; hostEpoch = authority.hostEpoch; sequence = 0;
      clearTimeout(libraryTimer); libraryGeneration++; library = authority.library;
      const generation = libraryGeneration;
      pendingEntries = value.queue.map(item => entry(item.track));
      state.commit({ hostEpoch, ownershipGeneration: value.ownershipGeneration + 1, transitionGeneration: value.transitionGeneration + 1 });
      try {
        if (library) {
          const access = library;
          const revision = await access.revision();
          checkLibrary(access, generation);
          if (!Number.isSafeInteger(revision) || revision < value.revisions.libraryRevision) throw new Error("Invalid native library revision");
          await unwrap(coordinator.executeLocal(async () => {
            checkLibrary(access, generation);
            if (revision < value.revisions.libraryRevision) throw new Error("Invalid native library revision");
            state.commit({ revisions: { libraryRevision: revision } });
            return applied;
          }));
        } else await unwrap(coordinator.executeLocal(async () => applied));
      } catch (error) {
        if (!disposed && generation === libraryGeneration) {
          library = undefined;libraryGeneration++;hostId = "desktop";hostEpoch = crypto.randomUUID();sequence = 0;
          pendingEntries = value.queue.map(item => entry(item.track));
          state.commit({ hostEpoch, ownershipGeneration: value.ownershipGeneration + 1, transitionGeneration: value.transitionGeneration + 1 });
        }
        throw error;
      }
      const access = library;
      const observeLibrary = async () => {
        if (disposed || !access || access.active?.() === false || library !== access) return;
        try {
          const revision = await access.revision();
          if (!disposed && library === access && revision !== value.revisions.libraryRevision) await coordinator.executeLocal(async () => { await refreshLibrary(); return applied; });
        } catch { /* Busy/unavailable is not an invented revision or successful refresh. */ }
        if (!disposed && access.active?.() !== false && library === access) libraryTimer = setTimeout(observeLibrary, 250);
      };
      if (access) libraryTimer = setTimeout(observeLibrary, 250);
    },
    pauseForTimer() { const signal = captureSignal("timer"); return coordinator.enqueueSignal(signal); },
    async dispose() { if (disposed) return; disposed = true; libraryGeneration++; clearTimeout(libraryTimer); library = undefined; projectionSubscriptions.forEach(stop => stop()); outputSubscriptions.forEach(stop => stop()); unregister(); bindSqueezeSelection(async () => { throw new Error("Desktop adapter disposed"); }); bindSqueezeObservations(() => async () => {}); player.bindPlaybackSignals(() => async () => {}); player.bindDesktopCommands(async () => { throw new Error("Desktop adapter disposed"); }); player.bindDesktopTransfers(async () => { throw new Error("Desktop adapter disposed"); }); setPlayerPreconditions(undefined); await coordinator.dispose(); listeners.clear(); },
  };
}
