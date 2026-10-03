import type { AlbumView } from "../stores/album-view";
export type { AlbumView };

export type ApplicationMode = "desktop" | "controller";
export type RepeatMode = "none" | "one" | "all";
export type AlbumSort = "artist-asc" | "artist-desc" | "year-desc" | "year-asc" | "added-desc" | "added-asc" | "name-asc" | "name-desc";
export type OutputRef = { kind: "pc" } | { kind: "squeeze"; playerId: string };
export type SnapshotOutputRef = OutputRef | { kind: "desktop_only"; reason: string };
export interface CommandPreconditions {
  hostEpoch: string;
  queueRevision?: number;
  libraryRevision?: number;
  outputRevision?: number;
}
export type ApplicationIntent =
  | { type: "play_album"; albumId: number; playMode: "all" | "liked_only"; startTrackId?: number }
  | { type: "play_playlist"; playlistId: number; startTrackId?: number }
  | { type: "play_artist"; artistName: string; startTrackId?: number }
  | { type: "play_liked"; startTrackId?: number }
  | { type: "play_track"; trackId: number }
  | { type: "select_output"; output: OutputRef }
  | { type: "pause" } | { type: "resume" } | { type: "next" } | { type: "previous" }
  | { type: "seek"; seconds: number }
  | { type: "set_volume"; volume: number }
  | { type: "set_shuffle"; enabled: boolean }
  | { type: "set_repeat"; mode: RepeatMode }
  | { type: "queue_insert"; trackIds: number[]; placement: "next" | "after_user_queue" }
  | { type: "queue_append"; trackIds: number[] }
  | { type: "queue_remove"; entryId: string }
  | { type: "queue_reorder"; entryId: string; beforeEntryId: string | null }
  | { type: "queue_clear_upcoming" }
  | { type: "queue_play"; entryId: string };
export interface CommandEnvelope {
  protocolVersion: 1;
  requestId: string;
  preconditions: CommandPreconditions;
  intent: ApplicationIntent;
}
export type ControlErrorCode = "unauthorized" | "permission_required" | "unsupported" | "not_found" | "revision_conflict" | "output_unavailable" | "host_not_ready" | "execution_failed" | "outcome_unknown" | "resync_required" | "invalid_request" | "rate_limited" | "too_large" | "busy";
export interface ControlError { code: ControlErrorCode; message: string; retryable: boolean }
export type ExecutionResult =
  | { status: "applied"; revision: number }
  | { status: "accepted"; jobId: string; revision: number }
  | { status: "failed" | "superseded"; error: ControlError; revision: number; partialEffects: string[] };
export interface ArtworkReference { resourceId: string; revision: number }
export interface ArtworkHandle { src: string; dispose(): void }
export interface QualitySummary { format: string | null; bitrate: number | null; badges: string[] }
export interface DisplayTrack {
  id: number;
  title: string | null;
  artist: string | null;
  albumId: number | null;
  album: string | null;
  duration: number | null;
  trackNumber: number | null;
  discNumber: number | null;
  quality: QualitySummary;
  artwork?: ArtworkReference;
}
export interface AlbumSortSummary { artist: string | null; year: number | null; dateAdded: string | null; name: string }
export interface DisplayAlbum {
  id: number;
  name: string;
  artist: string | null;
  year: number | null;
  qualityBadges: string[];
  sortSummary: AlbumSortSummary;
  artwork?: ArtworkReference;
}
export interface DisplayArtist { name: string; trackCount: number; albumCount: number; artwork?: ArtworkReference }
export interface QueueEntry { entryId: string; track: DisplayTrack }
export interface Page<T> { items: T[]; nextCursor: string | null; revision: number }
export interface Pagination { limit?: number; cursor?: string }
export type ApplicationQuery =
  | { type: "snapshot" } | { type: "outputs" }
  | ({ type: "albums"; sort?: AlbumSort } & Pagination)
  | ({ type: "album_tracks"; albumId: number } & Pagination)
  | ({ type: "tracks" } & Pagination)
  | ({ type: "artists" } & Pagination)
  | ({ type: "artist_albums"; artistName: string } & Pagination)
  | ({ type: "search"; text: string } & Pagination)
  | ({ type: "queue" } & Pagination);
export interface OutputCapabilities { playback: boolean; seek: boolean; volume: boolean; shuffle: boolean; repeat: boolean; equalizer: boolean }
export interface AvailableOutput { output: OutputRef; name: string; available: boolean; capabilities: OutputCapabilities }
export interface HostCapabilities { queries: ApplicationQuery["type"][]; intents: ApplicationIntent["type"][] }
export type PlaybackContext =
  | { type: "album"; albumId: number; playMode: "all" | "liked_only" }
  | { type: "playlist"; playlistId: number }
  | { type: "artist"; artistName: string }
  | { type: "liked" } | { type: "track"; trackId: number } | { type: "queue" };
export interface PlaybackState {
  status: "stopped" | "playing" | "paused" | "failed" | "unknown";
  track: DisplayTrack | null;
  context: PlaybackContext | null;
  position: number;
  duration: number | null;
  volume: number;
  shuffle: boolean;
  repeat: RepeatMode;
  error?: ControlError;
}
export interface QueueSummary { count: number; currentEntryId: string | null }
export interface DomainRevisions { libraryRevision: number; queueRevision: number; outputRevision: number; settingsRevision: number }
export interface SettingsProjection { albumView?: AlbumView; reducedMotion?: boolean }
export type CompletedExecutionResult = Exclude<ExecutionResult, { status: "accepted" }>;
export type JobSummary =
  | { jobId: string; status: "pending" | "running"; progress: number | null }
  | { jobId: string; status: "completed"; result: CompletedExecutionResult };
export interface HostSnapshot {
  hostId: string;
  hostEpoch: string;
  revision: number;
  revisions: DomainRevisions;
  playback: PlaybackState;
  queue: QueueSummary;
  output: SnapshotOutputRef;
  outputs: AvailableOutput[];
  capabilities: HostCapabilities;
  settings: SettingsProjection;
  jobs: JobSummary[];
}
export type HostEvent =
  | { type: "playback"; revision: number; playback: PlaybackState }
  | { type: "queue"; revision: number; queueRevision: number; queue: QueueSummary }
  | { type: "library"; revision: number; libraryRevision: number }
  | { type: "output"; revision: number; outputRevision: number; output: SnapshotOutputRef; outputs: AvailableOutput[] }
  | { type: "settings"; revision: number; settingsRevision: number; settings: SettingsProjection }
  | { type: "capabilities"; revision: number; capabilities: HostCapabilities }
  | { type: "job"; revision: number; job: JobSummary };
export type ApplicationUpdate =
  | { type: "snapshot"; snapshot: HostSnapshot }
  | { type: "events"; hostEpoch: string; events: HostEvent[] };
export type SearchMatch = { type: "track"; track: DisplayTrack } | { type: "album"; album: DisplayAlbum } | { type: "artist"; artist: DisplayArtist };
export type QueryResult =
  | { type: "snapshot"; snapshot: HostSnapshot }
  | { type: "albums"; page: Page<DisplayAlbum> }
  | { type: "album_tracks"; page: Page<DisplayTrack> }
  | { type: "tracks"; page: Page<DisplayTrack> }
  | { type: "artists"; page: Page<DisplayArtist> }
  | { type: "artist_albums"; page: Page<DisplayAlbum> }
  | { type: "search"; page: Page<SearchMatch> }
  | { type: "queue"; page: Page<QueueEntry> }
  | { type: "outputs"; outputs: AvailableOutput[]; revision: number };
export interface ApplicationPort {
  query(query: ApplicationQuery, signal?: AbortSignal): Promise<QueryResult>;
  execute(intent: ApplicationIntent, preconditions: CommandPreconditions): Promise<ExecutionResult>;
  subscribe(listener: (update: ApplicationUpdate) => void): () => void;
  resolveArtwork(reference: ArtworkReference, signal?: AbortSignal): Promise<ArtworkHandle>;
}

export type HostUpdate = { type: "projection"; snapshot: HostSnapshot };
export interface EventCursor { hostEpoch: string; revision: number }
export interface EventBatch { hostEpoch: string; revision: number; events: HostEvent[] }
