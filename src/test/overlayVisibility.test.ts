import { describe, expect, it } from "vitest";
import type { ScreenRegion } from "../types";
import { shouldRestoreOverlay } from "../utils/overlayVisibility";

const REGION: ScreenRegion = {
  monitor_id: "\\.\\DISPLAY1",
  x: 400,
  y: 300,
  width: 800,
  height: 400,
};

describe("shouldRestoreOverlay", () => {
  it("never restores the marker for a single-mode snapshot", () => {
    expect(shouldRestoreOverlay({ mode: "single", selected_region: REGION })).toBe(false);
  });

  it("restores the marker for a live-mode snapshot with a region", () => {
    expect(shouldRestoreOverlay({ mode: "live", selected_region: REGION })).toBe(true);
  });

  it("never restores the marker without a selected region", () => {
    expect(shouldRestoreOverlay({ mode: "live", selected_region: null })).toBe(false);
    expect(shouldRestoreOverlay({ mode: "single", selected_region: null })).toBe(false);
  });
});
