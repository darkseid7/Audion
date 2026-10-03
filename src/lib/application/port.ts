import type { ApplicationPort } from "./types";

let installed: ApplicationPort | undefined;
let generation: symbol | undefined;

export function getApplicationPort(): ApplicationPort {
  if (!installed) throw new Error("Application not ready");
  return installed;
}

/** An offline controller cannot silently become a local desktop player. */
export function createUnavailablePort(): ApplicationPort {
  const reject = async (): Promise<never> => {
    throw new Error("Application unavailable");
  };
  return Object.freeze({
    query: reject,
    execute: reject,
    resolveArtwork: reject,
    subscribe: () => () => {},
  });
}

export function installApplicationPort(port: ApplicationPort): () => void {
  const ownership = Symbol("application-port");
  generation = ownership;
  installed = port;
  return () => {
    if (generation !== ownership) return;
    generation = undefined;
    installed = undefined;
  };
}
