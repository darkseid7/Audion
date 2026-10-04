import { get } from "svelte/store";
import { controllerState } from "./controller/bootstrap";
import type { ControllerState } from "./controller/session";
import type { ApplicationIntent, OutputRef, SnapshotOutputRef } from "./types";
export function sameOutput(a: SnapshotOutputRef, b: SnapshotOutputRef): boolean {
 return a.kind === b.kind && (a.kind !== "squeeze" || (b.kind === "squeeze" && a.playerId === b.playerId));
}
/** Permission and output support are independent, confirmed host facts. */
export function canExecute(capability: string): boolean;
export function canExecute(state: ControllerState, capability: string): boolean;
export function canExecute(stateOrCapability: ControllerState | string, requested?: string): boolean {
 const state=typeof stateOrCapability === "string" ? get(controllerState) : stateOrCapability;
 const capability=typeof stateOrCapability === "string" ? stateOrCapability : requested!;
 const s = state.snapshot;
 if (!state.ready || !state.grants?.control || !s || !s.capabilities.intents.includes(capability as ApplicationIntent["type"])) return false;
 if (capability === "select_output") return true;
 if (s.output.kind === "desktop_only") return false;
 const active = s.outputs.find(o => sameOutput(o.output, s.output));
 if (!active?.available) return false;
 const key = capability === "seek" ? "seek" : capability === "set_volume" ? "volume" : capability === "set_shuffle" ? "shuffle" : capability === "set_repeat" ? "repeat" : "playback";
 return active.capabilities[key];
}
export function outputAvailable(state: ControllerState, output: OutputRef): boolean {
 return canExecute(state, "select_output") && !!state.snapshot?.outputs.some(o => o.available && sameOutput(o.output, output));
}
