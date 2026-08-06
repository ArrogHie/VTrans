import { describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import {
  applyFloaterAppearance,
  clampFloaterOpacity,
  clampFloaterSizePx,
  FLOATER_OPACITY_VARIABLE,
  FLOATER_SIZE_VARIABLE,
  persistFloaterAppearance,
} from "../services/floaterAppearance";

describe("clampFloaterOpacity", () => {
  it("keeps in-range values unchanged", () => {
    expect(clampFloaterOpacity(0.75)).toBe(0.75);
  });

  it("clamps values outside 0.3..1.0", () => {
    expect(clampFloaterOpacity(0.1)).toBe(0.3);
    expect(clampFloaterOpacity(1.5)).toBe(1.0);
  });

  it("falls back to the minimum for non-finite values", () => {
    expect(clampFloaterOpacity(Number.NaN)).toBe(0.3);
  });
});

describe("clampFloaterSizePx", () => {
  it("rounds and keeps in-range values unchanged", () => {
    expect(clampFloaterSizePx(48)).toBe(48);
    expect(clampFloaterSizePx(48.4)).toBe(48);
  });

  it("clamps values outside 32..72", () => {
    expect(clampFloaterSizePx(24)).toBe(32);
    expect(clampFloaterSizePx(80)).toBe(72);
  });
});

describe("applyFloaterAppearance", () => {
  it("sets only CSS custom properties, never a window opacity API", () => {
    const setProperty = vi.fn();
    applyFloaterAppearance({ style: { setProperty } }, 0.6, 56);
    expect(setProperty).toHaveBeenCalledWith(FLOATER_OPACITY_VARIABLE, "0.60");
    expect(setProperty).toHaveBeenCalledWith(FLOATER_SIZE_VARIABLE, "56px");
    // Tauri 2.11.5 无窗口级 opacity 能力：任何窗口 API（尤其 setOpacity）
    // 都不得出现。
    expect(setProperty.mock.calls.some(([name]) => String(name).includes("setOpacity"))).toBe(false);
  });

  it("clamps out-of-range values before writing", () => {
    const setProperty = vi.fn();
    applyFloaterAppearance({ style: { setProperty } }, 0.05, 80);
    expect(setProperty).toHaveBeenCalledWith(FLOATER_OPACITY_VARIABLE, "0.30");
    expect(setProperty).toHaveBeenCalledWith(FLOATER_SIZE_VARIABLE, "72px");
  });
});

describe("persistFloaterAppearance", () => {
  it("persists clamped appearance through the dedicated backend command", async () => {
    invoke.mockResolvedValueOnce(undefined);
    await persistFloaterAppearance(0.75, 56);
    expect(invoke).toHaveBeenCalledWith("update_floating_ball_appearance", {
      opacity: 0.75,
      sizePx: 56,
    });
  });

  it("clamps out-of-range values before invoking the command", async () => {
    invoke.mockResolvedValueOnce(undefined);
    await persistFloaterAppearance(0.05, 24);
    expect(invoke).toHaveBeenCalledWith("update_floating_ball_appearance", {
      opacity: 0.3,
      sizePx: 32,
    });
  });
});
