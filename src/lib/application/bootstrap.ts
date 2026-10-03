import { writable, readonly, derived } from "svelte/store";
import { installApplicationPort } from "./port";
import type { ApplicationMode, ApplicationPort } from "./types";
export interface ApplicationHandle {
    port: ApplicationPort;
    dispose(): Promise<void>;
}
export type BootstrapLoaders = Record<ApplicationMode, () => Promise<ApplicationHandle>>;
const modeState = writable<ApplicationMode | null>(null);
export const applicationMode = readonly(modeState);
const migrationState = writable<string | null>(null);
export const migrationStatus = readonly(migrationState);
/** @internal Desktop startup migration progress, never a domain initializer. */
export const setMigrationStatus = migrationState.set;
export const desktopEffectsEnabled = derived(modeState, mode => mode === "desktop");
export const defaultBootstrapLoaders: BootstrapLoaders = {
    desktop: async () => (await import("./desktop/bootstrap")).bootstrapDesktop(),
    controller: async () => (await import("./controller/bootstrap")).bootstrapController(),
};
let currentOwner: symbol | undefined;
let nativeRole: ApplicationMode | undefined;
/** The caller obtains mode from native setup, never a persisted preference. */
export async function bootstrapApplication(mode: ApplicationMode, loaders: BootstrapLoaders): Promise<ApplicationHandle> {
    if (nativeRole && nativeRole !== mode)
        throw new Error("Application role is immutable");
    nativeRole = mode;
    const owner = Symbol("application");
    currentOwner = owner;
    modeState.set(mode);
    let loaded: ApplicationHandle;
    try {
        loaded = await loaders[mode]();
    }
    catch (error) {
        if (currentOwner === owner) {
            currentOwner = undefined;
            modeState.set(null);
        }
        throw error;
    }
    if (currentOwner !== owner) {
        await loaded.dispose();
        throw new Error("Application bootstrap superseded");
    }
    const uninstall = installApplicationPort(loaded.port);
    let disposed = false;
    return {
        port: loaded.port,
        async dispose() {
            if (disposed)
                return;
            disposed = true;
            uninstall();
            if (currentOwner === owner) {
                currentOwner = undefined;
                modeState.set(null);
            }
            await loaded.dispose();
        },
    };
}
