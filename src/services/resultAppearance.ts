import type { AppConfig } from "../types";
import {
  RESULT_FONT_SIZE_MAX,
  RESULT_FONT_SIZE_MIN,
  RESULT_OPACITY_MAX,
  RESULT_OPACITY_MIN,
} from "../types";
import { saveSettings } from "./tauri";

/** CSS custom property carrying the mini-bar background alpha. */
export const RESULT_OPACITY_VARIABLE = "--result-opacity";
/** CSS custom property carrying the mini-bar translation font size. */
export const RESULT_FONT_SIZE_VARIABLE = "--result-font-size";

/**
 * Clamps an opacity value into the allowed 0.3–1.0 range.
 *
 * The mini-bar background uses this alpha so the desktop shows through the
 * transparent window; text color is never affected.
 */
export function clampResultOpacity(value: number): number {
  if (!Number.isFinite(value)) return RESULT_OPACITY_MIN;
  return Math.min(RESULT_OPACITY_MAX, Math.max(RESULT_OPACITY_MIN, value));
}

/**
 * Clamps and rounds a font size into the allowed 12–24 integer range.
 */
export function clampResultFontSize(value: number): number {
  if (!Number.isFinite(value)) return RESULT_FONT_SIZE_MIN;
  const clamped = Math.min(RESULT_FONT_SIZE_MAX, Math.max(RESULT_FONT_SIZE_MIN, value));
  return Math.round(clamped);
}

/**
 * Applies the mini-bar appearance as CSS custom properties on a root node.
 *
 * Tauri 2.11.5 has no window-level opacity API, so transparency is realised
 * purely with a CSS background alpha. Only the two custom properties are
 * touched; no window API (and in particular no `setOpacity`) is involved.
 */
export function applyResultAppearance(
  root: { style: { setProperty(name: string, value: string): void } },
  opacity: number,
  fontSizePx: number,
): void {
  root.style.setProperty(RESULT_OPACITY_VARIABLE, clampResultOpacity(opacity).toFixed(2));
  root.style.setProperty(RESULT_FONT_SIZE_VARIABLE, `${clampResultFontSize(fontSizePx)}px`);
}

/**
 * Persists the mini-bar appearance by saving the whole configuration.
 *
 * The appearance controls apply changes immediately through
 * {@link applyResultAppearance}; this function persists the same values so
 * they survive restarts.
 */
export async function persistResultAppearance(
  config: AppConfig,
  opacity: number,
  fontSizePx: number,
): Promise<AppConfig> {
  const next: AppConfig = {
    ...config,
    result_window: {
      ...config.result_window,
      opacity: clampResultOpacity(opacity),
      font_size_px: clampResultFontSize(fontSizePx),
    },
  };
  await saveSettings(next);
  return next;
}
