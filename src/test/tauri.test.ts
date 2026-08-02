import { describe, expect, it } from "vitest";
import { toPhysicalRegion } from "../services/tauri";

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
});
