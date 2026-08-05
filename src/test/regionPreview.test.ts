import { describe, expect, it } from "vitest";
import { regionPreviewBox } from "../utils/regionPreview";

describe("regionPreviewBox", () => {
  it("centers a region that fits the preview", () => {
    const box = regionPreviewBox({ width: 640, height: 480 }, 160, 120);
    expect(box).toEqual({ left: 0, top: 0, width: 160, height: 120 });
  });

  it("scales down a wide region preserving aspect ratio", () => {
    const box = regionPreviewBox({ width: 1920, height: 1080 }, 160, 120);
    // scale = min(160/1920, 120/1080) = 1/12 → 160 × 90，垂直居中 (120-90)/2。
    expect(box).toEqual({ left: 0, top: 15, width: 160, height: 90 });
  });

  it("scales down a tall region preserving aspect ratio", () => {
    const box = regionPreviewBox({ width: 600, height: 1600 }, 160, 120);
    // scale = min(160/600, 120/1600) = 0.075 → 45 × 120，水平居中 (160-45)/2。
    expect(box).toEqual({ left: 58, top: 0, width: 45, height: 120 });
  });

  it("handles degenerate zero dimensions without dividing by zero", () => {
    const box = regionPreviewBox({ width: 0, height: 0 }, 160, 120);
    expect(box.width).toBeGreaterThan(0);
    expect(box.height).toBeGreaterThan(0);
  });
});
