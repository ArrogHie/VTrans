import { describe, expect, it, vi } from "vitest";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import {
  applyResultAppearance,
  clampResultFontSize,
  clampResultOpacity,
  persistResultAppearance,
  RESULT_FONT_SIZE_VARIABLE,
  RESULT_OPACITY_VARIABLE,
} from "../services/resultAppearance";

describe("clampResultOpacity", () => {
  it("keeps in-range values unchanged", () => {
    expect(clampResultOpacity(0.7)).toBe(0.7);
  });

  it("clamps values outside 0.3..1.0", () => {
    expect(clampResultOpacity(0.1)).toBe(0.3);
    expect(clampResultOpacity(1.5)).toBe(1.0);
  });

  it("falls back to the minimum for non-finite values", () => {
    expect(clampResultOpacity(Number.NaN)).toBe(0.3);
  });
});

describe("clampResultFontSize", () => {
  it("rounds and keeps in-range values unchanged", () => {
    expect(clampResultFontSize(16)).toBe(16);
    expect(clampResultFontSize(16.4)).toBe(16);
  });

  it("clamps values outside 12..24", () => {
    expect(clampResultFontSize(10)).toBe(12);
    expect(clampResultFontSize(30)).toBe(24);
  });
});

describe("applyResultAppearance", () => {
  it("sets only CSS custom properties, never a window opacity API", () => {
    const setProperty = vi.fn();
    const root = { style: { setProperty } };
    applyResultAppearance(root, 0.6, 16);
    expect(setProperty).toHaveBeenCalledWith(RESULT_OPACITY_VARIABLE, "0.60");
    expect(setProperty).toHaveBeenCalledWith(RESULT_FONT_SIZE_VARIABLE, "16px");
    // Tauri 2.11.5 无窗口级 opacity 能力：透明完全由 CSS 背景 alpha 实现，
    // 任何窗口 API（尤其 setOpacity）都不得出现。
    expect(setProperty.mock.calls.some(([name]) => String(name).includes("setOpacity"))).toBe(false);
  });

  it("clamps out-of-range values before writing", () => {
    const setProperty = vi.fn();
    applyResultAppearance({ style: { setProperty } }, 0.05, 40);
    expect(setProperty).toHaveBeenCalledWith(RESULT_OPACITY_VARIABLE, "0.30");
    expect(setProperty).toHaveBeenCalledWith(RESULT_FONT_SIZE_VARIABLE, "24px");
  });
});

describe("persistResultAppearance", () => {
  it("persists clamped appearance through the dedicated backend command", async () => {
    invoke.mockResolvedValueOnce(undefined);
    await persistResultAppearance(0.55, 20);
    // 不再走整包 save_settings：只携带 opacity / fontSizePx 两个参数，
    // 实时会话运行期间也可以保存。
    expect(invoke).toHaveBeenCalledWith(
      "update_result_window_appearance",
      { opacity: 0.55, fontSizePx: 20 },
    );
  });

  it("clamps out-of-range values before invoking the command", async () => {
    invoke.mockResolvedValueOnce(undefined);
    await persistResultAppearance(0.05, 40);
    expect(invoke).toHaveBeenCalledWith(
      "update_result_window_appearance",
      { opacity: 0.3, fontSizePx: 24 },
    );
  });
});
