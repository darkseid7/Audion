import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { compile } from "svelte/compiler";
import { render } from "svelte/server";
import ts from "typescript";
import { expect, it } from "vitest";

const runtimeName: string = "svelte/internal/server";
const runtime = await import(runtimeName);

it("keeps LAN hosting off and every unfinished action disabled", () => {
  const source = readFileSync(new URL("./LanControllerSettings.svelte", import.meta.url), "utf8");
  const compiled = compile(source, { filename: "LanControllerSettings.svelte", generate: "server" }).js.code;
  const { outputText } = ts.transpileModule(compiled, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } });
  const exports: Record<string, any> = {};
  runInNewContext(outputText, { exports, require: (name: string) => {
    if (name !== runtimeName) throw new Error(`Unexpected side-effect dependency: ${name}`);
    return runtime;
  }});
  const html = render(exports.default).body;
  expect(html).toMatch(/role="switch"[^>]*aria-checked="false"[^>]*disabled/);
  expect(html).toContain("Native hosting is not available yet");
  expect(html).toContain("No invitation");
  expect(html).toContain("No paired devices");
  const controls = html.match(/<(?:button|input|select)\b[^>]*>/g) ?? [];
  expect(controls.length).toBeGreaterThanOrEqual(8);
  for (const control of controls) expect(control).toMatch(/\bdisabled\b/);
});
