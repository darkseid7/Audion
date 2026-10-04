import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { get } from "svelte/store";
vi.mock("$lib/services/native-audio", async original => ({ ...await original<object>(), shouldUseNativeAudio: async () => true,
  nativeAudioStop: vi.fn().mockResolvedValue(undefined), nativeAudioSetVolume: vi.fn().mockResolvedValue(undefined),
  nativeAudioSetRepeatOne: vi.fn().mockResolvedValue(undefined), nativeAudioSetEq: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("$lib/api/tauri", async original => ({ ...await original<object>(), updateWindowsThumbarState: vi.fn().mockResolvedValue(undefined), initWindowsThumbar: vi.fn().mockResolvedValue(false) }));
import * as runtime from "./player-runtime";
import * as facade from "$lib/stores/player";
import { createDesktopAdapter } from "./adapter";
import { equalizer } from "$lib/stores/equalizer";
class Media extends EventTarget {
  static all: Media[] = [];
  src=""; volume=1; currentTime=0; duration=100; paused=true; ended=false; error={message:"retired"};
  constructor() { super(); Media.all.push(this); }
  play() { this.paused=false; return Promise.resolve(); }
  pause() { this.paused=true; }
  load() {}
}
const node = () => ({ connect() {}, disconnect() {}, frequency:{value:0}, Q:{value:0}, gain:{value:0,cancelScheduledValues(){},setTargetAtTime(){}} });
class Graph {
  state="running"; currentTime=0; destination={};
  createMediaElementSource() { return node(); } createGain() { return node(); } createBiquadFilter() { return node(); }
  close() { return Promise.resolve(); }
}
const track = (id:number,path:string) => ({id,title:`Stream ${id}`,path,source_type:"plugin",cover_url:"fixture",duration:100} as any);
beforeEach(async () => {
  vi.stubGlobal("localStorage", {getItem:()=>null,setItem() {}});
  vi.stubGlobal("navigator", {}); vi.stubGlobal("Audio", Media);
  vi.stubGlobal("window", { location:{origin:"https://fixture.invalid"}, AudioContext:Graph, matchMedia:()=>({matches:false,addEventListener(){},removeEventListener(){}}) });
  Media.all=[]; runtime.queue.set([]);runtime.currentTrack.set(null);runtime.shuffle.set(false);runtime.repeat.set("none");runtime.activeBackend.set("none");
  await runtime.initAudioBackend();
});
afterEach(() => { runtime.cleanupPlayer(); equalizer.setEnabled(false); vi.unstubAllGlobals(); vi.restoreAllMocks(); });
it("held events from replaced and disposed HTML5 media cannot affect the current occurrence", async () => {
  const adapter=createDesktopAdapter();
  const logged=vi.spyOn(console,"error").mockImplementation(()=>{});
  try {
    equalizer.setEnabled(true);
    await facade.playTracks([track(1,"https://fixture.invalid/first")]);
    const old=Media.all.at(-1)!;
    await facade.playTracks([track(2,"https://other.invalid/second"),track(3,"https://other.invalid/third")]);
    const current=Media.all.at(-1)!;
    expect(current).not.toBe(old);
    old.currentTime=91;old.duration=999;
    for(const event of ["timeupdate","durationchange","pause","play","error"]) {
      old.dispatchEvent(new Event(event));
      expect(get(runtime.currentTime)).toBe(0); expect(get(runtime.duration)).toBe(100); expect(get(runtime.isPlaying)).toBe(true);
    }
    old.dispatchEvent(new Event("ended"));
    await adapter.coordinator.executeLocal(async()=>({status:"applied"}));
    expect(get(runtime.currentTime)).toBe(0);expect(get(runtime.duration)).toBe(100);
    expect(get(runtime.isPlaying)).toBe(true);expect(get(runtime.currentTrack)?.id).toBe(2);
    expect(logged).not.toHaveBeenCalled();
    current.dispatchEvent(new Event("ended"));
    await adapter.coordinator.executeLocal(async()=>({status:"applied"}));
    expect(get(runtime.currentTrack)?.id).toBe(3);
    const disposed=Media.all.at(-1)!;runtime.cleanupPlayer();
    await facade.playTracks([track(4,"https://other.invalid/fourth")]);
    for(const event of ["timeupdate","durationchange","pause","error","ended"]) disposed.dispatchEvent(new Event(event));
    await adapter.coordinator.executeLocal(async()=>({status:"applied"}));
    expect(get(runtime.currentTrack)?.id).toBe(4);expect(get(runtime.isPlaying)).toBe(true);
  } finally { await adapter.dispose(); }
});
