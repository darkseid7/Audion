import { expect, it } from "vitest";
import { get } from "svelte/store";
import { createLanSettings, type HostInspection, type LanSettingsDependencies } from "./lan-settings";
const off: HostInspection = { enabled: false, ready: false, endpoint: null, pending: null, paired: null };
const on: HostInspection = { enabled: true, ready: true, endpoint: "192.168.1.2:9010", pending: [{ id: "pending", name: "Phone", expiresInSeconds: 100 }], paired: [] };
it("keeps host availability native-authoritative while enable is pending or rejected", async () => {
  const calls: unknown[] = [];
  let reject!: (reason: unknown) => void;
  const dependencies: LanSettingsDependencies = {
    async invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
      calls.push({ command, args });
      if ((args?.request as { action: string }).action === "status") return off as T;
      return new Promise((_resolve, fail) => { reject = fail; });
    },
    async prepare() { calls.push("prepare"); }, async release() { calls.push("release"); },
  };
  const model = createLanSettings(dependencies);
  await model.refresh();
  const enabling = model.enable("192.168.1.2", 9010);
  await Promise.resolve();
  expect(get(model)).toMatchObject({ host: off, busy: true, error: null });
  reject({ message: "Address already in use" });
  await enabling;
  expect(get(model)).toMatchObject({ host: off, busy: false, error: "Address already in use" });
  expect(calls).toContainEqual({ command: "control_host_enable", args: { request: { action: "enable", config: { address: "192.168.1.2", port: 9010 } } } });
  expect(calls).toContain("release");
});
it("renders only acknowledged native approval/revocation results and never fabricates devices", async () => {
  let native = on;
  const calls: unknown[] = [];
  const model = createLanSettings({
    async invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
      calls.push({ command, args });
      if (command === "control_host_approve") native = { ...on, pending: [], paired: [{ id: "device", name: "Phone", grants: { control: true, administration: false } }] };
      if (command === "control_host_revoke") native = { ...on, pending: [], paired: [] };
      return native as T;
    }, async prepare() {}, async release() {},
  });
  await model.refresh();
  expect(get(model).host?.pending?.[0].id).toBe("pending");
  await model.approve("pending", { control: true, administration: false });
  expect(get(model).host?.paired?.[0].id).toBe("device");
  expect(calls).toContainEqual({ command: "control_host_approve", args: { pendingId: "pending", grants: { control: true, administration: false } } });
  await model.revoke("device");
  expect(get(model).host?.paired).toEqual([]);
  expect(calls).toContainEqual({ command: "control_host_revoke", args: { deviceId: "device" } });
});
it("disables the native listener even when its old bridge lease is already invalidated", async () => {
  let disabled = false;
  const model = createLanSettings({
    async invoke<T>(_command: string, args?: Record<string, unknown>): Promise<T> {
      if ((args?.request as { action: string }).action === "disable") disabled = true;
      return (disabled ? off : on) as T;
    }, async prepare() {}, async release() { throw new Error("Stale lease"); },
  });
  await model.refresh();
  await model.disable();
  expect(disabled).toBe(true);
  expect(get(model)).toMatchObject({ host: off, error: null, busy: false });
});
