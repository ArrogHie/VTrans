import { describe, expect, it } from "vitest";
import { isRegionSelectionCancelled, toPhysicalRegion } from "../services/tauri";

describe("toPhysicalRegion", () => {
  it("converts a dragged logical rectangle to physical pixels", () => {
    expect(toPhysicalRegion("display-1", { x: 100, y: 50 }, { x: 20, y: 10 }, 1.5)).toEqual({
      monitor_id: "display-1",
      x: 30,
      y: 15,
      width: 120,
      height: 60,
    });
  });

  it("returns null for a zero-sized selection", () => {
    expect(toPhysicalRegion("display-1", { x: 20, y: 20 }, { x: 20, y: 20 }, 2)).toBeNull();
  });

  it("normalizes a drag that ends above and left of its start", () => {
    expect(toPhysicalRegion("display-1", { x: 10, y: 10 }, { x: 90, y: 50 }, 1)).toEqual({
      monitor_id: "display-1",
      x: 10,
      y: 10,
      width: 80,
      height: 40,
    });
  });
});

describe("isRegionSelectionCancelled", () => {
  it("recognizes the backend cancellation error string", () => {
    expect(isRegionSelectionCancelled("state not initialized")).toBe(true);
    expect(isRegionSelectionCancelled("capture error: monitor not found")).toBe(false);
  });

  it("treats unexpected rejection shapes as non-cancellation", () => {
    expect(isRegionSelectionCancelled(null)).toBe(false);
    expect(isRegionSelectionCancelled(undefined)).toBe(false);
    expect(isRegionSelectionCancelled(new Error("state not initialized"))).toBe(true);
  });
});
