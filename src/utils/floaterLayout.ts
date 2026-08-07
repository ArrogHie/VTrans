/** Transparent padding around the ball inside the floater window (px). */
export const FLOATER_PADDING_PX = 16;

/** Width of the expanded menu panel (px). */
export const MENU_WIDTH = 220;

/**
 * Fallback menu height used before the first real measurement (px).
 *
 * The expanded window height is aligned to the measured menu content; until
 * the first `ResizeObserver` measurement arrives this value keeps the
 * expanded window large enough to render the whole menu.
 */
export const DEFAULT_MENU_HEIGHT = 300;

/** Logical size of the floating ball window in CSS pixels. */
export interface FloaterWindowSize {
  width: number;
  height: number;
}

/**
 * Computes the logical window size for the floating ball.
 *
 * The collapsed window is the ball plus `FLOATER_PADDING_PX` on every side,
 * so the ring/shadow around the ball never gets clipped and the extra
 * transparent margin is invisible. The expanded window adds the menu panel
 * below the ball area: the height equals the ball area plus the measured
 * menu height plus one bottom padding (which keeps the panel shadow inside
 * the window instead of producing a clipped grey block). When the menu has
 * not been measured yet the {@link DEFAULT_MENU_HEIGHT} fallback is used.
 */
export function computeFloaterWindowSize(
  open: boolean,
  sizePx: number,
  menuHeight: number | null,
): FloaterWindowSize {
  const paddedBall = sizePx + 2 * FLOATER_PADDING_PX;
  if (!open) return { width: paddedBall, height: paddedBall };
  return {
    width: MENU_WIDTH + 2 * FLOATER_PADDING_PX,
    height: sizePx + 2 * FLOATER_PADDING_PX + (menuHeight ?? DEFAULT_MENU_HEIGHT),
  };
}
