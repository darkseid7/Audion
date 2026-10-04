import { afterEach, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import * as mobile from './mobile';
import { isMiniPlayer } from './ui';
afterEach(() => { mobile.isMobileViewport.set(false); mobile.isMobilePlatform.set(false); isMiniPlayer.set(false); vi.unstubAllGlobals(); });
it('selects compact below 768 and expanded at 768, independently of Android', () => {
 expect(mobile.selectLayout(767)).toBe('compact');
 expect(mobile.selectLayout(768)).toBe('expanded');
 mobile.isMobileViewport.set(false); mobile.isMobilePlatform.set(true);
 expect(get(mobile.isMobile)).toBe(false);
});
it('preserves mini-player at compact window dimensions', () => {
 mobile.isMobileViewport.set(true); isMiniPlayer.set(true); expect(get(mobile.isMobile)).toBe(false);
 isMiniPlayer.set(false); expect(get(mobile.isMobile)).toBe(true);
});
it('tracks rotations with one owned listener and disposes without browser globals in SSR', () => {
 const listeners=new Set<Function>(); let query='';
 vi.stubGlobal('window',{ matchMedia:(q:string)=>{query=q; return { matches:true, addEventListener:(_:string,fn:Function)=>listeners.add(fn),removeEventListener:(_:string,fn:Function)=>listeners.delete(fn) };} });
 const first=mobile.initMobileDetection(); const dispose=mobile.initMobileDetection();
 expect(query).toBe('(width < 768px)'); expect(listeners.size).toBe(1);
 mobile.isMobileSidebarOpen.set(true); for(const change of listeners)change({matches:false});
 expect(get(mobile.isMobile)).toBe(false); expect(get(mobile.isMobileSidebarOpen)).toBe(false);
 first(); expect(listeners.size).toBe(1); dispose(); expect(listeners.size).toBe(0);
 vi.unstubAllGlobals(); expect(()=>mobile.initMobileDetection()).not.toThrow();
});
it('keeps live layout consistent with fractional viewport widths', () => {
 for(const width of [767,767.5,767.999,768,768.5]) {
  vi.stubGlobal('window',{matchMedia:(query:string)=>{
   const max=query.match(/^\(max-width: (\d+)px\)$/), lt=query.match(/^\(width < (\d+)px\)$/);
   if(!max&&!lt)throw new Error(`Unexpected media condition: ${query}`);
   const matches=max?width<=Number(max[1]):width<Number(lt![1]);
   return {matches,addEventListener(){},removeEventListener(){}};
  }});
  const dispose=mobile.initMobileDetection();
  expect(get(mobile.isMobileViewport)).toBe(mobile.selectLayout(width)==='compact');
  dispose();
 }
});
