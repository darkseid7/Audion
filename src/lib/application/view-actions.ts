import { get, writable, readonly, derived, type Readable } from "svelte/store";
import { parseEnvelope } from "./protocol";
import { getApplicationPort } from "./port";
import { controllerState } from "./controller/bootstrap";
import type { ControllerState } from "./controller/session";
import { canExecute, outputAvailable } from "./capabilities";
import type { ApplicationIntent, ApplicationQuery, ApplicationPort, CommandPreconditions, ControlError, ExecutionResult, OutputRef, PlaybackContext, RepeatMode } from "./types";
export interface ActionFeedback { status: "idle" | "pending" | "applied" | "error" | "unknown"; message: string; jobId?: string }
export interface ActionOutcome extends ActionFeedback { id: number; action: string }
export function createViewActions(port: () => ApplicationPort = getApplicationPort, state: Readable<ControllerState> = controllerState) {
 const outcomes = writable<ActionOutcome[]>([]);
 const admissionError = writable("");
 const feedback = derived(outcomes, entries => entries.at(-1) ?? { status: "idle" as const, message: "" });
 let owner = "", generation = 0, nextId = 0;
 const unsubscribe = state.subscribe(s => {
  const key = s.ready && s.snapshot ? `${s.snapshot.hostId}/${s.snapshot.hostEpoch}` : "";
  if (owner !== key) {
   owner = key;
   generation++;
   outcomes.set([]);
   admissionError.set("");
  }
  for (const entry of get(outcomes)) {
   if (entry.status !== "pending" || !entry.jobId) continue;
   const job = s.snapshot?.jobs.find(j => j.jobId === entry.jobId);
   if (job?.status === "completed") show(job.result, entry.id, generation);
  }
 });
 function show(result: ExecutionResult, id: number, ticket: number) {
  if (ticket !== generation) return;
  let value: ActionFeedback;
  if (result.status === "accepted") value = { status: "pending", jobId: result.jobId, message: "Waiting for the PC to finish…" };
  else if (result.status === "applied") value = { status: "applied", message: "Confirmed by the PC" };
  else value = {
   status: result.error.code === "outcome_unknown" ? "unknown" : "error",
   message: result.error.message + (result.partialEffects.length ? ` Effects already confirmed: ${result.partialEffects.join("; ")}.` : "")
  };
  outcomes.update(entries => entries.map(entry => entry.id === id ? { id, action: entry.action, ...value } : entry));
 }
 function dismiss(id: number) {
  outcomes.update(entries => entries.filter(entry => entry.id !== id || entry.status === "pending"));
  admissionError.set("");
 }
 async function execute(intent: ApplicationIntent): Promise<ExecutionResult> {
  const s = get(state), snapshot = s.snapshot, ticket = generation;
  // Bound presentation, never silently evict an unresolved/error outcome or queue commands.
  outcomes.update(entries => entries.filter(entry => entry.status !== "applied"));
  if (get(outcomes).length >= 32) {
   const message = "Dismiss completed outcomes before submitting more actions.";
   admissionError.set(message);
   return { status: "failed", revision: snapshot?.revision ?? 0, partialEffects: [], error: { code: "execution_failed", message, retryable: false } };
  }
  const id = ++nextId;
  outcomes.update(entries => [...entries, { id, action: intent.type.replaceAll("_", " "), status: "pending", message: "Waiting for the PC…" }]);
  admissionError.set("");
  try {
   if (!canExecute(s, intent.type)) throw { code: !s.ready ? "host_not_ready" : !s.grants?.control ? "permission_required" : "unsupported", message: !s.ready ? "The PC is unavailable. Nothing was queued." : !s.grants?.control ? "Playback permission is required." : "This action is unavailable on the active PC output.", retryable: false };
   if (intent.type === "select_output" && !outputAvailable(s, intent.output)) throw { code: "output_unavailable", message: "The selected PC output is unavailable.", retryable: false };
   const preconditions: CommandPreconditions = { hostEpoch: snapshot!.hostEpoch, outputRevision: snapshot!.revisions.outputRevision };
   if (intent.type.startsWith("play_") || intent.type === "queue_insert" || intent.type === "queue_append" || intent.type === "queue_entity") preconditions.libraryRevision = snapshot!.revisions.libraryRevision;
   if (intent.type.startsWith("queue_")) preconditions.queueRevision = snapshot!.revisions.queueRevision;
   let validated: ApplicationIntent;
   try { validated=parseEnvelope({protocolVersion:1,requestId:"view-validation",preconditions,intent}).intent; }
   catch { throw {code:"invalid_request",message:"The requested action is invalid.",retryable:false}; }
   const result = await port().execute(validated, preconditions);
   if (ticket === generation) { show(result, id, ticket); const job = get(state).snapshot?.jobs.find(j => result.status === "accepted" && j.jobId === result.jobId); if (job?.status === "completed") show(job.result, id, ticket); }
   return result;
  } catch (error) {
   const e: ControlError = error && typeof error === "object" && "code" in error ? error as ControlError : { code: "execution_failed", message: error instanceof Error ? error.message : "The PC command failed.", retryable: false };
   const result: ExecutionResult = { status: "failed", error: e, revision: snapshot?.revision ?? 0, partialEffects: [] };
   show(result, id, ticket);
   return result;
  }
 }
 const playAlbum = (albumId: number, playMode: "all" | "liked_only", startTrackId?: number) => execute({ type: "play_album", albumId, playMode, ...(startTrackId === undefined ? {} : { startTrackId }) });
 const playEntityTrack = (trackId: number, context?: PlaybackContext | null) => {
  if (context?.type === "album") return playAlbum(context.albumId, context.playMode, trackId);
  if (context?.type === "artist") return execute({ type: "play_artist", artistName: context.artistName, startTrackId: trackId });
  if (context?.type === "playlist") return execute({ type: "play_playlist", playlistId: context.playlistId, startTrackId: trackId });
  if (context?.type === "liked") return execute({ type: "play_liked", startTrackId: trackId });
  return execute({ type: "play_track", trackId });
 };
 const queueQuery = (query: ApplicationQuery, placement: "next" | "after_user_queue" | "end") => {
  if (query.type === "album_tracks") return execute({type:"queue_entity",entity:{type:"album",albumId:query.albumId,playMode:query.likedOnly ? "liked_only" : "all"},placement});
  if (query.type === "playlist_tracks") return execute({type:"queue_entity",entity:{type:"playlist",playlistId:query.playlistId},placement});
  if (query.type === "artist_tracks") return execute({type:"queue_entity",entity:{type:"artist",artistName:query.artistName},placement});
  if(query.type === "liked_tracks") return execute({type:"queue_entity",entity:{type:"liked"},placement});
  const result: ExecutionResult={status:"failed",revision:get(state).snapshot?.revision??0,partialEffects:[],error:{code:"invalid_request",message:"This view is not a queueable entity.",retryable:false}};admissionError.set(result.error.message);return Promise.resolve(result);
 };
 return { feedback, outcomes: readonly(outcomes), admissionError: readonly(admissionError), dismiss, execute, queueQuery, canExecute: (capability: string) => canExecute(get(state), capability),
  playAlbum, playEntityTrack,
  playArtist: (artistName: string, startTrackId?: number) => execute({ type: "play_artist", artistName, ...(startTrackId === undefined ? {} : { startTrackId }) }),
  playPlaylist: (playlistId: number, startTrackId?: number) => execute({ type: "play_playlist", playlistId, ...(startTrackId === undefined ? {} : { startTrackId }) }),
  playLiked: (startTrackId?: number) => execute({ type: "play_liked", ...(startTrackId === undefined ? {} : { startTrackId }) }),
  selectOutput: (output: OutputRef) => execute({ type: "select_output", output }),
  setVolume: (volume: number) => execute({ type: "set_volume", volume }), setShuffle: (enabled: boolean) => execute({ type: "set_shuffle", enabled }),
  setRepeat: (mode: RepeatMode) => execute({ type: "set_repeat", mode }),
  seek: (fraction: number) => execute({ type: "seek", seconds: fraction * (get(state).snapshot?.playback.duration ?? 0) }),
  togglePlay: () => execute({ type: get(state).snapshot?.playback.status === "playing" ? "pause" : "resume" }),
  next: () => execute({ type: "next" }), previous: () => execute({ type: "previous" }),
  insertTracks: (trackIds: number[], placement: "next" | "after_user_queue") => execute({ type: "queue_insert", trackIds, placement }),
  appendTracks: (trackIds: number[]) => execute({ type: "queue_append", trackIds }),
  playQueueEntry: (entryId: string) => execute({ type: "queue_play", entryId }),
  removeQueueEntry: (entryId: string) => execute({ type: "queue_remove", entryId }),
  reorderQueueEntry: (entryId: string, beforeEntryId: string | null) => execute({ type: "queue_reorder", entryId, beforeEntryId }),
  clearUpcoming: () => execute({ type: "queue_clear_upcoming" }), dispose: () => { generation++; unsubscribe(); }
 };
}

/** Shared feedback belongs to the controller session, not a local player. */
export const viewActions = createViewActions();

export const playAlbum = viewActions.playAlbum;
export const playArtist = viewActions.playArtist;
export const playPlaylist = viewActions.playPlaylist;
export const playLiked = viewActions.playLiked;
export const playEntityTrack = viewActions.playEntityTrack;
export const selectOutput = viewActions.selectOutput;
