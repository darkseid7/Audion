import { createUnavailablePort } from "../port";
import type { ApplicationHandle } from "../bootstrap";
/** No desktop dependencies: the native transport arrives in Task 9. */
export async function bootstrapController(): Promise<ApplicationHandle> {
    return { port: createUnavailablePort(), async dispose() { } };
}
