/** Minimal window surface needed for floating-ball visibility. */
export interface FloatWindow {
  show(): Promise<void>;
  hide(): Promise<void>;
}

/**
 * Shows or hides the floating ball window.
 *
 * Failures are swallowed: a missing capability or a closed window must not
 * break the rest of the UI. No sensitive data is involved.
 */
export function applyFloaterVisibility(window: FloatWindow, enabled: boolean): void {
  void (enabled ? window.show() : window.hide()).catch(() => undefined);
}
