<script lang="ts">
    import { onMount, onDestroy } from 'svelte';
    import { convertFileSrc } from '$lib/api/tauri';
    import { queue, queueIndex } from '$lib/stores/player';
    import type { Track } from '$lib/api/tauri';

    export let track: any;
    export let progress: number = 0;
    export let onSeek: (pos: number) => void = () => {};

    // High-resolution source data (fixed), downsampled at render time
    const SOURCE_RESOLUTION = 16000;
    // Desired bar width in CSS pixels (gap auto-calculated to fill width)
    const TARGET_BAR_W = 1;

    let canvas: HTMLCanvasElement;
    let rawData: { rms: Float32Array; peak: Float32Array } | null = null;
    let loading = false;
    let isDragging = false;
    let hoverPos: number | null = null;
    let rafId: number | null = null;
    let resizeObserver: ResizeObserver;
    let abortController: AbortController | null = null;
    let destroyed = false;

    const PREFETCH_CONCURRENCY = 2;
    const prefetchQueue: string[] = [];
    const prefetchQueued = new Set<string>();
    const prefetchInFlight = new Set<string>();

    // Session cache: path → raw high-res data
    const cache = new Map<string, { rms: Float32Array; peak: Float32Array }>();

    $: {
        const path = track?.path;
        if (path && canDecodeWaveformPath(path)) {
            loadWaveform(path);
        } else {
            rawData = null;
            loading = false;
            if (abortController) {
                abortController.abort();
                abortController = null;
            }
            scheduleRedraw();
        }
    }
    $: {
        const tracks = $queue;
        const idx = $queueIndex;
        enqueueQueuePrefetch(tracks, idx);
        pumpPrefetch();
    }
    $: { progress; hoverPos; scheduleRedraw(); }

    function canDecodeWaveformPath(path: string): boolean {
        if (!path) return false;
        const lower = path.toLowerCase();

        if (lower.startsWith('http://') || lower.startsWith('https://') || lower.startsWith('blob:')) {
            return false;
        }

        // Local filesystem paths and Tauri local schemes are supported.
        return !path.includes('://') || lower.startsWith('file://') || lower.startsWith('asset://') || lower.startsWith('tauri://');
    }

    function toWaveformUrl(path: string): string {
        return path.includes('://') ? path : convertFileSrc(path);
    }

    function enqueueQueuePrefetch(tracks: Track[], currentIdx: number): void {
        if (!tracks || tracks.length === 0) return;

        const safeIdx = Math.max(0, Math.min(currentIdx, tracks.length - 1));
        const ordered: string[] = [];

        // Prioritize upcoming tracks, then wrap to the beginning.
        for (let i = safeIdx; i < tracks.length; i++) {
            const path = tracks[i]?.path;
            if (path && canDecodeWaveformPath(path)) ordered.push(path);
        }
        for (let i = 0; i < safeIdx; i++) {
            const path = tracks[i]?.path;
            if (path && canDecodeWaveformPath(path)) ordered.push(path);
        }

        for (const path of ordered) {
            if (cache.has(path) || prefetchQueued.has(path) || prefetchInFlight.has(path)) continue;
            prefetchQueue.push(path);
            prefetchQueued.add(path);
        }
    }

    function pumpPrefetch(): void {
        if (destroyed) return;

        while (prefetchInFlight.size < PREFETCH_CONCURRENCY && prefetchQueue.length > 0) {
            const nextPath = prefetchQueue.shift();
            if (!nextPath) break;

            prefetchQueued.delete(nextPath);
            if (cache.has(nextPath) || prefetchInFlight.has(nextPath)) continue;

            prefetchInFlight.add(nextPath);
            void decodeAndCacheWaveform(nextPath)
                .catch(() => {})
                .finally(() => {
                    prefetchInFlight.delete(nextPath);
                    pumpPrefetch();
                });
        }
    }

    async function loadWaveform(path: string) {
        if (cache.has(path)) {
            rawData = cache.get(path)!;
            scheduleRedraw();
            return;
        }

        if (abortController) abortController.abort();
        abortController = new AbortController();
        const signal = abortController.signal;

        loading = true;
        rawData = null;
        scheduleRedraw();

        try {
            const decoded = await decodeAndCacheWaveform(path, signal);
            if (signal.aborted || !decoded) return;
            rawData = decoded;
        } catch (e: any) {
            if (e?.name !== 'AbortError') console.error('[WaveformSeekBar]', e);
        } finally {
            loading = false;
            scheduleRedraw();
        }
    }

    async function decodeAndCacheWaveform(
        path: string,
        signal?: AbortSignal
    ): Promise<{ rms: Float32Array; peak: Float32Array } | null> {
        if (cache.has(path)) return cache.get(path)!;

        const url = toWaveformUrl(path);
        const response = await fetch(url, signal ? { signal } : undefined);
        const arrayBuffer = await response.arrayBuffer();
        if (signal?.aborted) return null;

        const audioCtx = new AudioContext();
        try {
            const audioBuffer = await audioCtx.decodeAudioData(arrayBuffer);
            if (signal?.aborted) return null;

            const numChannels = audioBuffer.numberOfChannels;
            const length = audioBuffer.length;
            const channels: Float32Array[] = [];
            for (let c = 0; c < numChannels; c++) channels.push(audioBuffer.getChannelData(c));

            const blockSize = Math.max(1, Math.floor(length / SOURCE_RESOLUTION));
            const rms = new Float32Array(SOURCE_RESOLUTION);
            const peak = new Float32Array(SOURCE_RESOLUTION);

            for (let i = 0; i < SOURCE_RESOLUTION; i++) {
                let sumSq = 0;
                let peakAbs = 0;
                const start = i * blockSize;
                const end = Math.min(length, start + blockSize);

                if (start >= length || end <= start) {
                    rms[i] = 0;
                    peak[i] = 0;
                    continue;
                }

                for (let j = start; j < end; j++) {
                    let s = 0;
                    for (let c = 0; c < numChannels; c++) s += channels[c][j];
                    s /= numChannels;
                    sumSq += s * s;
                    const a = Math.abs(s);
                    if (a > peakAbs) peakAbs = a;
                }

                const samples = end - start;
                rms[i] = Math.sqrt(sumSq / samples);
                peak[i] = peakAbs;
            }

            // NO per-track normalization — keep absolute loudness values.
            // PCM is [-1,1] so peak is already in [0,1] absolute (1.0 = 0 dBFS).
            // RMS reflects true loudness: ~0.4-0.5 = brickwalled, ~0.1 = dynamic.
            // This lets the waveform height show actual track loudness (like Roon)
            // so you can visually spot loudness war victims vs dynamic masters.

            // Noise gate: silence below absolute threshold
            for (let i = 0; i < SOURCE_RESOLUTION; i++) {
                if (rms[i] < 0.002) rms[i] = 0;
                if (peak[i] < 0.002) peak[i] = 0;
            }

            const result = { rms, peak };
            cache.set(path, result);
            return result;
        } finally {
            audioCtx.close();
        }
    }

    /** Peak-preserving downsample: uses max value in each block to preserve transients */
    function downsamplePeak(src: Float32Array, bars: number): Float32Array {
        const out = new Float32Array(bars);
        const ratio = src.length / bars;
        for (let i = 0; i < bars; i++) {
            const from = Math.floor(i * ratio);
            const to = Math.max(from + 1, Math.floor((i + 1) * ratio));
            let mx = 0;
            for (let j = from; j < to && j < src.length; j++) {
                if (src[j] > mx) mx = src[j];
            }
            out[i] = mx;
        }
        return out;
    }

    /** RMS downsample: averages each block for smooth envelope */
    function downsampleAvg(src: Float32Array, bars: number): Float32Array {
        const out = new Float32Array(bars);
        const ratio = src.length / bars;
        for (let i = 0; i < bars; i++) {
            const from = Math.floor(i * ratio);
            const to = Math.max(from + 1, Math.floor((i + 1) * ratio));
            let sum = 0;
            for (let j = from; j < to && j < src.length; j++) sum += src[j];
            out[i] = sum / (to - from);
        }
        return out;
    }

    function scheduleRedraw() {
        if (!canvas) return;
        if (rafId !== null) cancelAnimationFrame(rafId);
        rafId = requestAnimationFrame(() => { rafId = null; renderCanvas(); });
    }

    function renderCanvas() {
        if (!canvas) return;
        const dpr = window.devicePixelRatio || 1;
        const w = canvas.offsetWidth;
        const h = canvas.offsetHeight;
        if (w === 0 || h === 0) return;

        canvas.width = w * dpr;
        canvas.height = h * dpr;
        const ctx = canvas.getContext('2d')!;
        ctx.scale(dpr, dpr);
        ctx.clearRect(0, 0, w, h);

        // Responsive: calculate bars to fill entire width edge-to-edge
        // Snap to device pixels so bars never overlap
        const dprInv = 1 / dpr;
        const barW = Math.max(dprInv, Math.floor(TARGET_BAR_W * dpr) * dprInv);
        const gapW = Math.max(dprInv, Math.floor(1 * dpr) * dprInv);
        const step = barW + gapW;
        const barCount = Math.max(20, Math.floor(w / step));

        const displayProg = hoverPos ?? progress;
        const cy = h / 2;
        const maxHalf = cy - 1;

        const accentColor = getComputedStyle(document.documentElement)
            .getPropertyValue('--accent-primary').trim() || '#1db954';

        if (!rawData) {
            const skH = Math.max(1, h * 0.08);
            for (let i = 0; i < barCount; i++) {
                const x = i * step;
                const barProg = (i + 0.5) / barCount;
                ctx.fillStyle = barProg < displayProg ? 'rgba(255,255,255,0.15)' : 'rgba(255,255,255,0.05)';
                ctx.fillRect(x, cy - skH, barW, skH * 2);
            }
        } else {
            // Blend RMS (smooth body) + Peak (transient detail) for Roon-like accuracy
            const rms = downsampleAvg(rawData.rms, barCount);
            const peaks = downsamplePeak(rawData.peak, barCount);

            for (let i = 0; i < barCount; i++) {
                const x = i * step;
                const barProg = (i + 0.5) / barCount;
                const played = barProg < displayProg;

                // Blend: 60% RMS (body) + 40% peak (transient detail)
                // No artificial scaling — absolute values reflect true loudness:
                // - Brickwalled (-6 LUFS): raw ~0.55 → bars ~60% height
                // - Well-mastered (-14 LUFS): raw ~0.25 → bars ~30% height
                // - Dynamic classical (-20 LUFS): raw ~0.10 → bars ~14% height
                const raw = Math.min(1.0, rms[i] * 0.6 + peaks[i] * 0.4);

                if (raw < 0.005) {
                    // Draw a minimum-height bar for silence instead of a gap
                    const minH = Math.max(1, maxHalf * 0.04);
                    ctx.fillStyle = played ? accentColor : 'rgba(255,255,255,0.10)';
                    ctx.fillRect(x, cy - minH, barW, minH * 2);
                    continue;
                }

                // Light compression — preserve dynamics, just soften extremes
                const val = Math.pow(raw, 0.85);
                const rH = Math.max(1, val * maxHalf);

                ctx.fillStyle = played ? accentColor : 'rgba(255,255,255,0.25)';
                ctx.fillRect(x, cy - rH, barW, rH * 2);
            }
        }
    }

    function posFromEvent(e: MouseEvent): number {
        if (!canvas) return 0;
        const rect = canvas.getBoundingClientRect();
        return Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
    }

    function handleMouseDown(e: MouseEvent) {
        isDragging = true;
        const pos = posFromEvent(e);
        hoverPos = pos;
        onSeek(pos);
    }

    function handleMouseMove(e: MouseEvent) {
        hoverPos = posFromEvent(e);
        if (isDragging) onSeek(hoverPos);
        scheduleRedraw();
    }

    function handleMouseLeave() {
        if (!isDragging) { hoverPos = null; scheduleRedraw(); }
    }

    function handleGlobalMouseMove(e: MouseEvent) {
        if (!isDragging) return;
        hoverPos = posFromEvent(e);
        onSeek(hoverPos);
        scheduleRedraw();
    }

    function handleGlobalMouseUp() {
        if (isDragging) {
            isDragging = false;
            hoverPos = null;
            scheduleRedraw();
        }
    }

    onMount(() => {
        resizeObserver = new ResizeObserver(() => scheduleRedraw());
        resizeObserver.observe(canvas);
        window.addEventListener('mousemove', handleGlobalMouseMove);
        window.addEventListener('mouseup', handleGlobalMouseUp);
    });

    onDestroy(() => {
        destroyed = true;
        if (rafId !== null) cancelAnimationFrame(rafId);
        if (abortController) abortController.abort();
        prefetchQueue.length = 0;
        prefetchQueued.clear();
        prefetchInFlight.clear();
        resizeObserver?.disconnect();
        window.removeEventListener('mousemove', handleGlobalMouseMove);
        window.removeEventListener('mouseup', handleGlobalMouseUp);
    });
</script>

<canvas
    bind:this={canvas}
    class="waveform-canvas"
    on:mousedown={handleMouseDown}
    on:mousemove={handleMouseMove}
    on:mouseleave={handleMouseLeave}
    role="slider"
    aria-label="Seek"
    aria-valuenow={Math.round(progress * 100)}
    aria-valuemin="0"
    aria-valuemax="100"
    tabindex="0"
></canvas>

<style>
    .waveform-canvas {
        display: block;
        width: 100%;
        height: 100%;
        cursor: pointer;
    }
</style>
