import { describe, expect, it, vi } from "vitest";
import { DEFAULT_CONFIG } from "../types";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import {
  applyHydratedAppearance,
  persistResultAppearance,
} from "../services/resultAppearance";

describe("applyHydratedAppearance", () => {
  it("returns clamped appearance values from a hydrated config", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.result_window.opacity = 0.8;
    config.result_window.font_size_px = 18;
    expect(applyHydratedAppearance(config)).toEqual({ opacity: 0.8, fontSizePx: 18 });
  });

  it("applies the values to CSS variables when a root node is given", () => {
    const setProperty = vi.fn();
    const config = structuredClone(DEFAULT_CONFIG);
    config.result_window.opacity = 0.65;
    config.result_window.font_size_px = 16;
    applyHydratedAppearance(config, { style: { setProperty } });
    expect(setProperty).toHaveBeenCalledWith("--result-opacity", "0.65");
    expect(setProperty).toHaveBeenCalledWith("--result-font-size", "16px");
  });

  it("clamps out-of-range hydrated values", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.result_window.opacity = 0.1;
    config.result_window.font_size_px = 40;
    expect(applyHydratedAppearance(config)).toEqual({ opacity: 0.3, fontSizePx: 24 });
  });
});

describe("persistResultAppearance with hydrated config", () => {
  it("persists appearance through the dedicated command without whole-config saves", async () => {
    invoke.mockResolvedValueOnce(undefined);
    await persistResultAppearance(0.8, 18);
    // 水合后的外观保存不再整包回传配置（也避免了后端对旧 schema 版本的
    // 校验拒绝）；前端 schema 版本与后端一致由 types 测试断言。
    expect(invoke).toHaveBeenCalledWith(
      "update_result_window_appearance",
      { opacity: 0.8, fontSizePx: 18 },
    );
    expect(invoke).not.toHaveBeenCalledWith("save_settings", expect.anything());
  });
});
