import { writable, derived } from 'svelte/store';
import { isMiniPlayer } from '$lib/stores/ui';

/** Viewport layout is presentation only; platform hints never grant native authority. */
const MOBILE_BREAKPOINT = 768;
export function selectLayout(width: number): "compact" | "expanded" {
    return width < MOBILE_BREAKPOINT ? "compact" : "expanded";
}
export const isMobileViewport = writable(false);
export const isMobileSidebarOpen = writable(false);
export const isMobilePlatform = writable(false);
// Native mini-player resizing must not turn the desktop into a phone layout.
export const isMobile = derived(
    [isMobileViewport, isMiniPlayer],
    ([$viewport, $pip]) => !$pip && $viewport
);
let stopDetection: (() => void) | undefined;
export function initMobileDetection(): () => void {
    stopDetection?.();
    if (typeof window === 'undefined') return () => {};
    const mediaQuery = window.matchMedia(`(width < ${MOBILE_BREAKPOINT}px)`);
    const update = () => {
        isMobileViewport.set(mediaQuery.matches);
        if (!mediaQuery.matches) isMobileSidebarOpen.set(false);
    };
    const handler = (event: MediaQueryListEvent) => {
        isMobileViewport.set(event.matches);
        if (!event.matches) isMobileSidebarOpen.set(false);
    };
    update();
    mediaQuery.addEventListener('change', handler);
    let active = true;
    const dispose = () => {
        if (!active) return;
        active = false;
        mediaQuery.removeEventListener('change', handler);
        if (stopDetection === dispose) stopDetection = undefined;
    };
    stopDetection = dispose;
    void detectMobilePlatform();
    return dispose;
}

async function detectMobilePlatform() {
    try {
        // Check if we're on Android/iOS via Tauri
        const { type } = await import('@tauri-apps/plugin-os');
        const osType = type();
        if (osType === 'android' || osType === 'ios') {
            isMobilePlatform.set(true);
        }
    } catch {
        // plugin-os not available, fall back to user agent
        if (typeof navigator !== 'undefined') {
            const ua = navigator.userAgent.toLowerCase();
            const isMobileUA = /android|iphone|ipad|ipod|mobile/i.test(ua);
            isMobilePlatform.set(isMobileUA);
        }
    }
}

export function toggleMobileSidebar() {
    isMobileSidebarOpen.update(v => !v);
}

export function closeMobileSidebar() {
    isMobileSidebarOpen.set(false);
}

export function openMobileSidebar() {
    isMobileSidebarOpen.set(true);
}

// Mobile search state (for bottom nav Search tab)
export const mobileSearchOpen = writable(false);
