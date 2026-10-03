import { describe, expect, it } from "vitest";
import { connectHostBridge, type HostDispatch, type HostTransport } from "./bridge";
import type { ApplicationPort, ExecutionResult } from "../types";
const portWith = (execute: ApplicationPort["execute"]): ApplicationPort => ({
  execute,
  async query() { throw new Error("Unexpected query"); },
  subscribe() { return () => {}; },
  async resolveArtwork() { throw new Error("Unexpected artwork resolution"); },
});

describe("native host bridge", () => {
  it("listens before preparation and attaches authority before ready, then completes exact tickets through the port", async () => {
    const steps: string[] = [];
    let receive!: (dispatch: HostDispatch) => void;
    const completed: unknown[] = [];
    const lease = { leaseId: "lease", hostEpoch: "epoch" };
    const transport: HostTransport = {
      async listen(listener) { steps.push("listen"); receive = listener; return () => { steps.push("unlisten"); }; },
      async register(request) { steps.push(request.phase); return { hostId: "host", lease }; },
      async authorize() { return envelope; },
      async complete(actualLease, ticket, result) { completed.push({ lease: actualLease, ticket, result }); },
    };
    const envelope = { protocolVersion: 1 as const, requestId: "request", preconditions: { hostEpoch: "epoch", outputRevision: 7 }, intent: { type: "pause" as const } };
    const port = portWith(async (intent, preconditions) => {
      expect(intent).toEqual(envelope.intent); expect(preconditions).toEqual(envelope.preconditions);
      steps.push("execute"); return { status: "applied", revision: 8 } as ExecutionResult;
    });
    const bridge = await connectHostBridge(port, async authority => {
      expect(authority).toEqual({ hostId: "host", hostEpoch: "epoch" }); steps.push("attach");
    }, transport);
    expect(steps).toEqual(["listen", "prepare", "attach", "ready"]);
    receive({ ticket: "native-ticket", envelope });
    await new Promise(resolve => setTimeout(resolve, 0));
    expect(completed).toEqual([{ lease, ticket: "native-ticket", result: { status: "applied", revision: 8 } }]);
    await bridge.dispose();
    expect(steps.slice(-2)).toEqual(["release", "unlisten"]);
    receive({ ticket: "late", envelope });
    expect(steps.filter(step => step === "execute")).toHaveLength(1);
  });
  it("releases a prepared lease when attachment fails without advertising ready", async () => {
    const steps: string[] = [];
    const transport: HostTransport = {
      async listen() { return () => { steps.push("unlisten"); }; },
      async register(request) { steps.push(request.phase); return { hostId: "host", lease: { leaseId: "lease", hostEpoch: "epoch" } }; },
      async authorize() { throw new Error("unexpected authorization"); },
      async complete() { throw new Error("unexpected completion"); },
    };
    await expect(connectHostBridge(portWith(async () => { throw new Error("unexpected execution"); }), async () => { throw new Error("attachment failed"); }, transport)).rejects.toThrow("attachment failed");
    expect(steps).toEqual(["prepare", "release", "unlisten"]);
  });
  it("rejects forged and duplicate events and executes only the native claimed envelope", async () => {
    let receive!: (dispatch: HostDispatch) => void;
    const intents: unknown[] = [];
    const original = { protocolVersion: 1 as const, requestId: "original", preconditions: { hostEpoch: "epoch" }, intent: { type: "pause" as const } };
    const tickets = new Set(["native"]);
    const bridge = await connectHostBridge(portWith(async intent => { intents.push(intent); return { status: "applied", revision: 1 }; }), async () => {}, {
      async listen(listener) { receive = listener; return () => {}; },
      async register() { return { hostId: "host", lease: { leaseId: "lease", hostEpoch: "epoch" } }; },
      async authorize(_lease, ticket) { if (!tickets.delete(ticket)) throw new Error("Unauthorized ticket"); return original; },
      async complete() {},
    });
    receive({ ticket: "forged", envelope: { ...original, intent: { type: "resume" } } });
    await new Promise(resolve => setTimeout(resolve, 0));
    expect(intents).toEqual([]);
    receive({ ticket: "native", envelope: { ...original, intent: { type: "resume" } } });
    receive({ ticket: "native", envelope: original });
    await new Promise(resolve => setTimeout(resolve, 0));
    expect(intents).toEqual([{ type: "pause" }]);
    await bridge.dispose();
  });
});
