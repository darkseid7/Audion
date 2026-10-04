import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import * as themeCatalog from '../stores/theme';
import { runInNewContext } from 'node:vm';
import { compile } from 'svelte/compiler';
import { render } from 'svelte/server';
import * as svelte from 'svelte';
import { writable } from 'svelte/store';
import ts from 'typescript';
import { expect, it, vi } from 'vitest';
const runtime: string='svelte/internal/server';
const server=await import(runtime);

async function component(name:string, enabled:boolean, overrides:Record<string,unknown>={}) {
 const mounts:Function[]=[], destroys:Function[]=[];
 const native=vi.fn(); const desktop=writable(enabled);
 const modules:Record<string,any>={svelte:{...svelte,onMount:(fn:Function)=>mounts.push(fn),onDestroy:(fn:Function)=>destroys.push(fn)},'svelte/internal/server':server,
 '$lib/application/bootstrap':{desktopEffectsEnabled:desktop,applicationMode:writable(enabled?'desktop':'controller')},
 '$lib/api/tauri':{isTauri:()=>true},
 '$lib/stores/theme':themeCatalog,
 'svelte-i18n':{locale:writable('en'),_:writable((key:string,args?:{default?:string})=>args?.default??key)},
 '@tauri-apps/api/window':{getCurrentWindow:native},
 '@tauri-apps/plugin-global-shortcut':{register:native,unregister:native,unregisterAll:native}, ...overrides};
 const code=ts.transpileModule(compile(readFileSync(resolve('src/lib/components',`${name}.svelte`),'utf8'),{filename:`${name}.svelte`,generate:'server'}).js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
 for(const [,id] of code.matchAll(/require\("([^"]+)"\)/g)) {
  if(id in modules)continue;
  if(id.includes('presentation/') && id.endsWith('.svelte'))modules[id]=presentation(id);
  else if(id.endsWith('.svelte'))modules[id]={default:()=>{}};
  else if(id.startsWith('$lib/'))modules[id]=await import(/* @vite-ignore */ resolve('src/lib',id.slice(5)));
  else modules[id]=await import(/* @vite-ignore */ id);
 }
 function presentation(id:string) {
  const source=readFileSync(resolve('src/lib/components',id),'utf8');
  const js=ts.transpileModule(compile(source,{filename:id,generate:'server'}).js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
  const child:any={};
  runInNewContext(js,{exports:child,require:(name:string)=>{if(!(name in modules))throw new Error('Unexpected presentation dependency: '+name);return modules[name];}});
  return child;
 }
 const exports:any={};runInNewContext(code,{exports,require:(id:string)=>modules[id],console,setTimeout,clearTimeout,window:{addEventListener(){},removeEventListener(){}}});
 const html=render(exports.default).body;
 return {html,native,desktop,async mount(){for(const fn of mounts){const cleanup=fn();if(typeof cleanup==='function')destroys.push(cleanup);}},dispose(){for(const fn of destroys)fn();}};
}
it('expanded controller SSR never acquires a native window or renders PC controls',async()=>{
 const view=await component('TitleBar',false);await view.mount();view.dispose();
 expect(view.native).not.toHaveBeenCalled();expect(view.html).not.toContain('Minimize to tray');expect(view.html).not.toContain('data-tauri-drag-region');
});
it('controller shortcut mount and destroy do not register or unregister OS shortcuts',async()=>{
 const view=await component('GlobalShortcuts',false);await view.mount();view.dispose();await Promise.resolve();expect(view.native).not.toHaveBeenCalled();
});
it('desktop owns late shortcut registration cleanup and ignores callbacks after disposal',async()=>{
 let complete!:()=>void;let callback:any;const unregister=vi.fn(async()=>{}), togglePlay=vi.fn();
 const register=vi.fn((_keys:unknown,fn:Function)=>{callback=fn;return new Promise<void>(r=>complete=r);});
 const view=await component('GlobalShortcuts',true,{'@tauri-apps/plugin-global-shortcut':{register,unregister,unregisterAll:vi.fn(async()=>{})},'$lib/stores/player':{togglePlay,nextTrack:vi.fn(),previousTrack:vi.fn()}});
 await view.mount();await vi.waitFor(()=>expect(register).toHaveBeenCalled());view.dispose();complete();await Promise.resolve();await Promise.resolve();
 callback({state:'Pressed',shortcut:'MediaPlayPause'});expect(togglePlay).not.toHaveBeenCalled();expect(unregister).toHaveBeenCalled();
});

it('overlay back dismissal is LIFO and restores focus without swallowing root back',async()=>{
 const ui=await import('./controller-ui');
 const focus=vi.fn(); const origin={isConnected:true,getClientRects:()=>[{}],focus};
 const document={activeElement:origin,addEventListener(){},removeEventListener(){}};vi.stubGlobal('document',document);
 const events=new Map<string,Function>();const first={focus:vi.fn(),isConnected:true,getClientRects:()=>[{}]};
 const node={querySelectorAll:()=>[first],addEventListener:(key:string,fn:Function)=>events.set(key,fn),removeEventListener:vi.fn(),contains:(target:unknown)=>target===first,focus:vi.fn()};
 const closeA=vi.fn(),closeB=vi.fn();
 const a=ui.controllerOverlay(node as any,{close:closeA});const b=ui.controllerOverlay(node as any,{close:closeB});await Promise.resolve();
 expect(ui.dismissControllerOverlay()).toBe(true);expect(closeB).toHaveBeenCalledOnce();expect(closeA).not.toHaveBeenCalled();
 b.destroy();a.destroy();expect(focus).toHaveBeenCalled();expect(ui.dismissControllerOverlay()).toBe(false);vi.unstubAllGlobals();
});
it('overlay keyboard traps focus and Escape closes only the top surface',async()=>{
 const ui=await import('./controller-ui'); const first={focus:vi.fn(),isConnected:true,getClientRects:()=>[{}]},last={focus:vi.fn(),isConnected:true,getClientRects:()=>[{}]};
 const document={activeElement:last,addEventListener(){},removeEventListener(){}};vi.stubGlobal('document',document);const events=new Map<string,Function>();
 const node={querySelectorAll:()=>[first,last],addEventListener:(key:string,fn:Function)=>events.set(key,fn),removeEventListener:vi.fn(),contains:(target:unknown)=>target===first||target===last,focus:vi.fn()};const close=vi.fn();
 const overlay=ui.controllerOverlay(node as any,{close});await Promise.resolve();
 const event={key:'Tab',shiftKey:false,preventDefault:vi.fn(),stopPropagation:vi.fn()};events.get('keydown')!(event);expect(first.focus).toHaveBeenCalled();expect(event.preventDefault).toHaveBeenCalled();
 events.get('keydown')!({...event,key:'Escape'});expect(close).toHaveBeenCalledOnce();overlay.destroy();vi.unstubAllGlobals();
});
it.each([[true,'compact'],[false,'expanded']] as const)('controller SSR exposes %s navigation without a PC titlebar',async(compact,layout)=>{
 const view=await component('ControllerShell',false,{'$lib/stores/mobile':{isMobile:writable(compact)}});
 expect(view.html).toContain(`data-layout="${layout}"`);expect(view.html).toContain('aria-label="Music library"');expect(view.html).not.toContain('Search PC library');expect(view.html).not.toContain('Minimize to tray');view.dispose();
});
it('controller MainView never acquires a desktop drag/drop webview',async()=>{
 const acquire=vi.fn(async()=>({onDragDropEvent:vi.fn(async()=>()=>{})}));
 const view=await component('MainView',false,{'@tauri-apps/api/webview':{getCurrentWebview:acquire}});await view.mount();await Promise.resolve();view.dispose();expect(acquire).not.toHaveBeenCalled();
});
it('late desktop drag/drop registration is released after MainView teardown',async()=>{
 let complete!:(unlisten:()=>void)=>void;
 const attach=vi.fn(()=>new Promise<()=>void>(resolve=>complete=resolve)),unlisten=vi.fn();
 const view=await component('MainView',true,{'@tauri-apps/api/webview':{getCurrentWebview:async()=>({onDragDropEvent:attach})}});
 await view.mount();await vi.waitFor(()=>expect(attach).toHaveBeenCalledOnce());view.dispose();complete(unlisten);await vi.waitFor(()=>expect(unlisten).toHaveBeenCalledOnce());
});
it('controller root back is not consumed, but a registered local search/history handler is',async()=>{
 const ui=await import('./controller-ui');expect(ui.handleControllerBack()).toBe(false);
 const stop=ui.registerControllerBack(()=>true);expect(ui.handleControllerBack()).toBe(true);stop();expect(ui.handleControllerBack()).toBe(false);
});
it('a maximize completion cannot query a window after TitleBar teardown',async()=>{
 const source=readFileSync(resolve('src/lib/components/TitleBar.svelte'),'utf8').match(/<script lang="ts">([\s\S]*?)<\/script>/)![1];
 const ast=ts.createSourceFile('titlebar',source,ts.ScriptTarget.Latest,true);
 const fn=ast.statements.find(node=>ts.isFunctionDeclaration(node)&&node.name?.text==='toggleMaximize')!;
 let complete!:()=>void;const isMaximized=vi.fn(async()=>false);
 const scope:any={get:()=>true,desktopEffectsEnabled:{},appWindow:{toggleMaximize:()=>new Promise<void>(r=>complete=r),isMaximized},isMaximized:false};
 const code=ts.transpileModule(fn.getText(ast),{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText;
 const toggle=runInNewContext(`${code};toggleMaximize`,scope);const pending=toggle();scope.appWindow=null;complete();await expect(pending).resolves.toBeUndefined();expect(isMaximized).not.toHaveBeenCalled();
});
it('connected PC settings panel appears only in Settings and frees the normal browse header',async()=>{
 const state={ready:true,status:'connected',grants:{control:true},snapshot:{capabilities:{queries:['albums']},outputs:[{output:{kind:'pc'},name:'Studio PC'}],playback:{track:null}}};
 const stores=(type:string)=>({'$lib/application/controller/bootstrap':{controllerState:writable(state)},'$lib/stores/view':{currentView:writable({type}),navigationHistory:writable({canGoBack:false}),goBack:vi.fn()}});
 const browse=await component('ControllerShell',false,stores('albums'));expect(browse.html).not.toContain('Connected · PC settings');expect(browse.html).not.toContain('class="connection"');expect(browse.native).not.toHaveBeenCalled();browse.dispose();
 const settings=await component('ControllerShell',false,stores('settings'));expect(settings.html).toContain('Connected · PC settings');expect(settings.html).toContain('aria-label="PC connection settings"');expect(settings.native).not.toHaveBeenCalled();settings.dispose();
});
it('offline browse keeps a compact route to connection Settings without the old expandable strip',async()=>{
 const offline=await component('ControllerShell',false,{'$lib/application/controller/bootstrap':{controllerState:writable({ready:false,status:'disconnected',grants:{control:false},snapshot:null})},'$lib/stores/view':{currentView:writable({type:'albums'}),navigationHistory:writable({canGoBack:false}),goBack:vi.fn()}});
 expect(offline.html).toContain('Connection settings');expect(offline.html).not.toContain('<summary>PC connection</summary>');expect(offline.native).not.toHaveBeenCalled();offline.dispose();
});
