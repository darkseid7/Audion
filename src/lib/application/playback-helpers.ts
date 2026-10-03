/** Pure playback classification and volume conversions shared by both roles. */
import type { Track } from "$lib/api/tauri";

export function isStreaming(track: Track): boolean {
  // 1. Explicitly local sources (by type or path)
  if (track.source_type === "local" || track.local_src) return false;

  if (track.path) {
    // Tauri local protocols are always local
    if (
      track.path.startsWith("file://") ||
      track.path.startsWith("asset://") ||
      track.path.startsWith("tauri://")
    ) {
      return false;
    }
    // Explicitly streaming protocols
    if (track.path.startsWith("http://") || track.path.startsWith("https://")) {
      return true;
    }
  }

  // 3. Known external source types (Tidal, etc.)
  if (track.source_type && track.source_type !== "local") return true;

  // 4. Default to local for anything else (safer for absolute paths)
  return false;
}

export function sliderToAudioVolume(sliderValue: number): number {
  // Quadratic curve: softer at low end, more range at high end
  // Alternative: Math.pow(sliderValue, 2.5) for steeper curve
  return Math.pow(sliderValue, 2);
}

export function audioVolumeToSlider(audioVolume: number): number {
  return Math.sqrt(audioVolume);
}
