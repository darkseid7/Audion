import { readFileSync, existsSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { runInNewContext } from 'node:vm';
import { compile } from 'svelte/compiler';
import { render } from 'svelte/server';
import * as svelte from 'svelte';
import { writable } from 'svelte/store';
import ts from 'typescript';
import { expect, it, vi } from 'vitest';
const runtime='svelte/internal/server'; const server=await import(runtime);
function fixture(track: any) {
 const artwork=vi.fn(),native=vi.fn(()=>{throw Error('Native effects are forbidden');});
 const modules:Record<string,any>={svelte,'svelte/internal/server':server,
 '$lib/application/controller-ui':{controllerOverlay:()=>{}},
 '$lib/application/controller/bootstrap':{controllerState:writable({ready:true,snapshot:{playback:{status:'paused',track,position:12,duration:100,volume:.5,shuffle:false,repeat:'none'}}})},
 '$lib/application/capabilities':{canExecute:()=>false},'$lib/stores/ui':{toggleFullScreen:native,toggleQueue:native,isFullScreen:writable(false)},
 '$lib/stores/view':{goToAlbumDetail:native,goToArtistDetail:native},
 './ControllerArtwork.svelte':{default:(_:any,props:any)=>artwork(props.reference)},'./ControllerFeedback.svelte':{default:()=>{}},'./ConnectPanel.svelte':{default:()=>{}}
 };
 function load(path:string):any {const result:any={};const source=readFileSync(path,'utf8');const code=compile(source,{filename:path,generate:'server'}).js.code;
 runInNewContext(ts.transpileModule(code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports:result,require:(id:string)=>{if(id in modules)return modules[id];if(id.endsWith('.svelte'))return load(resolve(dirname(path),id));throw Error(`Unexpected presentation dependency ${id}`);}});return result;}
 return {html:render(load(resolve('src/lib/components/ControllerTransport.svelte')).default,{props:{execute:native}}).body,artwork,native};
}
const track={id:7,title:'All Bout U',artist:'2Pac',quality:{format:'FLAC',bitrate:null,badges:['FLAC','44.1kHz','16bit']},artwork:{resourceId:'track:7',revision:2}};
it('controller uses shared desktop metadata hooks and exact authoritative artwork reference',()=>{
 const f=fixture(track);for(const hook of ['desktop-track-info','album-art','track-details','track-title','player-audio-chips','player-audio-chip'])expect(f.html).toContain(hook);
 expect(f.html).toContain('44.1kHz');expect(f.html).toContain('16bit');expect(f.artwork).toHaveBeenCalledWith(track.artwork);expect(f.native).not.toHaveBeenCalled();
});
it('known format remains visible when the host supplies an empty badge list without invented sample rate',()=>{
 const f=fixture({...track,artwork:undefined,quality:{format:'FLAC',bitrate:null,badges:[]}});
 expect(f.html).toContain('FLAC');expect(f.html).not.toContain('44.1kHz');expect(f.html).not.toContain('16bit');expect(f.native).not.toHaveBeenCalled();
});
it('shared player is passive and native wrapper retains its owned cover and metadata handlers',()=>{
 const path=resolve('src/lib/components/presentation/Player.svelte');expect(existsSync(path)).toBe(true);
 const source=readFileSync(path,'utf8');expect(source).not.toMatch(/\$lib\/(api|stores|application\/controller)/);expect(source).toContain('width: 96px');expect(source).toContain('player-audio-chip');
 const native=readFileSync(resolve('src/lib/components/PlayerBar.svelte'),'utf8');expect(native).toContain('<Player');expect(native).toContain('on:error={() => (imageLoadFailed = true)}');expect(native).toContain('currentTrackAudioInfo?.sampleRate');
 for(const generate of ['server','client'] as const)expect(()=>compile(native,{filename:'PlayerBar.svelte',generate})).not.toThrow();
});
it('transport controls use desktop SVGs instead of Unicode glyphs and remain capability-disabled',()=>{
 const f=fixture(track);expect(f.html).toContain('M6 6h2v12H6zm3.5 6l8.5 6V6z');expect(f.html).toContain('M6 18l8.5-6L6 6v12zM16 6v12h2V6h-2z');expect(f.html).not.toContain('⇄');expect(f.html).not.toContain('|◀');expect(f.html).toContain('disabled');expect(f.native).not.toHaveBeenCalled();
});
it('expanded controller bar keeps desktop height and its metadata remains a single shared instance',()=>{
 const source=readFileSync(resolve('src/lib/components/ControllerTransport.svelte'),'utf8');expect(source).toContain('height: calc(var(--player-height) + 60px)');
 expect((source.match(/<Player\s/g)||[])).toHaveLength(1);
 for(const name of ['ControllerTransport','presentation/Player'])for(const generate of ['server','client'] as const)expect(()=>compile(readFileSync(resolve('src/lib/components',name+'.svelte'),'utf8'),{filename:name+'.svelte',generate})).not.toThrow();
});
