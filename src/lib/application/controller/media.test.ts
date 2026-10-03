import { expect, it, vi } from "vitest";
import { createArtworkHandle } from "./media";
it("bounds native images and revokes exactly once", () => {
    const create = vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:image"), revoke = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {
    });
    const h = createArtworkHandle({ mime: "image/png", bytes: [137, 80, 78, 71] });
    h.dispose();
    h.dispose();
    expect(revoke).toHaveBeenCalledExactlyOnceWith("blob:image");
    expect(() => createArtworkHandle({ mime: "image/svg+xml", bytes: [1] })).toThrow();
    expect(() => createArtworkHandle({ mime: "image/png", bytes: new Uint8Array(5 * 1024 * 1024 + 1) })).toThrow();
    expect(create).toHaveBeenCalledOnce();
    vi.restoreAllMocks();
});
