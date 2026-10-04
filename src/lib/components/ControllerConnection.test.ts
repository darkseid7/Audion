import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { compile } from "svelte/compiler";
import { render } from "svelte/server";
import { writable } from "svelte/store";
import ts from "typescript";
import { expect, it } from "vitest";
const runtimeName: string = "svelte/internal/server";
const runtime = await import(runtimeName);
function renderConnection(patch: object = {}) {
  const source = readFileSync(new URL("./ControllerConnection.svelte", import.meta.url), "utf8");
  const compiled = compile(source, { filename: "ControllerConnection.svelte", generate: "server" }).js.code;
  const code = ts.transpileModule(compiled, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  const exports: Record<string, any> = {};
  runInNewContext(code, { exports, require: (name: string) => {
    if (name === runtimeName) return runtime;
    if (name === "$lib/application/controller/bootstrap") return {
      controllerState: writable({ status: "unavailable", ready: false, currentHostId: "pc", ...patch }), pairedHostIds: writable(["pc"]),
    };
    throw new Error(name);
  } });
  return render(exports.default).body;
}
it("offers explicit private endpoint repair on a paired PC without trust input", () => {
  const html = renderConnection();
  expect(html).toContain("Update address");
  expect(html).toContain("Private IPv4 address and port");
  expect(html).not.toContain('name="ca"');
});
it("shows only public fingerprint comparison while pairing", () => {
  const html = renderConnection({ status: "pairing", pairingFingerprint: "ab".repeat(32) });
  expect(html).toContain("ab".repeat(32));
  expect(html).toContain("SHA-256");
});
