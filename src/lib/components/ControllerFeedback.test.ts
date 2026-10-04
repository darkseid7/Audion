import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { compile } from "svelte/compiler";
import { render } from "svelte/server";
import { writable } from "svelte/store";
import ts from "typescript";
import { it, expect, vi } from "vitest";
const runtime: string = "svelte/internal/server";
const server = await import(runtime);
it("visibly retains pending, failed, partial and unknown outcomes with explicit dismissal",()=>{
 const entries=[
  {id:1,action:"seek",status:"pending",message:"Waiting for PC"},
  {id:2,action:"queue remove",status:"error",message:"Queue failed. Effects already confirmed: Output stopped."},
  {id:3,action:"select output",status:"unknown",message:"Unknown outcome; not replayed"}
 ];
 const actions={outcomes:writable(entries),admissionError:writable("Dismiss outcomes before submitting"),dismiss:vi.fn()};
 const source=readFileSync(new URL("./ControllerFeedback.svelte",import.meta.url),"utf8");
 const code=ts.transpileModule(compile(source,{filename:"ControllerFeedback.svelte",generate:"server"}).js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
 const exports:any={};runInNewContext(code,{exports,require:(id:string)=>id==="svelte/internal/server"?server:{viewActions:actions}});
 const html=render(exports.default).body;
 for(const entry of entries)expect(html).toContain(entry.message);
 expect(html).toContain("Dismiss queue remove outcome");expect(html).not.toContain("Dismiss seek outcome");
 expect(html).toContain("Dismiss outcomes before submitting");
});
