export type AlbumView = "grid" | "list";

const ALBUM_VIEW_STORAGE_KEY = "audion_album_view";

export function loadAlbumView(): AlbumView {
  try {
    return typeof localStorage !== "undefined" &&
      localStorage.getItem(ALBUM_VIEW_STORAGE_KEY) === "list"
      ? "list"
      : "grid";
  } catch {
    return "grid";
  }
}

export function saveAlbumView(view: AlbumView): void {
  try {
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(ALBUM_VIEW_STORAGE_KEY, view);
    }
  } catch {
    // Storage is optional; the selected view remains usable this session.
  }
}

export function albumViewTransitionDuration(): number {
  if (typeof window === "undefined") return 0;
  try {
    return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches
      ? 0
      : 180;
  } catch {
    return 0;
  }
}
