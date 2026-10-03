import type { ApplicationMode } from "./types";

/** Resolve the immutable build role from the native platform, not user preferences. */
export function resolveApplicationMode(platform: string): ApplicationMode {
  switch (platform) {
    case "android": return "controller";
    case "windows":
    case "macos":
    case "linux": return "desktop";
    default: throw new Error("Unsupported application platform");
  }
}
