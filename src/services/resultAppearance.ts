import type { AppConfig } from "../types";
import {
  RESULT_FONT_SIZE_MAX,
  RESULT_FONT_SIZE_MIN,
  RESULT_OPACITY_MAX,
  RESULT_OPACITY_MIN,
} from "../types";
import { updateResultWindowAppearance } from "./tauri";

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
 * Applies the persisted result-window appearance from a hydrated config.
 *
 * Returns the clamped values so the caller can also feed them into local
 * React state; this keeps hydration testable without a DOM.
 */
export function applyHydratedAppearance(
  config: Pick<AppConfig, "result_window">,
  root?: { style: { setProperty(name: string, value: string): void } } | null,
): { opacity: number; fontSizePx: number } {
  const opacity = clampResultOpacity(config.result_window.opacity);
  const fontSizePx = clampResultFontSize(config.result_window.font_size_px);
  if (root) applyResultAppearance(root, opacity, fontSizePx);
  return { opacity, fontSizePx };
}

/**
 * Persists the mini-bar appearance through the dedicated backend command.
 *
 * The appearance controls apply changes immediately through
 * {@link applyResultAppearance}; this function persists the same values so
 * they survive restarts. Unlike a whole-configuration `save_settings`, the
 * command does not rebuild the translation provider and works while a live
 * session is running.
 */
export async function persistResultAppearance(opacity: number, fontSizePx: number): Promise<void> {
  await updateResultWindowAppearance(
    clampResultOpacity(opacity),
    clampResultFontSize(fontSizePx),
  );
}
