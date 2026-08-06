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
  it("saves the hydrated config version 2 when persisting appearance", async () => {
    invoke.mockResolvedValueOnce(undefined);
    const config = structuredClone(DEFAULT_CONFIG);
    const next = await persistResultAppearance(config, 0.8, 18);
    expect(next.version).toBe(2);
    expect(invoke).toHaveBeenCalledWith(
      "save_settings",
      expect.objectContaining({
        settings: expect.objectContaining({
          version: 2,
          result_window: expect.objectContaining({ opacity: 0.8, font_size_px: 18 }),
        }),
      }),
    );
  });
});
