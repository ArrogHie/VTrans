import type { AppStatus, ScreenRegion } from "../types";

/**
 * Whether a backend status snapshot should restore the persistent region
 * marker during hydration.
 *
 * The marker belongs to live sessions only: a live session (running or
 * paused) restores the marker for its selected region, while a single-mode
 * selection must never bring the marker back. This is the frontend half of
 * the overlay lifecycle contract; the backend applies the same rule when a
 * selection is confirmed or a single capture finishes.
 */
export function shouldRestoreOverlay(
  status: Pick<AppStatus, "mode" | "selected_region">,
): status is Pick<AppStatus, "mode" | "selected_region"> & {
  selected_region: ScreenRegion;
} {
  return status.mode === "live" && status.selected_region !== null;
}
