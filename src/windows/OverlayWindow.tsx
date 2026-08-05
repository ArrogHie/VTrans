import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import type { ScreenRegion } from "../types";

export interface OverlayBox {
  left: number;
  top: number;
  width: number;
  height: number;
}

/**
 * Converts a physical-pixel region into CSS pixels for the overlay webview.
 *
 * The overlay window is positioned and sized to the region's monitor in
 * physical pixels, and `ScreenRegion` coordinates are physical pixels
 * relative to that monitor. CSS pixels equal physical pixels divided by the
 * device pixel ratio, so the marker aligns exactly with the captured area.
 */
export function overlayBox(region: ScreenRegion, devicePixelRatio: number): OverlayBox {
  const scale = Math.max(devicePixelRatio, 1);
  return {
    left: Math.round(region.x / scale),
    top: Math.round(region.y / scale),
    width: Math.round(region.width / scale),
    height: Math.round(region.height / scale),
  };
}

/**
 * Persistent screen-level region marker.
 *
 * A borderless, transparent, always-on-top, click-through window draws the
 * currently selected capture region so the user always sees what part of the
 * screen is being translated. Only coordinates cross IPC; the border is pure
 * CSS and the window never receives mouse input.
 */
export function OverlayWindow() {
  const [region, setRegion] = useState<ScreenRegion | null>(null);

  useEffect(() => {
    let disposed = false;
    let cleanup: (() => void) | undefined;
    void Promise.all([
      listen<ScreenRegion>("overlay_region_updated", (event) => {
        if (!disposed) setRegion(event.payload);
      }),
      listen("overlay_hidden", () => {
        if (!disposed) setRegion(null);
      }),
    ]).then((unlisteners) => {
      if (disposed) {
        for (const unlisten of unlisteners) unlisten();
      } else {
        cleanup = () => {
          for (const unlisten of unlisteners) unlisten();
        };
      }
    });
    return () => {
      disposed = true;
      cleanup?.();
    };
  }, []);

  const box = region ? overlayBox(region, window.devicePixelRatio || 1) : null;
  return (
    <main className="fixed inset-0 overflow-hidden" aria-hidden="true">
      {region && box && (
        <div
          className="pointer-events-none absolute border-2 border-indigo-400"
          style={{ left: box.left, top: box.top, width: box.width, height: box.height }}
        >
          <span className="absolute -top-6 left-0 rounded bg-indigo-500 px-1.5 py-0.5 text-[10px] leading-4 font-medium text-white">
            {region.width} × {region.height}
          </span>
        </div>
      )}
    </main>
  );
}
