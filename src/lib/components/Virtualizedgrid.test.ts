import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';

type GeometrySnapshot = {
    columns: number;
    scrollTop: number;
    currentScrollTop: number;
    totalHeight: number;
    visibleRows: { rowIndex: number; items: { id: number }[] }[];
};

// Run the component's actual reactive calculations, as the existing Node test
// environment has no DOM. No duplicate geometry implementation is used here.
function loadGeometry(initiallyEmpty = false) {
    const component = readFileSync(new URL('./Virtualizedgrid.svelte', import.meta.url), 'utf8');
    const script = component.match(/<script[^>]*>([\s\S]*?)<\/script>/)?.[1];
    if (!script) throw new Error('Virtualizedgrid script not found');
    const source = ts.createSourceFile('Virtualizedgrid.ts', script, ts.ScriptTarget.Latest, true);
    const statements = source.statements.filter((node) => !ts.isImportDeclaration(node));
    const names = statements.flatMap((node) => ts.isVariableStatement(node)
        ? node.declarationList.declarations.flatMap((declaration) => ts.isIdentifier(declaration.name)
            ? [declaration.name.text] : []) : []);
    const reactive = statements.filter((node) => ts.isLabeledStatement(node) && node.label.text === '$');
    const program = statements.map((node) => node.getText(source).replace(/^export\s+/, '')).join('\n');
    const runReactive = reactive.map((node) => node.getText(source)).join('\n');
    const setters = names.map((name) => `case ${JSON.stringify(name)}: ${name} = value; break;`).join('\n');
    const { outputText } = ts.transpileModule(`${program}
        ({
            update(values) {
                for (const [key, value] of Object.entries(values)) {
                    switch (key) { ${setters} }
                }
                ${runReactive}
                flushUpdates();
                ${runReactive}
                flushUpdates();
                return { columns, scrollTop, currentScrollTop, ...virtualScrollState };
            },
            scrollTo(top) {
                containerElement.scrollTop = top;
                handleScroll({ target: containerElement });
                ${runReactive}
                flushUpdates();
                ${runReactive}
                flushUpdates();
                return { columns, scrollTop, currentScrollTop, ...virtualScrollState };
            },
            mount() {
                flushMounts();
                ${runReactive}
                flushUpdates();
                ${runReactive}
                flushUpdates();
                return { columns, scrollTop, currentScrollTop, ...virtualScrollState };
            },
            resize(element) {
                emitResize(element);
                ${runReactive}
                flushUpdates();
                ${runReactive}
                flushUpdates();
                return { columns, scrollTop, currentScrollTop, ...virtualScrollState };
            },
            queuedResize() {
                emitQueuedResize();
                ${runReactive}
                flushUpdates();
                ${runReactive}
                flushUpdates();
                return { columns, scrollTop, currentScrollTop, ...virtualScrollState };
            }
        });`, { compilerOptions: { target: ts.ScriptTarget.ES2022 } });
    const afterUpdates: (() => void)[] = [];
    const mounts: (() => void)[] = [];
    const observers: { callback: () => void; element?: unknown }[] = [];
    const geometry = runInNewContext(outputText, {
        onMount(callback: () => void) { mounts.push(callback); }, onDestroy() {}, console,
        afterUpdate(callback: () => void) { afterUpdates.push(callback); },
        flushUpdates() { afterUpdates.forEach((callback) => callback()); },
        flushMounts() { mounts.forEach((callback) => callback()); },
        getComputedStyle(element: { paddingTop: string; paddingBottom: string }) {
            return { paddingTop: element.paddingTop, paddingBottom: element.paddingBottom };
        },
        ResizeObserver: class {
            element?: unknown;
            constructor(public callback: () => void) { observers.push(this); }
            observe(element: unknown) { this.element = element; }
            disconnect() { this.element = undefined; }
        },
        emitResize(element: unknown) {
            observers.filter((observer) => observer.element === element).forEach((observer) => observer.callback());
        },
        emitQueuedResize() { observers.forEach((observer) => observer.callback()); },
    }) as {
        update(values: Record<string, unknown>): GeometrySnapshot;
        scrollTo(top: number): GeometrySnapshot;
        mount(): GeometrySnapshot;
        resize(element: unknown): GeometrySnapshot;
        queuedResize(): GeometrySnapshot;
    };
    const element = { scrollTop: 0, clientWidth: 1280, clientHeight: 600, paddingTop: '0px', paddingBottom: '0px' };
    geometry.update({
        items: initiallyEmpty ? [] : Array.from({ length: 300 }, (_, id) => ({ id })),
        containerElement: initiallyEmpty ? null : element,
        containerWidth: 1280,
        containerHeight: 600,
    });
    return { geometry, element };
}

describe('Virtualizedgrid layout geometry', () => {
    it('keeps the existing multi-column grid when layout is omitted', () => {
        const { geometry } = loadGeometry();
        const view = geometry.update({ containerWidth: 800 });
        expect(view.columns).toBe(3);
        expect(view.totalHeight).toBe(28400);
        expect(view.visibleRows[0].items.map((item) => item.id)).toEqual([0, 1, 2]);
    });

    it('renders one full-width item per row in list layout at desktop widths', () => {
        const { geometry } = loadGeometry();
        const view = geometry.update({ layout: 'list', cardHeightDesktop: 104, gridGapDesktop: 8 });
        expect(view.columns).toBe(1);
        expect(view.totalHeight).toBe(33600);
        expect(view.visibleRows.map((row) => row.items.map((item) => item.id)))
            .toEqual([[0], [1], [2], [3], [4], [5], [6], [7]]);
    });

    it('anchors a deep one-column viewport when switching to a shorter grid', () => {
        const { geometry, element } = loadGeometry();
        // This also reproduces the old component's real one-column geometry,
        // before it understands the new layout prop.
        geometry.update({ layout: 'list', cardWidthDesktop: 1280, cardHeightDesktop: 104, gridGapDesktop: 8 });
        geometry.scrollTo(30296); // Item 270, halfway through its 112px row.
        const view = geometry.update({ layout: 'grid', cardWidthDesktop: 180, cardHeightDesktop: 380, gridGapDesktop: 24 });
        expect(view.visibleRows.length).toBeGreaterThan(0);
        expect(view.scrollTop).toBe(18382); // Row 45 * 404px + half a row.
        expect(view.currentScrollTop).toBe(18382);
        expect(element.scrollTop).toBe(18382);
        expect(view.visibleRows.flatMap((row) => row.items).some((item) => item.id === 270)).toBe(true);
    });

    it('clamps the anchored viewport to the new bottom instead of leaving blank space', () => {
        const { geometry, element } = loadGeometry();
        geometry.update({ layout: 'list', cardWidthDesktop: 1280, cardHeightDesktop: 104, gridGapDesktop: 8 });
        geometry.scrollTo(33000);
        const view = geometry.update({ layout: 'grid', cardWidthDesktop: 180, cardHeightDesktop: 380, gridGapDesktop: 24 });
        expect(view.scrollTop).toBe(19600);
        expect(view.currentScrollTop).toBe(19600);
        expect(element.scrollTop).toBe(19600);
        expect(view.visibleRows.at(-1)?.items.at(-1)?.id).toBe(299);
    });

    it('keeps the same approximate first visible item across repeated grid/list toggles', () => {
        const { geometry } = loadGeometry();
        geometry.update({ cardHeightDesktop: 380, gridGapDesktop: 24 });
        geometry.scrollTo(12144); // Item 180 plus 24px into its grid row.
        const list = geometry.update({ layout: 'list', cardHeightDesktop: 104, gridGapDesktop: 8 });
        expect(list.scrollTop).toBeCloseTo(20166.653465346535);
        expect(list.visibleRows.flatMap((row) => row.items).some((item) => item.id === 180)).toBe(true);
        const grid = geometry.update({ layout: 'grid', cardHeightDesktop: 380, gridGapDesktop: 24 });
        expect(grid.scrollTop).toBeCloseTo(12144);
    });

    it('clamps both remembered and actual scroll when fewer items fit within one viewport', () => {
        const { geometry, element } = loadGeometry();
        geometry.scrollTo(10000);
        const view = geometry.update({ items: [{ id: 0 }, { id: 1 }] });
        expect(view.scrollTop).toBe(0);
        expect(view.currentScrollTop).toBe(0);
        expect(element.scrollTop).toBe(0);
        expect(view.visibleRows[0]?.items.map((item) => item.id)).toEqual([0, 1]);
    });

    it.each([
        { top: '16px', bottom: '16px', maximum: 12627 },
        { top: '32px', bottom: '8px', maximum: 12635 },
    ])('allows the last card to be reached with $top/$bottom container padding', ({ top, bottom, maximum }) => {
        const { geometry, element } = loadGeometry();
        geometry.update({
            items: Array.from({ length: 120 }, (_, id) => ({ id })),
            layout: 'list', cardHeightDesktop: 104, gridGapDesktop: 8,
        });
        Object.assign(element, { clientHeight: 845, paddingTop: top, paddingBottom: bottom });
        geometry.mount(); // Exercise the production computed-style measurement.
        const view = geometry.scrollTo(maximum);
        expect(view.totalHeight).toBe(13440);
        expect(view.scrollTop).toBe(maximum);
        expect(view.currentScrollTop).toBe(maximum);
        expect(element.scrollTop).toBe(maximum);
        expect(view.visibleRows.at(-1)?.items.at(-1)?.id).toBe(119);
    });

    it('ignores a queued resize callback after the empty state removes the container', () => {
        const { geometry } = loadGeometry();
        geometry.mount();
        geometry.update({ items: [], containerElement: null });
        expect(() => geometry.queuedResize()).not.toThrow();
    });

    it('measures and observes the new container when items return from the empty state', () => {
        const { geometry } = loadGeometry();
        geometry.mount();
        geometry.update({ items: [], containerElement: null });
        const replacement = { scrollTop: 0, clientWidth: 500, clientHeight: 420, paddingTop: '0px', paddingBottom: '0px' };
        const restored = geometry.update({
            items: Array.from({ length: 90 }, (_, id) => ({ id })), containerElement: replacement,
        });
        expect(restored.columns).toBe(3);
        expect(restored.totalHeight).toBe(6540);
        replacement.clientWidth = 340;
        const resized = geometry.resize(replacement);
        expect(resized.columns).toBe(2);
        expect(resized.totalHeight).toBe(9810);
        expect(resized.visibleRows[0].items.map((item) => item.id)).toEqual([0, 1]);
    });

    it('attaches responsive measurement when an initially empty component receives items', () => {
        const { geometry, element } = loadGeometry(true);
        geometry.mount();
        Object.assign(element, { clientWidth: 340, clientHeight: 420 });
        const arrived = geometry.update({
            items: Array.from({ length: 90 }, (_, id) => ({ id })), containerElement: element,
        });
        expect(arrived.columns).toBe(2);
        expect(arrived.totalHeight).toBe(9810);
        element.clientWidth = 500;
        const resized = geometry.resize(element);
        expect(resized.columns).toBe(3);
        expect(resized.totalHeight).toBe(6540);
    });
});
