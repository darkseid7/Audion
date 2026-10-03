import { describe, expect, it } from "vitest";
import { resolveApplicationMode } from "./mode";
import { createUnavailablePort, getApplicationPort, installApplicationPort } from "./port";
import { parseEnvelope } from "./protocol";
import validFixtures from "../../../tests/fixtures/controller/valid.json";
import invalidFixtures from "../../../tests/fixtures/controller/invalid.json";

describe("application contract", () => {
  it("rejects_invalid_role_and_unsafe_wire_fields", () => {
    expect(resolveApplicationMode("android")).toBe("controller");
    expect(() => parseEnvelope({ command: "audio_play", path: "C:/Music/a.flac" })).toThrow();
    for (const platform of ["windows", "macos", "linux"]) {
      expect(resolveApplicationMode(platform)).toBe("desktop");
    }
    for (const platform of ["ios", "web", "", "Android"]) {
      expect(() => resolveApplicationMode(platform)).toThrow();
    }
  });
});

it("port_never_falls_back", async () => {
  expect(() => getApplicationPort()).toThrow("Application not ready");
  const unavailable = createUnavailablePort();
  await expect(unavailable.query({ type: "snapshot" })).rejects.toThrow("Application unavailable");
  await expect(unavailable.execute({ type: "pause" }, { hostEpoch: "epoch-1" })).rejects.toThrow("Application unavailable");
  await expect(unavailable.resolveArtwork({ resourceId: "art-1", revision: 1 })).rejects.toThrow("Application unavailable");
  const updates: unknown[] = [];
  const unsubscribe = unavailable.subscribe((update: unknown) => updates.push(update));
  unsubscribe();
  expect(updates).toEqual([]);
});

it("stale_cleanup_cannot_uninstall_a_newer_port_even_when_reused", () => {
  const first = createUnavailablePort();
  const second = createUnavailablePort();
  const releaseFirst = installApplicationPort(first);
  expect(getApplicationPort()).toBe(first);
  const releaseSecond = installApplicationPort(second);
  releaseFirst();
  expect(getApplicationPort()).toBe(second);
  const releaseReused = installApplicationPort(second);
  releaseSecond();
  expect(getApplicationPort()).toBe(second);
  releaseReused();
  expect(() => getApplicationPort()).toThrow("Application not ready");
  releaseFirst();
  releaseReused();
  expect(() => getApplicationPort()).toThrow("Application not ready");
});

for (const fixture of validFixtures) {
  it(`accepts shared fixture ${fixture.name}`, () => {
    expect(parseEnvelope(fixture.envelope)).toEqual(fixture.envelope);
  });
}
for (const fixture of invalidFixtures) {
  it(`rejects shared fixture ${fixture.name}`, () => {
    expect(() => parseEnvelope(fixture.envelope)).toThrow();
  });
}

it.each([NaN, Infinity, -Infinity])("rejects non-finite seek and volume %s", value => {
  for (const intent of [{ type: "seek", seconds: value }, { type: "set_volume", volume: value }]) {
    expect(() => parseEnvelope({ protocolVersion: 1, requestId: "r", preconditions: { hostEpoch: "e", outputRevision: 0 }, intent })).toThrow();
  }
});

it("returns validated data detached from caller-owned command objects", () => {
  const source = { protocolVersion: 1, requestId: "r", preconditions: { hostEpoch: "e", queueRevision: 0, libraryRevision: 0 }, intent: { type: "queue_append", trackIds: [1, 2] } };
  const parsed = parseEnvelope(source);
  source.preconditions.hostEpoch = "changed";
  source.intent.trackIds.push(3);
  expect(parsed.preconditions.hostEpoch).toBe("e");
  expect(parsed.intent).toEqual({ type: "queue_append", trackIds: [1, 2] });
});

it("rejects lone Unicode surrogates", () => {
  expect(() => parseEnvelope({ protocolVersion: 1, requestId: "\ud800", preconditions: { hostEpoch: "e", outputRevision: 0 }, intent: { type: "pause" } })).toThrow();
});
