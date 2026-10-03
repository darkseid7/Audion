import { get } from "svelte/store";
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
const displayTrack = (track: api.Track): DisplayTrack => ({ id: track.id, title: track.title, artist: track.artist, album: track.album, albumId: track.album_id ?? null, duration: track.duration, trackNumber: track.track_number, discNumber: track.disc_number ?? null, quality: { format: track.format, bitrate: track.bitrate, badges: [] } });
const capabilities = { playback: true, seek: true, volume: true, shuffle: true, repeat: true, equalizer: false };
const unwrap = async (result: Promise<ExecutionResult>): Promise<void> => { const value = await result; if (value.status === "failed" || value.status === "superseded") throw new PlaybackFailure(value.error, value.status, value.partialEffects); };
/** Desktop authority. Network transports never receive host Track objects or legacy calls. */
export function createDesktopAdapter() {
  const hostEpoch = crypto.randomUUID();
  let sequence = 0;
  const entry = (track: api.Track) => ({ entryId: `${hostEpoch}:${++sequence}`, track });
  let value: HostState = { hostEpoch, revision: 0, revisions: { libraryRevision: 0, queueRevision: 0, outputRevision: 0, settingsRevision: 0 }, selectedOutput: output(), ownershipGeneration: 0, transitionGeneration: 0, queue: get(player.queue).map(entry) };
  const listeners = new Set<(update: ApplicationUpdate) => void>();
  let pendingEntries: HostState["queue"] | undefined;
  let disposed = false;
  const outputs = (): AvailableOutput[] => [{ output: { kind: "pc" }, name: "This PC", available: true, capabilities }, ...get(discoveredSqueezePlayers).map(device => ({ output: { kind: "squeeze" as const, playerId: device.mac }, name: device.name, available: true, capabilities }))];
  const snapshot = (): HostSnapshot => {
    const track = get(player.currentTrack);
    const context = get(player.playbackContext);
    return {
      hostId: "desktop", hostEpoch, revision: value.revision, revisions: value.revisions,
      output: get(player.activeBackend) === "remote" ? { kind: "desktop_only", reason: "Legacy cloud control is desktop-only" } : value.selectedOutput,
      outputs: outputs(), settings: {}, jobs: [],
      capabilities: { queries: ["snapshot", "queue", "outputs"], intents: ["play_album", "play_playlist", "play_artist", "play_liked", "play_track", "select_output", "pause", "resume", "next", "previous", "seek", "set_volume", "set_shuffle", "set_repeat", "queue_insert", "queue_append", "queue_remove", "queue_reorder", "queue_clear_upcoming", "queue_play"] },
      queue: { count: value.queue.length, currentEntryId: value.queue[get(player.queueIndex)]?.entryId ?? null },
      playback: { status: get(player.isPlaying) ? "playing" : track ? "paused" : "stopped", track: track ? displayTrack(track) : null,
        context: context?.type === "album" && context.albumId !== undefined ? { type: "album", albumId: context.albumId, playMode: context.playMode ?? "all" } : context?.type === "playlist" && context.playlistId !== undefined ? { type: "playlist", playlistId: context.playlistId } : context?.type === "artist" && context.artistName ? { type: "artist", artistName: context.artistName } : context?.type === "liked" ? { type: "liked" } : context?.type === "track" && context.trackId !== undefined ? { type: "track", trackId: context.trackId } : context?.type === "queue" ? { type: "queue" } : null,
        position: get(player.currentTime), duration: get(player.duration), volume: get(player.volume), shuffle: get(player.shuffle), repeat: get(player.repeat) },
    };
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
  const runtime: DesktopPlaybackRuntime = {
    async validateOutput(target) {
      if (target.kind === "squeeze" && !(await api.squeezeGetPlayers()).some(device => device.mac === target.playerId)) fail("output_unavailable", "Squeeze player is unavailable");
      if (get(player.activeBackend) === "remote") fail("unsupported", "Leave legacy cloud control before selecting a LAN output");
      return applied;
    },
    async resolvePlayback(intent): Promise<ResolvedPlayback> {
      let tracks: api.Track[];
      let context: ResolvedPlayback["context"] = null;
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
      const startIndex = "startTrackId" in intent && intent.startTrackId !== undefined ? tracks.findIndex(track => track.id === intent.startTrackId) : 0;
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
        case "seek": await player.seek(get(player.duration) > 0 ? intent.seconds / get(player.duration) : 0); break;
        case "set_volume": await player.setVolume(intent.volume); break;
        case "set_shuffle": if (get(player.shuffle) !== intent.enabled) await player.toggleShuffle(); break;
        case "set_repeat": await player.setRepeatMode(intent.mode); break;
        case "queue_insert":
          if (!resolved) fail("not_found", "Queue tracks were not resolved");
          if (intent.placement === "next") await player.playNext(resolved!.tracks); else await player.addToQueue(resolved!.tracks); break;
        case "queue_append": await player.appendToQueueEnd(resolved!.tracks); break;
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
  const coordinator = createPlaybackCoordinator(runtime, state);
  const preconditions = () => ({ hostEpoch, ...value.revisions });
  const execute = (intent: ApplicationIntent) => unwrap(coordinator.execute(intent, preconditions()));
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
  setPlayerPreconditions(preconditions());
  const port: ApplicationPort = {
    execute: (intent, context) => get(player.activeBackend) === "remote"
      ? Promise.resolve({ status: "failed", error: { code: "unsupported", message: "Legacy cloud output is desktop-only", retryable: false }, revision: value.revision, partialEffects: [] })
      : coordinator.execute(intent, context),
    async query(query) {
      if (disposed) throw new Error("Desktop adapter disposed");
      if (query.type === "snapshot") return { type: "snapshot", snapshot: snapshot() };
      if (query.type === "outputs") return { type: "outputs", outputs: outputs(), revision: value.revision };
      if (query.type === "queue") return { type: "queue", page: { items: value.queue.map(item => ({ entryId: item.entryId, track: displayTrack(item.track) })), revision: value.revisions.queueRevision, nextCursor: null } };
      return fail("unsupported", "Library query adapter is not installed yet");
    },
    subscribe(listener) { listeners.add(listener); listener({ type: "snapshot", snapshot: snapshot() }); return () => { listeners.delete(listener); }; },
    async resolveArtwork() { return fail("unsupported", "Artwork resolver is not installed yet"); },
  };
  return {
    port, state, coordinator,
    pauseForTimer() { const signal = captureSignal("timer"); return coordinator.enqueueSignal(signal); },
    async dispose() { if (disposed) return; disposed = true; outputSubscriptions.forEach(stop => stop()); unregister(); bindSqueezeSelection(async () => { throw new Error("Desktop adapter disposed"); }); bindSqueezeObservations(() => async () => {}); player.bindPlaybackSignals(() => async () => {}); player.bindDesktopCommands(async () => { throw new Error("Desktop adapter disposed"); }); player.bindDesktopTransfers(async () => { throw new Error("Desktop adapter disposed"); }); setPlayerPreconditions(undefined); await coordinator.dispose(); listeners.clear(); },
  };
}
