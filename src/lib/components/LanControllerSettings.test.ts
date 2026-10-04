import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { compile } from "svelte/compiler";
import { render } from "svelte/server";
import ts from "typescript";
import { expect, it } from "vitest";
import * as svelte from "svelte";
import * as lanSettings from "$lib/application/desktop/lan-settings";

const runtimeName: string = "svelte/internal/server";
const runtime = await import(runtimeName);

it("renders Off without native side effects and gates hosting until native inspection", () => {
  const source = readFileSync(new URL("./LanControllerSettings.svelte", import.meta.url), "utf8");
  const compiled = compile(source, { filename: "LanControllerSettings.svelte", generate: "server" }).js.code;
  const { outputText } = ts.transpileModule(compiled, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } });
  const exports: Record<string, any> = {};
  let invoked = false;
  runInNewContext(outputText, { exports, require: (name: string) => {
    if (name === runtimeName) return runtime;
    if (name === "svelte") return svelte;
    if (name === "$lib/application/desktop/lan-settings") return lanSettings;
    if (name === "@tauri-apps/api/core") return { invoke: () => { invoked = true; throw new Error("Unexpected native call during rendering"); } };
    throw new Error(`Unexpected side-effect dependency: ${name}`);
  }, clearTimeout });
  const html = render(exports.default).body;
  expect(html).toMatch(/role="switch"[^>]*aria-checked="false"[^>]*disabled/);
  expect(html).toContain("Checking native host availability");
  expect(html).toContain("No active invitation");
  expect(html).toContain("Device inventory is not loaded");
  expect(html).toMatch(/<button[^>]*disabled[^>]*>Create invitation/);
  expect(html).toContain("Refresh devices");
  expect(invoked).toBe(false);
});
it("renders the public PC fingerprint alongside its native invitation", () => {
  const source = readFileSync(new URL("./LanControllerSettings.svelte", import.meta.url), "utf8");
  const compiled = compile(source, { filename: "LanControllerSettings.svelte", generate: "server" }).js.code;
  const code = ts.transpileModule(compiled, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  const exports: Record<string, any> = {};
  runInNewContext(code, { exports, clearTimeout, require: (name: string) => {
    if (name === runtimeName) return runtime;
    if (name === "svelte") return svelte;
    if (name === "@tauri-apps/api/core") return { invoke() {} };
    if (name === "$lib/application/desktop/lan-settings") return { createLanSettings: () => ({
      subscribe(run: Function) { run({ busy:false, host:{enabled:true,ready:true,endpoint:"192.168.1.9:1234"}, invitation:{encoded:"opaque",qrSvg:"<svg/>",fingerprint:"ab".repeat(32)} }); return () => {}; }, clearInvitation() {},
    }) };
    throw new Error(name);
  } });
  const html = render(exports.default).body;
  expect(html).toContain("SHA-256");
  expect(html).toContain("ab".repeat(32));
  expect(html).not.toContain("opaque");
});
