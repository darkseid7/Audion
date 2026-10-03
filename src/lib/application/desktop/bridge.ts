import { startStatePublisher, type PublisherStop } from "./state-publisher";
import type { PlaybackCoordinator } from "./playback-coordinator";
import type { HostUpdate, ApplicationPort, CommandEnvelope, ExecutionResult, ApplicationQuery, QueryResult, ArtworkReference, ArtworkHandle } from "../types";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
export interface CoordinatorLease { leaseId: string; hostEpoch: string }
export interface HostDispatch { ticket: string; envelope: CommandEnvelope }
export type Registration = { phase: "prepare" } | { phase: "ready" | "release"; lease: CoordinatorLease } | { phase: "publish"; lease: CoordinatorLease; update: HostUpdate };
export interface DesktopLibraryAccess {
  active?(): boolean;
  revision(): Promise<number>;
  query(query: ApplicationQuery, signal?: AbortSignal): Promise<QueryResult>;
  artwork(reference: ArtworkReference, signal?: AbortSignal): Promise<ArtworkHandle>;
}
export interface HostTransport {
  library?(lease: CoordinatorLease): DesktopLibraryAccess;
  listen(listener: (dispatch: HostDispatch) => void): Promise<() => void>;
  register(request: Registration): Promise<{ hostId: string; lease: CoordinatorLease; revision?: number }>;
  authorize(lease: CoordinatorLease, ticket: string): Promise<CommandEnvelope>;
  complete(lease: CoordinatorLease, ticket: string, result: ExecutionResult): Promise<void>;
}
type LibraryResponse = { type: "library_revision"; revision: number } | { type: "library_query"; result: QueryResult } | { type: "artwork"; mime: string; base64: string };
function artworkHandle(r: LibraryResponse): ArtworkHandle {
  if (r.type !== "artwork" || !["image/png", "image/jpeg", "image/webp"].includes(r.mime) || r.base64.length > Math.ceil(5 * 1024 * 1024 / 3) * 4) throw new Error("Invalid artwork reply");
  const bytes = Uint8Array.from(atob(r.base64), c => c.charCodeAt(0)); if (bytes.length > 5 * 1024 * 1024) throw new Error("Oversize artwork");
  const src = URL.createObjectURL(new Blob([bytes], { type: r.mime })); let disposed = false;
  return { src, dispose() { if (!disposed) { disposed = true; URL.revokeObjectURL(src); } } };
}
export type DesktopLibraryRegistration =
  | { phase: "desktop_library_query"; pinnedAlbumIds: number[]; query?: undefined }
  | { phase: "desktop_library_query"; pinnedAlbumIds: number[]; query: ApplicationQuery }
  | { phase: "desktop_artwork"; reference: ArtworkReference };
/** Lazy local-main IPC; never enables hosting or requests a protected credential. */
export function createDesktopLibraryAccess(pins: () => number[]): DesktopLibraryAccess {
  const request = (request: DesktopLibraryRegistration) => invoke<LibraryResponse>("control_host_register", { request });
  return {
    async revision() { const r = await request({ phase: "desktop_library_query", pinnedAlbumIds: pins() });if (r.type !== "library_revision") throw new Error("Invalid library revision reply");return r.revision; },
    async query(query, signal) { signal?.throwIfAborted();const r = await request({ phase: "desktop_library_query", query, pinnedAlbumIds: pins() });signal?.throwIfAborted();if (r.type !== "library_query") throw new Error("Invalid library reply");return r.result; },
    async artwork(reference, signal) { signal?.throwIfAborted();const r = await request({ phase: "desktop_artwork", reference });signal?.throwIfAborted();return artworkHandle(r); },
  };
}
const nativeTransport: HostTransport = {
  library: lease => ({
    async revision() { const r = await invoke<LibraryResponse>("control_host_register", { request: { phase: "library_revision", lease } }); if (r.type !== "library_revision") throw new Error("Invalid library reply"); return r.revision; },
    async query(query, signal) { signal?.throwIfAborted(); const r = await invoke<LibraryResponse>("control_host_register", { request: { phase: "library_query", lease, query } }); signal?.throwIfAborted(); if (r.type !== "library_query") throw new Error("Invalid library reply"); return r.result; },
    async artwork(reference, signal) {
      signal?.throwIfAborted(); const r = await invoke<LibraryResponse>("control_host_register", { request: { phase: "artwork", lease, reference } }); signal?.throwIfAborted();
      return artworkHandle(r);
    },
  }),
  listen: listener => getCurrentWebviewWindow().listen<HostDispatch>("controller://dispatch", event => listener(event.payload)),
  register: request => invoke("control_host_register", { request }),
  authorize: (lease, ticket) => invoke("control_host_complete", { request: { phase: "authorize", lease, ticket } }),
  complete: (lease, ticket, result) => invoke("control_host_complete", { request: { phase: "complete", lease, ticket, result } }),
};
export async function connectHostBridge(port: ApplicationPort, attach: (authority: { hostId: string; hostEpoch: string; library?: DesktopLibraryAccess }) => Promise<void>, coordinator: PlaybackCoordinator, transport: HostTransport = nativeTransport): Promise<{ dispose(): Promise<void> }> {
  let lease: CoordinatorLease | undefined;
  let publisher: PublisherStop | undefined;
  let ready = false;
  let disposed = false;
  const unlisten = await transport.listen(dispatch => {
    if (!ready || disposed || !lease || typeof dispatch?.ticket !== "string") return;
    const capturedLease = lease;
    // Native admission bounds this work before it enters the one domain lane.
    void transport.authorize(capturedLease, dispatch.ticket).then(async envelope => {
      if (disposed || !ready || envelope.preconditions.hostEpoch !== capturedLease.hostEpoch) return;
      const result = await port.execute(envelope.intent, envelope.preconditions);
      await publisher!.flush();
      if (disposed || !ready) return;
      await transport.complete(capturedLease, dispatch.ticket, result);
    }).catch(() => {
      // Events are not authority. Forged, duplicate and expired tickets are
      // rejected natively, without executing or logging their untrusted payload.
    });
  });
  let release: Promise<void> | undefined;
  const dispose = (): Promise<void> => {
    if (release) return release;
    // Local admission and observation end immediately, independently of native
    // release, which may be waiting behind protected storage on the host mutex.
    disposed = true; ready = false; publisher?.(); unlisten();
    const capturedLease = lease;
    if (!capturedLease) return release = Promise.resolve();
    let deadline: ReturnType<typeof setTimeout> | undefined;
    release = Promise.race([
      Promise.resolve().then(() => transport.register({ phase: "release", lease: capturedLease })).then(() => {}),
      new Promise<never>((_, reject) => {
        // Cleanup gets its own short bound, not another publication deadline.
        // This does not cancel native work or claim the lease was invalidated.
        deadline = setTimeout(() => reject(new Error("Controller release unavailable; native invalidation outcome unknown")), 1000);
      }),
    ]).finally(() => clearTimeout(deadline));
    // Publication failure can start cleanup without a waiting caller. Retain
    // this exact outcome for dispose(), including after a late native receipt.
    void release.catch(() => {});
    return release;
  };
  try {
    const registration = await transport.register({ phase: "prepare" });
    lease = registration.lease;
    await attach({ hostId: registration.hostId, hostEpoch: lease.hostEpoch, ...(transport.library ? { library: { ...transport.library(lease), active: () => !disposed } } : {}) });
    const capturedLease = lease;
    publisher = startStatePublisher(coordinator, async snapshot => {
      const result = await transport.register({ phase: "publish", lease: capturedLease, update: { type: "projection", snapshot, ...(coordinator.capturePresentation?.(snapshot) ? { presentation: coordinator.capturePresentation(snapshot) } : {}) } });
      if (!Number.isSafeInteger(result.revision) || result.revision! < 0) throw new Error("Invalid publication acknowledgement");
      return result.revision!;
    }, () => {
      if (!ready) return; // Initial failure is handled by preparation cleanup below.
      void dispose();
    });
    await publisher.ready;
    await publisher.flush();
    // Listener may receive an event as soon as native readiness is committed.
    ready = true;
    await transport.register({ phase: "ready", lease });
    if (disposed) throw new Error("Controller bridge unavailable");
  } catch (error) {
    await dispose();
    throw error;
  }
  return { dispose };
}
