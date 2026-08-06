import {
  FLOATER_OPACITY_MAX,
  FLOATER_OPACITY_MIN,
  FLOATER_SIZE_MAX,
  FLOATER_SIZE_MIN,
} from "../types";
import { updateFloatingBallAppearance } from "./tauri";

/** CSS custom property carrying the floating ball background alpha. */
export const FLOATER_OPACITY_VARIABLE = "--floater-opacity";
/** CSS custom property carrying the floating ball diameter. */
export const FLOATER_SIZE_VARIABLE = "--floater-size";

/**
 * Clamps an opacity value into the allowed 0.3–1.0 range.
 *
 * The ball background uses this alpha so the desktop shows through the
 * transparent window; the icon is never faded.
 */
export function clampFloaterOpacity(value: number): number {
  if (!Number.isFinite(value)) return FLOATER_OPACITY_MIN;
  return Math.min(FLOATER_OPACITY_MAX, Math.max(FLOATER_OPACITY_MIN, value));
}

/**
 * Clamps and rounds a ball diameter into the allowed 32–72 integer range.
 */
export function clampFloaterSizePx(value: number): number {
  if (!Number.isFinite(value)) return FLOATER_SIZE_MIN;
  const clamped = Math.min(FLOATER_SIZE_MAX, Math.max(FLOATER_SIZE_MIN, value));
  return Math.round(clamped);
}

/**
 * Applies the floating ball appearance as CSS custom properties on a root
 * node.
 *
 * Tauri 2.11.5 has no window-level opacity API, so transparency is realised
 * purely with a CSS background alpha and the diameter is carried by a CSS
 * length variable consumed by Tailwind's `w-[var(...)]`/`h-[var(...)]`
 * utilities. No window API (and in particular no `setOpacity`) is involved.
 */
export function applyFloaterAppearance(
  root: { style: { setProperty(name: string, value: string): void } },
  opacity: number,
  sizePx: number,
): void {
  root.style.setProperty(FLOATER_OPACITY_VARIABLE, clampFloaterOpacity(opacity).toFixed(2));
  root.style.setProperty(FLOATER_SIZE_VARIABLE, `${clampFloaterSizePx(sizePx)}px`);
}

/**
 * Persists the floating ball appearance through the dedicated backend
 * command.
 *
 * The menu controls apply changes immediately through
 * {@link applyFloaterAppearance}; this function persists the same values so
 * they survive restarts. The command never touches the live lifecycle lock,
 * so appearance changes apply while a live session is running.
 */
export async function persistFloaterAppearance(opacity: number, sizePx: number): Promise<void> {
  await updateFloatingBallAppearance(
    clampFloaterOpacity(opacity),
    clampFloaterSizePx(sizePx),
  );
}
