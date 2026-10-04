import { existsSync, readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { runInNewContext } from "node:vm";
import { compile } from "svelte/compiler";
import { render } from "svelte/server";
import ts from "typescript";
import * as svelte from "svelte";
import { expect, it } from "vitest";
const runtime:string="svelte/internal/server";const server=await import(runtime);
function load(path:string):any {
 if(!existsSync(path))return {default:()=>{}};
 const result:any={};const js=compile(readFileSync(path,"utf8"),{filename:path,generate:"server"}).js.code;
 runInNewContext(ts.transpileModule(js,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports:result,require:(id:string)=>{
 if(id===runtime)return server;if(id==="svelte")return svelte;if(id.endsWith('.svelte'))return load(resolve(dirname(path),id));throw Error(`Unexpected passive dependency:${id}`);
 }});return result;
}
const base=resolve('src/lib/components/presentation');
it.each(['grid','list'])("album browser extracts desktop toolbar SVGs and stable browse slot for %s",albumView=>{
 const html=render(load(resolve(base,'AlbumBrowser.svelte')).default,{props:{albumView,searchQuery:'Miles',sortOptions:[{value:'artist-asc',label:'Artista (A-Z)'}],selectedSortLabel:'Artista (A-Z)',labels:{viewMode:'Album view',gridView:'Grid view',listView:'List view'},$$slots:{default:(renderer:any)=>renderer.push('same-grid-instance')}}}).body;
 expect(html).toContain('albums-toolbar-wrap');expect(html).toContain('search-filter-group');expect(html).toContain('rect x="3" y="3" width="7" height="7"');expect(html).toContain('data-layout="'+albumView+'"');expect(html).toContain('same-grid-instance');expect(html).not.toContain('▦');
});
import { writable } from 'svelte/store';
import { vi } from 'vitest';
function controllerBrowseFixture(enabled = true, context: any = undefined, metadataValue: any = {counts:new Map(),album:null,loading:false,stale:false,error:''}, options: any = {}) {
 const artistNavigation=vi.fn(); const artwork=vi.fn(),execute=vi.fn(),native=vi.fn(()=>{throw Error('Unexpected native execution');});
 const album={id:7,name:'Kind of Blue',artist:'Miles Davis',year:1959,qualityBadges:['FLAC','96kHz','24bit'],sortSummary:{artist:'Miles Davis',year:1959,name:'Kind of Blue',dateAdded:null},artwork:{resourceId:'album:7',revision:4}};
 const page={state:writable({items:options.items ?? (context ? [] : [album]),revision:options.revision ?? 4,nextCursor:null,hasEarlier:false,loading:false,error:''}),setQuery:vi.fn(),more:vi.fn(async()=>{}),refresh:vi.fn(),dispose:vi.fn()};
 const details={...page,dispose:vi.fn(),state:writable({items:[],detail:context?.type==='album' ? {album,originalYear:1959,trackCount:5,liked:true} : undefined,revision:4,nextCursor:null,loading:false,error:''})};let calls=0;
 const metadata={state:writable(metadataValue),setQuery:vi.fn(),dispose:vi.fn()}; const cleanup: Array<()=>void> = [];
 const modules:any={'$lib/application/controller/browse-metadata':{createControllerBrowseMetadata:()=>metadata},'svelte':{...svelte,onMount:()=>{},onDestroy:(callback:()=>void)=>cleanup.push(callback)},'svelte/internal/server':server,
 '$lib/application/controller/bootstrap':{controllerState:writable({ready:true,grants:{control:true},snapshot:{revisions:{libraryRevision:4},capabilities:{intents:['play_album','queue_entity','pause','resume'],queries:['albums']},playback:{status:'stopped',context:null}}})},
 '$lib/application/capabilities':{canExecute:()=>enabled},'$lib/stores/library':{createControllerLibraryPage:()=>++calls===1?details:page},'$lib/stores/search':{createControllerSearchPage:native},'$lib/stores/view':{goToAlbumDetail:vi.fn(),goToArtistDetail:artistNavigation,goToPlaylistDetail:vi.fn()},
 './ControllerArtwork.svelte':{default:(_:any,props:any)=>{artwork(props.reference);}},
 };
 const captures:any[]=[];
 function real(path:string):any {
 const result:any={};const js=compile(readFileSync(path,'utf8'),{filename:path,generate:'server'}).js.code;
 runInNewContext(ts.transpileModule(js,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports:result,require:(id:string)=>{
 if(id in modules)return modules[id];if(id==='$lib/application/presentation/browse') {const result:any={};runInNewContext(ts.transpileModule(readFileSync('src/lib/application/presentation/browse.ts','utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS}}).outputText,{exports:result});return result;}
 if(id.endsWith('.svelte')){const child=real(resolve(dirname(path),id));return {...child,default:(renderer:any,props:any)=>{captures.push({id,props});return child.default(renderer,props);}};}throw Error(`Unexpected owner dependency:${id}`);
 }});return result;
 }
 return {html:render(real(resolve('src/lib/components/ControllerBrowse.svelte')).default,{props:{query:options.query ?? (context ? {type:'liked_tracks'} : {type:'albums'}),context,heading:context ? 'Liked Songs' : '',execute,enqueue:native}}).body,captures,execute,native,artwork,page,artistNavigation,metadata,dispose:()=>cleanup.forEach(callback=>callback())};
}
it('controller album browsing uses desktop toolbar/card metadata and untouched artwork reference',()=>{
 const fixture=controllerBrowseFixture();expect(fixture.html).toContain('albums-toolbar-wrap');expect(fixture.html).toContain('Miles Davis · 1959');expect(fixture.html).toContain('class="audio-chip');expect(fixture.html).toContain('96kHz');expect(fixture.artwork).toHaveBeenCalledWith({resourceId:'album:7',revision:4});expect(fixture.native).not.toHaveBeenCalled();
});

it('real controller album gesture issues only host album ID intent and honors capability rejection',()=>{
 const allowed=controllerBrowseFixture(); const card=allowed.captures.find(child=>child.id==='./presentation/EntityGrid.svelte')!; card.props.onPlay();
 expect(allowed.execute).toHaveBeenCalledExactlyOnceWith({type:'play_album',albumId:7,playMode:'all'});expect(allowed.native).not.toHaveBeenCalled();
 const denied=controllerBrowseFixture(false);denied.captures.find(child=>child.id==='./presentation/EntityGrid.svelte')!.props.onPlay();expect(denied.execute).not.toHaveBeenCalled();
});
it('unsupported_context_action_explains_reason through touch/keyboard disclosure without executing',()=>{
 const action={id:'delete',label:'Delete Album',availability:{enabled:false,reason:'Deleting files is managed on the PC.'},run:()=>{throw Error('Must not execute');}};
 const html=render(load(resolve(base,'EntityGrid.svelte')).default,{props:{primaryText:'Kind of Blue',actions:[action]}}).body;
 expect(html).toContain('Actions');expect(html).toContain('Deleting files is managed on the PC.');expect(html).toContain('disabled');expect(html).toMatch(/<summary[^>]*>/);
});
it.each(['album','artist','playlist'])('detail header preserves existing %s presentation hooks and owned slots',kind=>{
 const html=render(load(resolve(base,'EntityDetail.svelte')).default,{props:{kind,$$slots:{default:(renderer:any)=>renderer.push('<h1>Authoritative title</h1>')}}}).body;
 expect(html).toContain(`class="${kind}-header`);expect(html).toContain('Authoritative title');
});
it('track presentation retains desktop row columns and unknown duration/play-count is not fabricated',()=>{
 const track={id:3,title:'So What',artist:'Miles Davis',album:'Kind of Blue',albumId:7,duration:null,trackNumber:1,discNumber:1,quality:{format:'FLAC',bitrate:null,badges:['FLAC','24bit']}};
 const html=render(load(resolve(base,'TrackRows.svelte')).default,{props:{tracks:[track],showAlbum:true}}).body;
 for(const hook of ['track-list','list-header','track-row','col-cover','col-title','col-duration','col-plays'])expect(html).toContain(hook);
 expect(html).toContain('So What');expect(html).toContain('24bit');expect(html).toContain('Not supplied by the PC');expect(html).not.toMatch(/col-plays[^>]*>0</);
});
it.each(['AlbumGrid','ArtistGrid','PlaylistView','AlbumDetail','ArtistDetail','PlaylistDetail','LikedSongs','TrackList','ControllerBrowse','presentation/AlbumBrowser','presentation/EntityGrid','presentation/EntityDetail','presentation/TrackRows'])('%s compiles for SSR/client without bootstrap/native execution',name=>{
 const source=readFileSync(resolve('src/lib/components',name+'.svelte'),'utf8');for(const generate of ['server','client'] as const)expect(()=>compile(source,{filename:name+'.svelte',generate})).not.toThrow();
});
it('controller album context queues host entity IDs without local track execution',()=>{
 const fixture=controllerBrowseFixture(); const card=fixture.captures.find(child=>child.id==='./presentation/EntityGrid.svelte')!;
 card.props.actions.find((action:any)=>action.id==='next').run();
 expect(fixture.execute).toHaveBeenCalledExactlyOnceWith({type:'queue_entity',entity:{type:'album',albumId:7,playMode:'all'},placement:'next'});expect(fixture.native).not.toHaveBeenCalled();
 const denied=controllerBrowseFixture(false);denied.captures.find(child=>child.id==='./presentation/EntityGrid.svelte')!.props.actions.find((action:any)=>action.id==='next').run();expect(denied.execute).not.toHaveBeenCalled();
});
it('controller retains bounded-page recovery and virtualized browse geometry instead of duplicating owner policy',()=>{
 const fixture=controllerBrowseFixture();const grid=fixture.captures.find(child=>child.id==='./Virtualizedgrid.svelte')!;
 expect(grid.props.cardWidthDesktop).toBe(240);expect(grid.props.cardWidthMobile).toBe(170);expect(grid.props.cardHeightDesktop).toBe(380);expect(grid.props.hasMore).toBe(false);
 const source=readFileSync(resolve('src/lib/components/ControllerBrowse.svelte'),'utf8');expect(source).toContain('page.dispose();details.dispose()');expect(source).toContain('Showing the latest 1,000 loaded entries.');expect(source).toContain('Back to first page');expect(source).not.toContain('{#key layout}');
});
it('compact track actions remain in the visible title column without permanent row disclosures',()=>{
 const track={id:3,title:'So What',artist:'Miles Davis',album:null,albumId:null,duration:null,quality:{badges:[]}};
 const html=render(load(resolve(base,'TrackRows.svelte')).default,{props:{tracks:[track],mobileViewMode:'library'}}).body;
 expect(html).toMatch(/class="col-title(?:\s[^"\r\n]*)?"[\s\S]*?aria-label="Actions for So What"[\s\S]*?class="artist-with-format"/);
 expect(html).not.toContain('<details');
 const source=readFileSync(resolve(base,'TrackRows.svelte'),'utf8');
 expect(source).toContain('showModal()');expect(source).toContain('height: 58px');
});
it('controller liked presentation keeps Play All and Shuffle outside the centered header',()=>{
 const html=controllerBrowseFixture(true,{type:'liked'}).html;
 expect(html).toMatch(/<\/header>[\s\S]*class="liked-controls"/);
 expect(html).toContain('controls-wrapper');expect(html).toContain('btn-icon');expect(html).toContain('Play All');
 expect(html).not.toContain('818 songs');
});
it('controller album detail preserves authoritative metadata and desktop quality hooks',()=>{
 const html=controllerBrowseFixture(true,{type:'album',albumId:7,playMode:'all'}).html;
 expect(html).toContain('5 songs');expect(html).toContain('album-audio-meta');expect(html).toContain('album-audio-chip format-chip');expect(html).toContain('96kHz');expect(html).toContain('Total duration not supplied by the PC.');
});
it('controller track play button resets native button chrome and menu does not expand the title line',()=>{
 const source=readFileSync(resolve(base,'TrackRows.svelte'),'utf8');
 expect(source).toMatch(/button\.track-name\s*\{[^}]*background:\s*none;[^}]*border:\s*0;/);
 expect(source).toMatch(/\.row-menu-button\s*\{[^}]*position:\s*absolute;/);
});
it('controller rows and artist header match existing desktop selector contracts',()=>{
 const track={id:3,title:'So What',artist:'Miles Davis',album:null,albumId:null,duration:null,liked:true,quality:{badges:['FLAC','24bit']}};
 const html=render(load(resolve(base,'TrackRows.svelte')).default,{props:{tracks:[track]}}).body;
 expect(html).toContain('track-quality-row');expect(html).toMatch(/class="col-like liked(?:\s[^"\r\n]*)?"/);
 const artist=controllerBrowseFixture(true,{type:'artist',artistName:'Miles Davis'}).html;
 expect(artist).toContain('artist-name');expect(artist).not.toContain('artist-title');
});
it('controller album header uses desktop info and known like presentation without inventing mutations',()=>{
 const html=controllerBrowseFixture(true,{type:'album',albumId:7,playMode:'all'}).html;
 expect(html).toContain('btn-info');const likeButton=html.match(/<button class="(btn-like-album[^"\r\n]*)"[^>]*disabled[^>]*>/);expect(likeButton).not.toBeNull();expect(likeButton![1].split(/\s+/)).toContain("liked");
 expect(html).toContain('Album like changes are managed on the PC.');expect(html).toContain('btn-filter-liked');
});
it('entity context trigger uses quiet desktop icon rather than visible Actions text over artwork',()=>{
 const html=render(load(resolve(base,'EntityGrid.svelte')).default,{props:{primaryText:'Kind of Blue',actions:[{id:'pin',label:'Pin to Top',availability:{enabled:false,reason:'Managed on PC'}}]}}).body;
 expect(html).toContain('aria-label="Entity actions"');expect(html).toContain('context-menu-icon');expect(html).not.toMatch(/<summary[^>]*>Actions<\/summary>/);
});
it.each(['album','artist','playlist'])('controller %s detail restores the desktop back control',type=>{
 const html=controllerBrowseFixture(true,{type,albumId:7,artistName:'Miles Davis',playlistId:9,playMode:'all'}).html;
 expect(html).toMatch(/class="back-btn(?:\s[^"\r\n]*)?"/);expect(html).toContain('aria-label="Go back"');
});
it('known track likes use the native desktop heart SVG and unknown state stays explicit',()=>{
 const track={id:3,title:'So What',artist:'Miles Davis',album:null,albumId:null,duration:null,liked:true,quality:{badges:[]}};
 const html=render(load(resolve(base,'TrackRows.svelte')).default,{props:{tracks:[track]}}).body;
 expect(html).toContain('aria-label="Liked; changes managed on the PC"');expect(html).toMatch(/width="16" height="16" fill="currentColor" stroke="currentColor"/);expect(html).not.toContain('♥');
 const unknown=render(load(resolve(base,'TrackRows.svelte')).default,{props:{tracks:[{...track,liked:undefined}]}}).body;
 expect(unknown).toContain('Like state not supplied by the PC');
});
it('album metadata reserves room for title artist and quality tags at stretched grid widths',()=>{
 const fixture=controllerBrowseFixture();
 const card=fixture.captures.find(child=>child.id==='./presentation/EntityGrid.svelte')!;
 expect(card.props.albumMetadata).toBe(true);
 const source=readFileSync(resolve(base,'EntityGrid.svelte'),'utf8');
 expect(source).toMatch(/\.album-metadata[^}]*\.info[^}]*flex-shrink:\s*0/);
 expect(source).toMatch(/\.album-metadata[^}]*\.cover[^}]*max-height:\s*calc\(100% - var\(--album-metadata-reserve\)\)/);
 expect(fixture.html).toContain('FLAC');expect(fixture.html).toContain('96kHz');expect(fixture.html).toContain('24bit');
});
it('dense controller album metadata does not inherit44px inline artist button height; artist remains in touch menu',()=>{
 const f=controllerBrowseFixture();const card=f.captures.find(child=>child.id==='./presentation/EntityGrid.svelte')!;
 expect(card.props.secondaryAction).toBeUndefined();expect(f.html).not.toContain('secondary-link');
 const artist=card.props.actions.find((action:any)=>action.id==='artist');expect(artist).toBeDefined();artist.run();
 expect(f.artistNavigation).toHaveBeenCalledExactlyOnceWith('Miles Davis');expect(f.native).not.toHaveBeenCalled();
 const styles=readFileSync(resolve(base,'EntityGrid.svelte'),'utf8');expect(styles).toContain('gap: var(--spacing-xs)');
});

it('album detail uses one controller scroll surface and compact header without changing desktop defaults', () => {
 const source=readFileSync('src/lib/components/ControllerBrowse.svelte','utf8');
 expect(source).toContain('class:album-detail-scroll={context?.type === "album"}');
 expect(source).toContain('disableVirtualScroll={context?.type === "album"}');
 expect(source).toContain('.browse-view.album-detail-scroll');
 const rowHtml=render(load(resolve(base,'TrackRows.svelte')).default,{props:{disableVirtualScroll:true}}).body;
 expect(rowHtml).toMatch(/class="list-body[^\"]*no-scroll/);
 const header=render(load(resolve(base,'EntityDetail.svelte')).default,{props:{kind:'album',compact:true}}).body;
 expect(header).toContain('controller-compact');
 const desktop=render(load(resolve(base,'EntityDetail.svelte')).default,{props:{kind:'album'}}).body;
 expect(desktop).not.toContain('controller-compact');
});

it('album Play contains an accessible chevron disclosure instead of More actions', () => {
 const fixture=controllerBrowseFixture(true,{type:'album',albumId:7});
 expect(fixture.html).toContain('album-play-split');
 expect(fixture.html).toContain('aria-label="Playback options"');
 expect(fixture.html).not.toContain('More actions');
 for(const label of ['Play Next','Add to Queue','Add to End'])expect(fixture.html).toContain(label);
 expect(fixture.html).toContain('Show only liked tracks');
 expect(fixture.native).not.toHaveBeenCalled();
 const denied=controllerBrowseFixture(false,{type:'album',albumId:7});
 expect(denied.html).toContain('Playback and queue actions require permission and support from the PC.');
 expect(denied.html).toMatch(/disabled[^>]*>Play Next/);
});

it('album Play chevron shares a seamless pill without a divider', () => {
 const source=readFileSync('src/lib/components/ControllerBrowse.svelte','utf8');
 const rule=source.match(/\.play-options > summary \{([^}]+)\}/)![1];
 expect(rule).not.toContain('border-left');
});

it('album Play keeps original primary styling with transparent internal targets', () => {
 const fixture=controllerBrowseFixture(true,{type:'album',albumId:7});
 expect(fixture.html).toContain('class="btn-primary album-play-split');
 const source=readFileSync('src/lib/components/ControllerBrowse.svelte','utf8');
 expect(source).toContain('.album-play-split .play-main { background: transparent;');
 expect(source).not.toContain('.play-options > summary:hover { background: var(--accent-hover); }');
});

it('open Play dropdown stacks above track headers even with primary hover transform', () => {
 const source=readFileSync('src/lib/components/ControllerBrowse.svelte','utf8');
 expect(source).toContain('.album-play-split:has(.play-options[open]) { z-index: 30; }');
});

it('album header participates in dropdown stacking without changing closed layout', () => {
 expect(readFileSync('src/lib/components/ControllerBrowse.svelte','utf8')).toContain(':global(.album-header:has(.play-options[open])) { z-index: 30; }');
});

it('album rows group title and quality inside one touch target without repeated artist', () => {
 const track={id:3,title:'So What',artist:'Miles Davis',album:'Kind of Blue',albumId:7,duration:120,trackNumber:1,discNumber:1,quality:{format:'FLAC',bitrate:null,badges:['FLAC','24bit']}};
 const html=render(load(resolve(base,'TrackRows.svelte')).default,{props:{tracks:[track],mobileViewMode:'album'}}).body;
 expect(html).toMatch(/class="track-name[^\"]*album-title-target/);
 const target=html.match(/<button class="track-name[^>]*>([\s\S]*?)<\/button>/)![1];
 expect(target).toContain('So What');expect(target).toContain('24bit');
 expect(html).not.toContain('class="track-artist');
 const library=render(load(resolve(base,'TrackRows.svelte')).default,{props:{tracks:[track]}}).body;
 expect(library).toContain('Miles Davis');
});

it('track actions render themed action rows with song context and deduplicated restrictions', () => {
 const track={id:3,title:'So What',artist:'Miles Davis',album:'Kind of Blue',albumId:7,duration:120,trackNumber:1,discNumber:1,quality:{format:'FLAC',bitrate:null,badges:['FLAC']}};
 const html=render(load(resolve(base,'TrackRows.svelte')).default,{props:{selectedTrack:track,queueAvailability:()=>({enabled:false,reason:'Control permission is required.'})}}).body;
 expect(html).toContain('aria-label="Actions for So What"');
 expect(html).toContain('Miles Davis');
 expect(html).toContain('aria-label="Close track actions"');
 expect(html.match(/class="track-action-item/g)).toHaveLength(3);
 expect(html.match(/Control permission is required\./g)).toHaveLength(1);
 expect(html).toContain('Unavailable actions');
 expect(html).toMatch(/disabled[^>]*>[\s\S]*?Play Next/);
 const source=readFileSync('src/lib/components/presentation/TrackRows.svelte','utf8');
 expect(source).toContain('use:controllerOverlay={{ close: closeActions }}');
 expect(source).toContain('margin: auto;');
 expect(source).toContain('@media (max-width: 600px)');
});

it('passive counts preserve zero, known counts, unknown/provider dash and accessible stale state',()=>{
 const tracks=[3,4,5,-1].map(id=>({id,title:'Track '+id,artist:'Artist',album:null,albumId:null,duration:20,quality:{badges:[]}}));
 const html=render(load(resolve(base,'TrackRows.svelte')).default,{props:{tracks,playCounts:new Map([[3,0],[4,12],[5,null],[-1,99]]),countsStale:true}}).body;
 const cells=[...html.matchAll(/<span class="col-plays"[^>]*>(.*?)<\/span>/g)].map(match=>match[1].replace(/<!--.*?-->/g,''));
 expect(cells).toEqual(['0','12','—','—']);expect(html).toContain('Last known play count');
});
it('album duration uses full authoritative total and removes only obsolete duration copy',()=>{
 const fixture=controllerBrowseFixture(true,{type:'album',albumId:7,playMode:'all'},{counts:new Map(),album:{albumId:7,totalDurationSeconds:202},loading:true,stale:false,error:''});
 expect(fixture.html).toContain('3:22');expect(fixture.html).not.toContain('Total duration not supplied');
 expect(fixture.html).not.toContain('Release metadata and total duration');expect(fixture.html).toContain('Release metadata');
 expect(fixture.html).toContain('Liked-track total');expect(fixture.html).toContain('Album like changes');expect(fixture.native).not.toHaveBeenCalled();
});

it('metadata joins only confirmed local page IDs and never refreshes artwork or catalog',()=>{
 const items=[{id:3,title:'Local',quality:{badges:[]}},{id:-1,title:'Provider',quality:{badges:[]}}];
 const context={type:'album',albumId:7,playMode:'all'};
 const metadata={counts:new Map([[3,0]]),album:{albumId:7,totalDurationSeconds:202},loading:false,stale:false,error:''};
 const confirmed=controllerBrowseFixture(true,context,metadata,{items,query:{type:'album_tracks',albumId:7,likedOnly:true}});
 expect(confirmed.metadata.setQuery).toHaveBeenCalledWith({trackIds:[3],albumId:7});
 expect(confirmed.html).toContain('3:22');expect(confirmed.page.refresh).not.toHaveBeenCalled();
 const stale=controllerBrowseFixture(true,context,metadata,{items,revision:3,query:{type:'album_tracks',albumId:7}});
 expect(stale.metadata.setQuery).toHaveBeenCalledWith({trackIds:[]});expect(stale.page.refresh).not.toHaveBeenCalled();
});
it('metadata error stays inline and completed values remove only count-gap copy',()=>{
 const tracks=[{id:3,title:'Local',quality:{badges:[]}}];
 const html=render(load(resolve(base,'TrackRows.svelte')).default,{props:{tracks,playCounts:new Map([[3,0]])}}).body;
 expect(html).not.toContain('Play counts: Not supplied');expect(html).toContain('Track sorting is managed');
 const fixture=controllerBrowseFixture(true,{type:'liked'},{counts:new Map(),album:null,loading:false,stale:false,error:'Could not read PC metadata.'});
 expect(fixture.html).toContain('Could not read PC metadata.');expect(fixture.html).not.toContain('PC action outcomes');
});

it.each([0,202,null])('full album duration %s remains authoritative without loaded tracks',seconds=>{
 const fixture=controllerBrowseFixture(true,{type:'album',albumId:7,playMode:'all'},{counts:new Map(),album:{albumId:7,totalDurationSeconds:seconds},loading:false,stale:seconds!==null,error:''},{query:{type:'album_tracks',albumId:7,likedOnly:true},items:[]});
 expect(fixture.html).toContain(seconds===null ? 'Total duration not supplied' : seconds===0 ? '0:00' : '3:22');
 if(seconds!==null)expect(fixture.html).toContain('Last known album duration');
 expect(fixture.html).not.toContain('PC action outcomes');fixture.dispose();expect(fixture.metadata.dispose).toHaveBeenCalledOnce();expect(fixture.page.dispose).toHaveBeenCalledOnce();
});

it('album toolbar uses matching34px visual controls and proportionally compact view buttons',()=>{
 const source=readFileSync(resolve(base,'AlbumBrowser.svelte'),'utf8');
 expect(source).toMatch(/\.view-controls\s*\{[^}]*height:\s*34px/);
 expect(source).toMatch(/\.view-button\s*\{[^}]*width:\s*28px;[^}]*height:\s*28px/);
 expect(source).toMatch(/\.sort-trigger\s*\{[^}]*height:\s*34px/);
});

it('client compiled Plays effects explicitly track replacement playCounts props',()=>{
 const source=readFileSync(resolve(base,'TrackRows.svelte'),'utf8');
 const client=compile(source,{filename:'TrackRows.svelte',generate:'client'}).js.code;
 expect(client).toMatch(/\(\) => \(\s*\$\.deep_read_state\(playCounts\(\)\),\s*\$\.get\(track\),\s*\$\.untrack\(\(\) => playCount\(playCounts\(\), \$\.get\(track\)\.id\) \?\? "—"\)/);
 expect(client).toMatch(/playCount\(playCounts\(\), \$\.get\(track\)\.id\)/);
 expect(source).toContain('tracks.some(track=>playCount(playCounts, track.id)');
});

it('compiler regression distinguishes original hidden Map dependency from explicit count argument',()=>{
 const source=readFileSync(resolve(base,'TrackRows.svelte'),'utf8');
 const original=source.replace('function playCount(counts:ReadonlyMap<number, number | null>, id:number)','function playCount(id:number)').replace('? counts.get(id) : null','? playCounts.get(id) : null').replaceAll('playCount(playCounts, track.id)','playCount(track.id)');
 const hidden=compile(original,{filename:'TrackRows.svelte',generate:'client'}).js.code;
 const fixed=compile(source,{filename:'TrackRows.svelte',generate:'client'}).js.code;
 const trackedText=/\(\) => \(\s*\$\.deep_read_state\(playCounts\(\)\),\s*\$\.get\(track\),\s*\$\.untrack\(\(\) => playCount\(playCounts\(\), \$\.get\(track\)\.id\) \?\? "—"\)/;
 expect(hidden).not.toMatch(trackedText);expect(fixed).toMatch(trackedText);
});

it('player title and artist expose passive keyboard-accessible navigation callbacks',()=>{
 const html=render(load(resolve(base,'Player.svelte')).default,{props:{title:'R U Mine?',artist:'Arctic Monkeys',onTitle:()=>{},onArtist:()=>{}}}).body;
 expect(html).toMatch(/<button[^>]*class="track-title[^>]*>R U Mine\?/);
 expect(html).toMatch(/<button[^>]*class="track-artist[^>]*>Arctic Monkeys/);
 const inert=render(load(resolve(base,'Player.svelte')).default,{props:{title:'Nothing playing'}}).body;
 expect(inert).not.toMatch(/<button[^>]*class="track-title/);
});

it('controller albums initially request recently added order and show its selected label',()=>{
 const fixture=controllerBrowseFixture();
 expect(fixture.page.setQuery).toHaveBeenCalledWith({type:'albums',sort:'added-desc',likedOnly:false});
 expect(fixture.html).toContain('Antigüedad (agregados recientemente)');
});
