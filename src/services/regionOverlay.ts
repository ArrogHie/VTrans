import { PhysicalPosition, PhysicalSize } from "@tauri-apps/api/dpi";
import { emit } from "@tauri-apps/api/event";
import { availableMonitors } from "@tauri-apps/api/window";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import type { ScreenRegion } from "../types";

const OVERLAY_LABEL = "overlay";

/**
 * Shows the persistent screen-level region marker over the selected monitor.
 *
 * The overlay webview is a borderless, transparent, always-on-top and
 * click-through window configured in `tauri.conf.json`. It covers the
 * region's monitor completely (window origin = monitor origin, window size =
 * monitor size); the region is published through the `overlay_region_updated`
 * event and the window draws the border at the region's monitor-relative
 * offset with pure CSS. Only coordinates cross IPC; no image data is ever
 * transferred.
 *
 * Failures are logged but never propagated: the marker is a convenience
 * visual, and a missing capability or monitor must not break translation.
 */
export async function showRegionOverlay(region: ScreenRegion): Promise<void> {
  try {
    const overlay = await WebviewWindow.getByLabel(OVERLAY_LABEL);
    if (!overlay) return;
    const monitors = await availableMonitors();
    // `ScreenRegion` coordinates are physical pixels relative to the region's
    // monitor; fall back to the first monitor when the name no longer matches
    // (for example after a display topology change).
    const monitor = monitors.find((candidate) => candidate.name === region.monitor_id) ?? monitors[0];
    if (!monitor) return;
    await overlay.setPosition(
      new PhysicalPosition(monitor.position.x, monitor.position.y),
    );
    await overlay.setSize(new PhysicalSize(monitor.size.width, monitor.size.height));
    await overlay.setIgnoreCursorEvents(true);
    await emit("overlay_region_updated", region);
    await overlay.show();
  } catch (error) {
    console.warn("[vtrans] failed to show region overlay", error);
  }
}

/**
 * Positions the overlay window on the first translation box's monitor without
 * showing it.
 *
 * The backend's `start_multi_realtime` shows the overlay window but never
 * positions it, so the multi-box session must align the window to the boxes'
 * monitor before the backend makes it visible — otherwise the frames would be
 * drawn against a stale/default window geometry. The window is positioned and
 * sized to the first box's monitor (falling back to the primary monitor when
 * the box's monitor cannot be resolved, for example after a display topology
 * change). A single overlay window can only cover one monitor; frames on
 * other monitors do not align (see the multi-box section of README.md).
 *
 * Deliberately does not show the window: the backend shows it as part of
 * starting the session, avoiding a flash of an empty overlay.
 *
 * Failures are logged but never propagated: positioning is a convenience and
 * must not break starting the translation session.
 */
export async function showMultiBoxOverlay(boxes: { region: ScreenRegion }[]): Promise<void> {
  try {
    if (boxes.length === 0) return;
    const overlay = await WebviewWindow.getByLabel(OVERLAY_LABEL);
    if (!overlay) return;
    const monitors = await availableMonitors();
    if (monitors.length === 0) return;
    const monitor =
      monitors.find((candidate) => candidate.name === boxes[0].region.monitor_id) ?? monitors[0];
    await overlay.setPosition(
      new PhysicalPosition(monitor.position.x, monitor.position.y),
    );
    await overlay.setSize(new PhysicalSize(monitor.size.width, monitor.size.height));
    await overlay.setIgnoreCursorEvents(true);
  } catch (error) {
    console.warn("[vtrans] failed to position multi-box overlay", error);
  }
}

/** Hides the persistent region marker and clears its content. */
export async function hideRegionOverlay(): Promise<void> {
  try {
    const overlay = await WebviewWindow.getByLabel(OVERLAY_LABEL);
    if (!overlay) return;
    await emit("overlay_hidden");
    await overlay.hide();
  } catch (error) {
    console.warn("[vtrans] failed to hide region overlay", error);
  }
}
