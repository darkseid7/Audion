/** Presentation-only overlay stack; no application commands or native authority. */
const overlays: { close: () => void }[] = [];
let navigateBack: (() => boolean) | undefined;
export function registerControllerBack(handler: () => boolean): () => void {
    navigateBack = handler;
    return () => { if (navigateBack === handler) navigateBack = undefined; };
}
export function dismissControllerOverlay(): boolean {
    const top = overlays.at(-1);
    if (!top) return false;
    top.close();
    return true;
}
export function handleControllerBack(): boolean {
    return dismissControllerOverlay() || navigateBack?.() || false;
}
export function controllerOverlay(node: HTMLElement, options: { close: () => void; enabled?: boolean }) {
    if (options.enabled === false) return { destroy() {} };
    const origin = document.activeElement as HTMLElement | null;
    const entry = { close: options.close };
    overlays.push(entry);
    let active = true;
    const isTop = () => active && overlays.at(-1) === entry;
    const controls = () => Array.from(node.querySelectorAll<HTMLElement>(
        'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'
    )).filter(element => element.getClientRects().length > 0);
    const focusFirst = () => (controls()[0] ?? node).focus();
    queueMicrotask(() => { if (isTop()) focusFirst(); });
    const focusIn = (event: FocusEvent) => {
        if (isTop() && !node.contains(event.target as Node)) focusFirst();
    };
    const keydown = (event: KeyboardEvent) => {
        if (!isTop()) return;
        if (event.key === 'Escape') {
            event.preventDefault(); event.stopPropagation(); entry.close();
        } else if (event.key === 'Tab') {
            const items = controls(), first = items[0], last = items.at(-1);
            if (!first) { event.preventDefault(); node.focus(); }
            else if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last!.focus(); }
            else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
        }
    };
    node.addEventListener('keydown', keydown);
    document.addEventListener('focusin', focusIn);
    return { destroy() {
        const wasTop = isTop(); active = false;
        const index = overlays.indexOf(entry); if (index >= 0) overlays.splice(index, 1);
        node.removeEventListener('keydown', keydown);
        document.removeEventListener('focusin', focusIn);
        if (wasTop && origin?.isConnected) origin.focus();
    } };
}
