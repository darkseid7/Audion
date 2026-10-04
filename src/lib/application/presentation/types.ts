import type { ApplicationIntent, ArtworkHandle, ArtworkReference, AvailableOutput, ExecutionResult, PlaybackState, QueueEntry, SnapshotOutputRef } from "../types";

export type ActionAvailability = { enabled: true } | { enabled: false; reason: string };
export type NavigationIcon = "home" | "albums" | "liked-songs" | "listen-later" | "recently-played" | "discover" | "tracks" | "artists" | "playlists" | "plugins" | "settings" | "library" | "actions" | "listenbrainz";
export interface NavigationRow {
  id: string;
  label: string;
  icon: NavigationIcon;
  active: boolean;
  count?: number;
  availability: ActionAvailability;
}
export interface NavigationSection { id: string; label: string; rows: readonly NavigationRow[] }
export interface BrowsePresentation<T> {
  items: readonly T[];
  loading: boolean;
  error: string;
  revision: number | null;
  hasMore: boolean;
  hasEarlier: boolean;
}
export interface PlaybackPresentation {
  playback: PlaybackState;
  outputs: readonly AvailableOutput[];
  selectedOutput: SnapshotOutputRef;
  queue: readonly QueueEntry[];
}
export type PresentationCommand = (intent: ApplicationIntent) => Promise<ExecutionResult>;
export type ArtworkResolver = (reference: ArtworkReference, signal?: AbortSignal) => Promise<ArtworkHandle>;

export interface BrowseContextAction { id: string; label: string; availability: ActionAvailability; run?: () => void }
