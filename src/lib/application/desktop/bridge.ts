import { startStatePublisher, type PublisherStop } from "./state-publisher";
import type { PlaybackCoordinator } from "./playback-coordinator";
import type { HostUpdate, ApplicationPort, CommandEnvelope, ExecutionResult } from "../types";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
export interface CoordinatorLease { leaseId: string; hostEpoch: string }
export interface HostDispatch { ticket: string; envelope: CommandEnvelope }
export type Registration = { phase: "prepare" } | { phase: "ready" | "release"; lease: CoordinatorLease } | { phase: "publish"; lease: CoordinatorLease; update: HostUpdate };
export interface HostTransport {
  listen(listener: (dispatch: HostDispatch) => void): Promise<() => void>;
  register(request: Registration): Promise<{ hostId: string; lease: CoordinatorLease; revision?: number }>;
  authorize(lease: CoordinatorLease, ticket: string): Promise<CommandEnvelope>;
  complete(lease: CoordinatorLease, ticket: string, result: ExecutionResult): Promise<void>;
}
const nativeTransport: HostTransport = {
  listen: listener => getCurrentWebviewWindow().listen<HostDispatch>("controller://dispatch", event => listener(event.payload)),
  register: request => invoke("control_host_register", { request }),
  authorize: (lease, ticket) => invoke("control_host_complete", { request: { phase: "authorize", lease, ticket } }),
  complete: (lease, ticket, result) => invoke("control_host_complete", { request: { phase: "complete", lease, ticket, result } }),
};
export async function connectHostBridge(port: ApplicationPort, attach: (authority: { hostId: string; hostEpoch: string }) => Promise<void>, coordinator: PlaybackCoordinator, transport: HostTransport = nativeTransport): Promise<{ dispose(): Promise<void> }> {
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
  try {
    const registration = await transport.register({ phase: "prepare" });
    lease = registration.lease;
    await attach({ hostId: registration.hostId, hostEpoch: lease.hostEpoch });
    const capturedLease = lease;
    publisher = startStatePublisher(coordinator, async snapshot => {
      const result = await transport.register({ phase: "publish", lease: capturedLease, update: { type: "projection", snapshot } });
      if (!Number.isSafeInteger(result.revision) || result.revision! < 0) throw new Error("Invalid publication acknowledgement");
      return result.revision!;
    }, () => {
      if (!ready) return; // Initial failure is handled by preparation cleanup below.
      ready = false;
      void transport.register({ phase: "release", lease: capturedLease }).catch(() => {}).finally(unlisten);
    });
    await publisher.ready;
    await publisher.flush();
    // Listener may receive an event as soon as native readiness is committed.
    ready = true;
    await transport.register({ phase: "ready", lease });
  } catch (error) {
    ready = false; publisher?.();
    try { if (lease) await transport.register({ phase: "release", lease }); } finally { unlisten(); }
    throw error;
  }
  return { async dispose() {
    if (disposed) return;
    disposed = true; ready = false; publisher?.();
    try { await transport.register({ phase: "release", lease: lease! }); } finally { unlisten(); }
  } };
}
