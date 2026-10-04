import type { ControlError } from "../types";
const cancelled = (): ControlError => ({ code: "resync_required", message: "Controller read is no longer current.", retryable: false });
const delays = [200, 500, 1000];
/** Cancel the consumer promptly; a native call still owns admission until settled. */
function cancellable<T>(signal: AbortSignal, start: () => Promise<T>, discard?: (value: T) => void): Promise<T> {
    if (signal.aborted)
        return Promise.reject(cancelled());
    return new Promise((resolve, reject) => {
        const abort = () => reject(cancelled());
        signal.addEventListener("abort", abort, { once: true });
        let pending: Promise<T>;
        try {
            pending = start();
        }
        catch (error) {
            pending = Promise.reject(error);
        }
        pending.then(value => {
            if (signal.aborted)
                discard?.(value);
            resolve(value);
        }, reject).finally(() => signal.removeEventListener("abort", abort));
    });
}
function backoff(signal: AbortSignal, delay: number): Promise<void> {
    if (signal.aborted)
        return Promise.reject(cancelled());
    return new Promise((resolve, reject) => {
        const abort = () => { clearTimeout(timer); reject(cancelled()); };
        const timer = setTimeout(() => { signal.removeEventListener("abort", abort); resolve(); }, delay);
        signal.addEventListener("abort", abort, { once: true });
    });
}
/** Safe reads only. Commands and mutations must never use this recovery policy. */
export async function recoverRead<T>(signal: AbortSignal, attempt: () => Promise<T>, discard?: (value: T) => void): Promise<T> {
    for (let index = 0;; index++) {
        if (signal.aborted)
            throw cancelled();
        try {
            return await cancellable(signal, attempt, discard);
        }
        catch (error) {
            if (signal.aborted)
                throw cancelled();
            const e = error as Partial<ControlError> | null;
            if (e?.code !== "busy" || e.retryable !== true || index === delays.length)
                throw error;
            await backoff(signal, delays[index]);
        }
    }
}
/** Session-local FIFO admission shared by query and media, not by long polling. */
export function createReadAdmission() {
    let active = 0;
    const queue: {
        enter(): void;
    }[] = [];
    function drain() {
        while (active < 2 && queue.length)
            queue.shift()!.enter();
    }
    function acquire(signal: AbortSignal): Promise<() => void> {
        if (signal.aborted)
            return Promise.reject(cancelled());
        return new Promise((resolve, reject) => {
            const abort = () => {
                const index = queue.indexOf(waiter);
                if (index !== -1)
                    queue.splice(index, 1);
                signal.removeEventListener("abort", abort);
                reject(cancelled());
            };
            const waiter = { enter() {
                    signal.removeEventListener("abort", abort);
                    if (signal.aborted) {
                        reject(cancelled());
                        return;
                    }
                    active++;
                    resolve(() => { active--; drain(); });
                } };
            signal.addEventListener("abort", abort, { once: true });
            queue.push(waiter);
            drain();
        });
    }
    return async <T>(signal: AbortSignal, attempt: () => Promise<T>): Promise<T> => {
        const release = await acquire(signal);
        try {
            if (signal.aborted)
                throw cancelled();
            return await attempt();
        }
        finally {
            release();
        }
    };
}
