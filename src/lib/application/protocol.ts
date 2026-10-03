import type { ApplicationIntent, CommandEnvelope, CommandPreconditions } from "./types";

/** Shared UTF-8 wire bounds; lyrics have a separate later contract. */
export const MAX_IDENTIFIER_BYTES = 256;
export const MAX_TEXT_BYTES = 2048;
export const MAX_PAGE_ITEMS = 200;
export const MAX_COMMAND_BYTES = 1024 * 1024;
const encoder = new TextEncoder();
type WireObject = Record<string, unknown>;
type Validator = (value: unknown) => void;

function invalid(): never { throw new Error("Invalid command envelope"); }
function object(value: unknown, required: string[], optional: string[] = []): WireObject {
  if (value === null || typeof value !== "object" || Array.isArray(value)) invalid();
  const prototype = Object.getPrototypeOf(value);
  if (prototype !== Object.prototype && prototype !== null) invalid();
  const result = value as WireObject;
  if (required.some(key => !Object.hasOwn(result, key))) invalid();
  if (Reflect.ownKeys(result).some(key => typeof key !== "string" || ![...required, ...optional].includes(key))) invalid();
  return result;
}
function text(value: unknown, max = MAX_IDENTIFIER_BYTES): void {
  if (typeof value !== "string" || value.length === 0 || encoder.encode(value).length > max) invalid();
  // JSON strings must contain Unicode scalar values, just as serde's UTF-8 strings do.
  for (let i = 0; i < value.length; i++) {
    const code = value.charCodeAt(i);
    if (code >= 0xd800 && code <= 0xdbff) {
      const next = value.charCodeAt(++i);
      if (!(next >= 0xdc00 && next <= 0xdfff)) invalid();
    } else if (code >= 0xdc00 && code <= 0xdfff) invalid();
  }
}
function revision(value: unknown): void {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) invalid();
}
function id(value: unknown): void { revision(value); if ((value as number) === 0) invalid(); }
function nonnegative(value: unknown): void {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0) invalid();
}
function oneOf(...values: unknown[]): Validator {
  return value => { if (!values.includes(value)) invalid(); };
}
function output(value: unknown): void {
  if (value === null || typeof value !== "object") invalid();
  const kind = (value as WireObject).kind;
  const result = object(value, kind === "squeeze" ? ["kind", "playerId"] : ["kind"]);
  if (kind === "squeeze") text(result.playerId);
  else if (kind !== "pc") invalid();
}
function ids(value: unknown): void {
  if (!Array.isArray(value) || value.length === 0 || value.length > MAX_PAGE_ITEMS) invalid();
  for (const item of value) id(item);
}
const intentFields: { [K in ApplicationIntent["type"]]: { required: Record<string, Validator>; optional?: Record<string, Validator> } } = {
  play_album: { required: { albumId: id, playMode: oneOf("all", "liked_only") }, optional: { startTrackId: id } },
  play_playlist: { required: { playlistId: id }, optional: { startTrackId: id } },
  play_artist: { required: { artistName: text }, optional: { startTrackId: id } },
  play_liked: { required: {}, optional: { startTrackId: id } },
  play_track: { required: { trackId: id } },
  select_output: { required: { output } },
  pause: { required: {} }, resume: { required: {} }, next: { required: {} }, previous: { required: {} },
  seek: { required: { seconds: nonnegative } },
  set_volume: { required: { volume: value => { nonnegative(value); if ((value as number) > 1) invalid(); } } },
  set_shuffle: { required: { enabled: value => { if (typeof value !== "boolean") invalid(); } } },
  set_repeat: { required: { mode: oneOf("none", "one", "all") } },
  queue_insert: { required: { trackIds: ids, placement: oneOf("next", "after_user_queue") } },
  queue_append: { required: { trackIds: ids } },
  queue_remove: { required: { entryId: text } },
  queue_reorder: { required: { entryId: text, beforeEntryId: value => { if (value !== null) text(value); } } },
  queue_clear_upcoming: { required: {} },
  queue_play: { required: { entryId: text } },
};

export function parseEnvelope(value: unknown): CommandEnvelope {
  const envelope = object(value, ["protocolVersion", "requestId", "preconditions", "intent"]);
  if (envelope.protocolVersion !== 1) invalid();
  text(envelope.requestId);
  const preconditions = object(envelope.preconditions, ["hostEpoch"], ["queueRevision", "libraryRevision", "outputRevision"]);
  text(preconditions.hostEpoch);
  for (const key of ["queueRevision", "libraryRevision", "outputRevision"]) {
    if (Object.hasOwn(preconditions, key)) revision(preconditions[key]);
  }
  const rawIntent = envelope.intent;
  if (rawIntent === null || typeof rawIntent !== "object" || Array.isArray(rawIntent)) invalid();
  const type = (rawIntent as WireObject).type;
  if (typeof type !== "string" || !Object.hasOwn(intentFields, type)) invalid();
  const schema = intentFields[type as ApplicationIntent["type"]];
  const intent = object(rawIntent, ["type", ...Object.keys(schema.required)], Object.keys(schema.optional ?? {}));
  for (const [key, validate] of Object.entries(schema.required)) validate(intent[key]);
  for (const [key, validate] of Object.entries(schema.optional ?? {})) {
    if (Object.hasOwn(intent, key)) validate(intent[key]);
  }
  const entityPlayback = type.startsWith("play_");
  if (type.startsWith("queue_") && !Object.hasOwn(preconditions, "queueRevision")) invalid();
  if ((entityPlayback || type === "queue_insert" || type === "queue_append") && !Object.hasOwn(preconditions, "libraryRevision")) invalid();
  if ((entityPlayback || ["queue_play", "select_output", "pause", "resume", "next", "previous", "seek", "set_volume"].includes(type)) && !Object.hasOwn(preconditions, "outputRevision")) invalid();
  const result = {
    protocolVersion: 1 as const,
    requestId: envelope.requestId as string,
    preconditions: { ...preconditions } as unknown as CommandPreconditions,
    intent: { ...intent } as unknown as ApplicationIntent,
  };
  if ("trackIds" in result.intent) result.intent.trackIds = [...result.intent.trackIds];
  if (result.intent.type === "select_output") result.intent.output = { ...result.intent.output };
  if (encoder.encode(JSON.stringify(result)).length > MAX_COMMAND_BYTES) invalid();
  return result;
}
