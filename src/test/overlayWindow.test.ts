import { describe, expect, it } from "vitest";
import { overlayBox } from "../windows/OverlayWindow";

const REGION = {
  monitor_id: "\\\\.\\DISPLAY1",
  x: 120,
  y: 240,
  width: 480,
  height: 320,
};

describe("overlayBox", () => {
  it("maps physical pixels 1:1 at 100% device pixel ratio", () => {
    expect(overlayBox(REGION, 1)).toEqual({
      left: 120,
      top: 240,
      width: 480,
      height: 320,
    });
  });

  it("divides by the device pixel ratio at 150% scaling", () => {
    expect(overlayBox(REGION, 1.5)).toEqual({
      left: 80,
      top: 160,
      width: 320,
      height: 213,
    });
  });

  it("clamps degenerate device pixel ratios to 1", () => {
    expect(overlayBox(REGION, 0)).toEqual({
      left: 120,
      top: 240,
      width: 480,
      height: 320,
    });
  });
});
