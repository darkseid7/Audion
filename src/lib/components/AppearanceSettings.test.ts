import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { runInNewContext } from "node:vm";
import { compile, parse } from "svelte/compiler";
import { render } from "svelte/server";
import * as svelte from "svelte";
import { writable } from "svelte/store";
import ts from "typescript";
import { expect, it, vi } from "vitest";
import * as catalog from "../stores/theme";
const runtime: string = "svelte/internal/server";
const server = await import(runtime);
const path = resolve("src/lib/components/presentation/AppearanceSettings.svelte");
function component() {
  if (!existsSync(path)) return { default: () => {} };
  const code = ts.transpileModule(compile(readFileSync(path, "utf8"), { filename: path, generate: "server" }).js.code, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  const exports: any = {};
  const modules: Record<string, unknown> = { "svelte/internal/server": server, svelte, "$lib/stores/theme": catalog, "svelte-i18n": { locale: writable("en"), _: writable((key: string, args: any) => args?.default ?? key) } };
  runInNewContext(code, { exports, require(id: string) { if (!(id in modules)) throw new Error(`Unexpected appearance dependency: ${id}`); return modules[id]; } });
  return exports;
}
const modes = ["dark", "light", "system", ...catalog.themePresets.map(p => p.id)];
it.each(modes)("shared appearance selects %s and exposes the complete existing catalog", mode => {
  const html = render(component().default, { props: { state: { mode, accentColor: "#1DB954", customAccentColors: [] } } }).body;
  expect(html).toContain("Theme mode"); expect(html).toContain("Theme presets");
  for (const preset of catalog.themePresets) expect(html).toContain(preset.name);
  expect(html).toContain(`data-theme-mode="${mode}"`);
  expect(html).toMatch(new RegExp(`data-theme-mode="${mode}"[^>]*aria-pressed="true"`));
  expect(html.includes("Accent color")).toBe(["dark", "light", "system"].includes(mode));
});
it("appearance exposes saved custom accents without a new catalog or host settings", () => {
  const html = render(component().default, { props: { state: { mode: "dark", accentColor: "#123456", customAccentColors: ["#123456"] } } }).body;
  expect(html).toContain("#123456"); expect(html).toContain("Custom accent color"); expect(html).not.toContain("Equalizer"); expect(html).not.toContain("Connected PC");
});
it("appearance gestures delegate locally and reject malformed custom colors", () => {
  const { selectThemeMode, selectAccentColor, addCustomAccent } = component() as any;
  expect(selectThemeMode).toBeTypeOf("function"); expect(selectAccentColor).toBeTypeOf("function"); expect(addCustomAccent).toBeTypeOf("function");
  if (!selectThemeMode || !selectAccentColor || !addCustomAccent) return;
  const mode = vi.fn(), accent = vi.fn(), custom = vi.fn();
  for (const value of modes) selectThemeMode(value, mode);
  expect(mode.mock.calls.map(call => call[0])).toEqual(modes);
  selectThemeMode("unsupported", mode); expect(mode).toHaveBeenCalledTimes(modes.length);
  selectAccentColor("#123456", accent); expect(accent).toHaveBeenCalledExactlyOnceWith("#123456");
  selectAccentColor("not-a-color", accent); expect(accent).toHaveBeenCalledOnce();
  addCustomAccent("#abcdef", custom); addCustomAccent("bad", custom); expect(custom).toHaveBeenCalledExactlyOnceWith("#abcdef");
});

function declarationsAt(selector: string, width: number): Record<string, string> {
  const source = readFileSync(path, "utf8");
  const ast = parse(source, { modern: true }) as any;
  const values: Record<string, string> = {};
  function visit(nodes: any[]) {
    for (const node of nodes) {
      if (node.type === "Atrule" && node.name === "media") {
        const maximum = /max-width:\s*(\d+)px/.exec(node.prelude);
        if (!maximum || width <= Number(maximum[1])) visit(node.block.children);
      } else if (node.type === "Rule" && node.prelude.children.some((item: any) => source.slice(item.start, item.end) === selector)) {
        for (const declaration of node.block.children) if (declaration.type === "Declaration") values[declaration.property] = declaration.value;
      }
    }
  }
  visit(ast.css.children);
  return values;
}
it("appearance extraction keeps compact margins conditional instead of shrinking the desktop card", () => {
  expect(declarationsAt(".settings-section", 1280)["margin-bottom"]).toBe("var(--spacing-xl)");
  expect(declarationsAt(".settings-section", 1280)["margin-left"]).toBeUndefined();
  expect(declarationsAt(".settings-section", 600)["margin-bottom"]).toBe("var(--spacing-md)");
  expect(declarationsAt("button", 600)["min-height"]).toBe("48px");
});
