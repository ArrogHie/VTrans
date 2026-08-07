import { describe, expect, it } from "vitest";
import {
  computeFloaterWindowSize,
  DEFAULT_MENU_HEIGHT,
  FLOATER_PADDING_PX,
  MENU_WIDTH,
} from "../utils/floaterLayout";

describe("computeFloaterWindowSize", () => {
  it("keeps at least 16px transparent padding around the ball", () => {
    expect(FLOATER_PADDING_PX).toBeGreaterThanOrEqual(16);
  });

  it("sizes the collapsed window to the ball plus padding on every side", () => {
    const { width, height } = computeFloaterWindowSize(false, 48, null);
    expect(width).toBe(48 + 2 * FLOATER_PADDING_PX);
    expect(height).toBe(48 + 2 * FLOATER_PADDING_PX);
  });

  it("keeps the ball outermost edge (ring/shadow included) inside the collapsed window", () => {
    const sizePx = 48;
    const { width, height } = computeFloaterWindowSize(false, sizePx, null);
    // 球位于 (PAD, PAD)，外沿 = PAD + 球径 + 2px ring；PAD ≥ 16
    // 保证 ring 与 shadow 完整落在窗口内，任何状态无裁剪。
    expect(FLOATER_PADDING_PX + sizePx + 2).toBeLessThanOrEqual(width);
    expect(FLOATER_PADDING_PX + sizePx + 2).toBeLessThanOrEqual(height);
  });

  it("sizes the expanded width to the menu width plus padding", () => {
    const { width } = computeFloaterWindowSize(true, 48, 300);
    expect(width).toBe(MENU_WIDTH + 2 * FLOATER_PADDING_PX);
  });

  it("aligns the expanded height with the measured menu content (no blank band)", () => {
    const sizePx = 48;
    const menuHeight = 282;
    const { height } = computeFloaterWindowSize(true, sizePx, menuHeight);
    // 窗口高 = 上边距 + 球 + 面板实际高 + 下边距：没有额外空白带。
    expect(height).toBe(FLOATER_PADDING_PX + sizePx + menuHeight + FLOATER_PADDING_PX);
  });

  it("keeps the panel bottom inside the window with padding for its shadow", () => {
    const sizePx = 48;
    const menuHeight = 282;
    const { height } = computeFloaterWindowSize(true, sizePx, menuHeight);
    // 面板底边 = PAD + 球 + 面板高，窗口底边多出 PAD 透明余量，
    // 阴影不会被底边裁出明显方块。
    expect(FLOATER_PADDING_PX + sizePx + menuHeight).toBeLessThanOrEqual(height);
  });

  it("falls back to the default menu height before the first measurement", () => {
    const { height } = computeFloaterWindowSize(true, 48, null);
    expect(height).toBe(48 + 2 * FLOATER_PADDING_PX + DEFAULT_MENU_HEIGHT);
  });
});
