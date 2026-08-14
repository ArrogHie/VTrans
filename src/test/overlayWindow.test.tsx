import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import {
  OVERLAY_BORDER_PX,
  OverlayFrame,
  overlayBorderBox,
  overlayBox,
} from "../windows/OverlayWindow";

const REGION = {
  monitor_id: "\\\\.\\DISPLAY1",
  x: 120,
  y: 240,
  width: 480,
  height: 320,
};

const VIEWPORT = { width: 1920, height: 1080 };

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

describe("overlayBorderBox", () => {
  it("outsets every side by the stroke width so no stroke pixel enters the region", () => {
    const box = overlayBox(REGION, 1); // { 120, 240, 480, 320 }
    const border = overlayBorderBox(box, VIEWPORT);
    expect(border).toEqual({
      left: 118,
      top: 238,
      width: 484,
      height: 324,
      insetLeft: 2,
      insetTop: 2,
      insetRight: 2,
      insetBottom: 2,
    });
    // 每侧描边带都紧贴区域外沿、完全落在区域之外。
    expect(border.left + OVERLAY_BORDER_PX).toBe(box.left);
    expect(border.top + OVERLAY_BORDER_PX).toBe(box.top);
    expect(border.left + border.width - OVERLAY_BORDER_PX).toBe(box.left + box.width);
    expect(border.top + border.height - OVERLAY_BORDER_PX).toBe(box.top + box.height);
  });

  it("keeps the element inside the viewport when the region touches the left/top edge", () => {
    const box = { left: 0, top: 0, width: 100, height: 100 };
    const border = overlayBorderBox(box, VIEWPORT);
    expect(border.insetLeft).toBe(0);
    expect(border.insetTop).toBe(0);
    expect(border.left).toBe(0);
    expect(border.top).toBe(0);
    // 贴边侧没有窗口外空间：描边内缩到区域内侧绘制（可见性优先的取舍）。
    expect(border.left + OVERLAY_BORDER_PX).toBeGreaterThan(box.left);
    expect(border.top + OVERLAY_BORDER_PX).toBeGreaterThan(box.top);
    // 元素整体仍落在窗口内。
    expect(border.left).toBeGreaterThanOrEqual(0);
    expect(border.top).toBeGreaterThanOrEqual(0);
    expect(border.left + border.width).toBeLessThanOrEqual(VIEWPORT.width);
    expect(border.top + border.height).toBeLessThanOrEqual(VIEWPORT.height);
  });

  it("clamps only the flush right/bottom sides when the region touches them", () => {
    const box = { left: 1820, top: 980, width: 100, height: 100 };
    const border = overlayBorderBox(box, VIEWPORT);
    expect(border.insetRight).toBe(0);
    expect(border.insetBottom).toBe(0);
    expect(border.left + border.width).toBe(VIEWPORT.width);
    expect(border.top + border.height).toBe(VIEWPORT.height);
    // 各边独立 clamp：左侧仍有完整外移。
    expect(border.insetLeft).toBe(2);
    expect(border.insetTop).toBe(2);
  });

  it("reduces the outset only on sides within one stroke of the viewport edge", () => {
    const box = { left: 1, top: 10, width: 100, height: 100 };
    const border = overlayBorderBox(box, VIEWPORT);
    expect(border.insetLeft).toBe(1);
    expect(border.left).toBe(0);
    expect(border.insetRight).toBe(2);
    expect(border.insetTop).toBe(2);
  });

  it("applies the same 2px outset in CSS pixels after dpr scaling", () => {
    const box = overlayBox(REGION, 1.5); // { 80, 160, 320, 213 }
    const border = overlayBorderBox(box, VIEWPORT);
    expect(border).toEqual({
      left: 78,
      top: 158,
      width: 324,
      height: 217,
      insetLeft: 2,
      insetTop: 2,
      insetRight: 2,
      insetBottom: 2,
    });
    expect(border.left + OVERLAY_BORDER_PX).toBe(box.left);
    expect(border.top + OVERLAY_BORDER_PX).toBe(box.top);
  });

  it("treats a physical edge flush with the monitor as flush after dpr scaling", () => {
    const border = overlayBorderBox(
      overlayBox({ ...REGION, x: 0, y: 0 }, 1.5),
      VIEWPORT,
    );
    expect(border.insetLeft).toBe(0);
    expect(border.insetTop).toBe(0);
    expect(border.insetRight).toBe(2);
    expect(border.insetBottom).toBe(2);
  });

  it("does not clamp when the viewport is unknown", () => {
    const box = overlayBox(REGION, 1);
    const border = overlayBorderBox(box, {
      width: Number.POSITIVE_INFINITY,
      height: Number.POSITIVE_INFINITY,
    });
    expect(border).toEqual({
      left: 118,
      top: 238,
      width: 484,
      height: 324,
      insetLeft: 2,
      insetTop: 2,
      insetRight: 2,
      insetBottom: 2,
    });
  });
});

describe("OverlayFrame", () => {
  it("renders the outset rectangle with the stroke and the label above the top stroke band", () => {
    const box = overlayBox(REGION, 1);
    const geometry = overlayBorderBox(box, VIEWPORT);
    const html = renderToStaticMarkup(
      <OverlayFrame
        geometry={geometry}
        color="#818cf8"
        labelColor="#6366f1"
        label="480 × 320"
        testId="overlay-frame"
      />,
    );
    expect(html).toContain("border-2");
    expect(html).toContain('data-testid="overlay-frame"');
    // 外移后的描边矩形几何。
    expect(html).toContain("left:118px");
    expect(html).toContain("top:238px");
    expect(html).toContain("width:484px");
    expect(html).toContain("height:324px");
    expect(html).toContain("border-color:#818cf8");
    expect(html).toContain("background-color:#6366f1");
    // 尺寸标签：-top-6（24px）上移且自身高 20px，底沿在元素上沿上方 4px，
    // 不压到新外移的顶部描边带 [top, top+2)。
    expect(html).toContain("-top-6");
    expect(html).toContain("480 × 320");
  });
});
