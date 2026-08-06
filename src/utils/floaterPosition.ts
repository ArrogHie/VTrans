/** Screen-space position of the floating ball window (physical pixels). */
export interface FloaterPosition {
  x: number;
  y: number;
}

/** Monitor descriptor used for clamping. */
export interface FloaterMonitor {
  position: { x: number; y: number };
  size: { width: number; height: number };
}

/** localStorage key holding the floating ball position. */
export const FLOATER_POSITION_KEY = "vtrans.floater.position";

/** Size of the collapsed floating ball, matching `tauri.conf.json`. */
export const FLOATER_BALL_SIZE = 48;

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

/**
 * Clamps a floating ball position so the whole window stays on a monitor.
 *
 * The third argument is the full window size in physical pixels (ball
 * diameter plus the transparent padding on both sides), not the ball size:
 * the transparent margin around the ball must also stay on screen.
 *
 * The monitor that contains the ball's centre wins; when the saved position
 * no longer matches any monitor (display topology changed), the first
 * monitor is used as the fallback.
 */
export function clampFloaterPosition(
  position: FloaterPosition,
  monitors: readonly FloaterMonitor[],
  windowSize = FLOATER_BALL_SIZE,
): FloaterPosition {
  if (monitors.length === 0) return position;
  const centreX = position.x + windowSize / 2;
  const centreY = position.y + windowSize / 2;
  const target =
    monitors.find(
      (monitor) =>
        centreX >= monitor.position.x &&
        centreX <= monitor.position.x + monitor.size.width &&
        centreY >= monitor.position.y &&
        centreY <= monitor.position.y + monitor.size.height,
    ) ?? monitors[0];
  return {
    x: clamp(position.x, target.position.x, target.position.x + target.size.width - windowSize),
    y: clamp(position.y, target.position.y, target.position.y + target.size.height - windowSize),
  };
}

/**
 * Loads a previously saved floating ball position.
 *
 * Returns `null` when nothing is stored or the stored value is malformed;
 * storage read failures (private mode, quota) are treated the same way.
 */
export function loadFloaterPosition(
  storage: Pick<Storage, "getItem">,
  key = FLOATER_POSITION_KEY,
): FloaterPosition | null {
  try {
    const raw = storage.getItem(key);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<FloaterPosition>;
    if (
      typeof parsed.x !== "number" ||
      typeof parsed.y !== "number" ||
      !Number.isFinite(parsed.x) ||
      !Number.isFinite(parsed.y)
    ) {
      return null;
    }
    return { x: Math.round(parsed.x), y: Math.round(parsed.y) };
  } catch {
    return null;
  }
}

/** Persists the floating ball position; storage failures are swallowed. */
export function saveFloaterPosition(
  storage: Pick<Storage, "setItem">,
  position: FloaterPosition,
  key = FLOATER_POSITION_KEY,
): void {
  try {
    storage.setItem(key, JSON.stringify(position));
  } catch {
    // 位置记忆是可选的便利功能，存储不可用时静默降级。
  }
}
