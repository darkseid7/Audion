import { writable } from "svelte/store";
export interface HostInspection {
  enabled: boolean; ready: boolean; endpoint: string | null;
  pending: { id: string; name: string; expiresInSeconds: number }[] | null;
  paired: { id: string; name: string; grants: { control: boolean; administration: boolean } }[] | null;
}
export interface LanSettingsDependencies {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  prepare(): Promise<void>;
  release(): Promise<void>;
}
export function createLanSettings(dependencies: LanSettingsDependencies) {
  const state = writable<{ host: HostInspection | null; busy: boolean; error: string | null; invitation: { encoded: string; qrSvg: string; fingerprint: string } | null }>({ host: null, busy: false, error: null, invitation: null });
  let busy = false;
  const inspect = async () => {
    const host = await dependencies.invoke<HostInspection>("control_host_enable", { request: { action: "status" } });
    state.update(value => ({ ...value, host, invitation: host.enabled ? value.invitation : null }));
  };
  const run = async (operation: () => Promise<void>) => {
    if (busy) return;
    busy = true; state.update(value => ({ ...value, busy: true, error: null }));
    try { await operation(); } catch (error) {
      state.update(value => ({ ...value, error: error && typeof error === "object" && "message" in error ? String(error.message) : "Native host operation failed. Refresh to check its state." }));
    } finally { busy = false; state.update(value => ({ ...value, busy: false })); }
  };
  return {
    subscribe: state.subscribe,
    refresh: () => run(inspect),
    enable: (address: string, port: number) => run(async () => {
      await dependencies.prepare();
      try {
        await dependencies.invoke("control_host_enable", { request: { action: "enable", config: { address, port } } });
      } catch (error) { await dependencies.release().catch(() => {}); throw error; }
      await inspect();
    }),
    disable: () => run(async () => {
      // Withdraw authority before listener shutdown, never after player teardown.
      // Native Disable independently invalidates authority. A stale/missing
      // bridge must never prevent the user from shutting down the listener.
      await dependencies.release().catch(() => {});
      await dependencies.invoke("control_host_enable", { request: { action: "disable" } });
      await inspect();
    }),
    createInvitation: () => run(async () => {
      const invitation = await dependencies.invoke<{ encoded: string; qrSvg: string; fingerprint: string }>("control_host_invitation");
      state.update(value => ({ ...value, invitation }));
    }),
    approve: (pendingId: string, grants: { control: boolean; administration: boolean }) => run(async () => {
      await dependencies.invoke("control_host_approve", { pendingId, grants });
      await inspect();
    }),
    revoke: (deviceId: string) => run(async () => {
      await dependencies.invoke("control_host_revoke", { deviceId });
      await inspect();
    }),
    clearInvitation() { state.update(value => ({ ...value, invitation: null })); },
  };
}
