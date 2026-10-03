import type { ArtworkHandle } from "../types";
export interface NativeImage {
    mime: string;
    bytes: number[] | Uint8Array;
}
export function createArtworkHandle(image: NativeImage): ArtworkHandle {
    if (!["image/png", "image/jpeg", "image/webp"].includes(image.mime) || !image.bytes.length || image.bytes.length > 5 * 1024 * 1024 || Array.from(image.bytes).some(n => !Number.isInteger(n) || n < 0 || n > 255))
        throw new Error("Invalid native image");
    const src = URL.createObjectURL(new Blob([new Uint8Array(image.bytes)], { type: image.mime }));
    let disposed = false;
    return { src, dispose() {
            if (!disposed) {
                disposed = true;
                URL.revokeObjectURL(src);
            }
        } };
}
