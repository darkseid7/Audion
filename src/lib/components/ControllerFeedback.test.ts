import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { compile } from "svelte/compiler";
import { render } from "svelte/server";
import { get, writable } from "svelte/store";
import type { Component } from "svelte";
import ts from "typescript";
import { it, expect, vi } from "vitest";
import type { ActionOutcome } from "../application/view-actions";
const runtime: string = "svelte/internal/server";
const server = await import(runtime);
function renderFeedback(entries: ActionOutcome[], admissionError = "") {
 const actions = { outcomes: writable(entries), admissionError: writable(admissionError), dismiss: vi.fn() };
 const source = readFileSync(new URL("./ControllerFeedback.svelte", import.meta.url), "utf8");
 const code = ts.transpileModule(compile(source, { filename: "ControllerFeedback.svelte", generate: "server" }).js.code, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
 const exports: { default?: Component<{ actions: typeof actions }> } = {};
 runInNewContext(code, { exports, require: (id: string) => id === "svelte/internal/server" ? server : { viewActions: actions } });
 return { html: render(exports.default!, { props: { actions } }).body, actions };
}
it("retains failed, partial and unknown outcomes with explicit dismissal while hiding routine notices", () => {
 const entries: ActionOutcome[] = [
  { id: 1, action: "seek", status: "pending", message: "Waiting for PC" },
  { id: 2, action: "queue remove", status: "error", message: "Queue failed. Effects already confirmed: Output stopped." },
  { id: 3, action: "select output", status: "unknown", message: "Unknown outcome; not replayed" },
  { id: 4, action: "pause", status: "applied", message: "Confirmed by PC" }
 ];
 const { html, actions } = renderFeedback(entries, "Dismiss outcomes before submitting");
 expect(html).toContain(entries[1].message); expect(html).toContain(entries[2].message);
 expect(html).not.toContain(entries[0].message); expect(html).not.toContain(entries[3].message);
 expect(html).toContain("Dismiss queue remove outcome"); expect(html).toContain("Dismiss select output outcome");
 expect(html).not.toContain("Dismiss seek outcome"); expect(html).not.toContain("Dismiss pause outcome");
 expect(html).toContain("Dismiss outcomes before submitting"); expect(html).toContain('role="alert"');
 // Filtering presentation must not discard pending jobs or acknowledged outcomes.
 expect(get(actions.outcomes)).toEqual(entries); expect(actions.dismiss).not.toHaveBeenCalled();
});
it.each(["idle", "pending", "applied"] as const)("does not reserve a feedback panel for %s outcomes", status => {
 const { html } = renderFeedback([{ id: 1, action: "seek", status, message: "Routine outcome" }]);
 expect(html).not.toContain("action-feedback"); expect(html).not.toContain("Routine outcome");
});
it("shows admission errors even with no action outcomes", () => {
 const { html } = renderFeedback([], "Playback permission is required.");
 expect(html).toContain("action-feedback"); expect(html).toContain("Playback permission is required."); expect(html).toContain('role="alert"');
});
it("renders no feedback panel when there are no errors or outcomes", () => {
 expect(renderFeedback([]).html).not.toContain("action-feedback");
});
