import { describe, expect, it } from "vitest";
import { upsertOverlayBox, type OverlayMultiBox } from "../windows/OverlayWindow";

const REGION = { monitor_id: "\\\\.\\DISPLAY1", x: 0, y: 0, width: 100, height: 100 };

describe("upsertOverlayBox", () => {
  it("appends a new box", () => {
    const box: OverlayMultiBox = { box_id: 0, color: "#FF6B6B", region: REGION };
    expect(upsertOverlayBox([], box)).toEqual([box]);
  });

  it("replaces an existing box with the same id (idempotent hydration + event)", () => {
    const first: OverlayMultiBox = { box_id: 0, color: "#FF6B6B", region: REGION };
    const updated: OverlayMultiBox = {
      box_id: 0,
      color: "#FF6B6B",
      region: { ...REGION, width: 200 },
    };
    const list = upsertOverlayBox([first], updated);
    expect(list).toHaveLength(1);
    expect(list[0].region.width).toBe(200);
  });

  it("keeps distinct boxes in insertion order", () => {
    const a: OverlayMultiBox = { box_id: 0, color: "#FF6B6B", region: REGION };
    const b: OverlayMultiBox = { box_id: 1, color: "#4ECDC4", region: REGION };
    const list = upsertOverlayBox(upsertOverlayBox([], a), b);
    expect(list.map((entry) => entry.box_id)).toEqual([0, 1]);
  });
});
