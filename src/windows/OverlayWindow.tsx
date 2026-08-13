import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import {
  onMultiBoxBoxAdded,
  onMultiBoxBoxRemoved,
  onMultiBoxBoxUpdated,
} from "../services/events";
import { listTranslationBoxes } from "../services/tauri";
import type { ScreenRegion } from "../types";

export interface OverlayBox {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** A translation box rendered on the overlay, keyed by box id and color. */
export interface OverlayMultiBox {
  box_id: number;
  color: string;
  region: ScreenRegion;
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
 * Inserts or replaces an overlay box, keeping entries unique by `box_id`.
 *
 * The `multibox://box-added` event can race with the mount-time list hydration
 * (both describe the same box), so updates are idempotent by id.
 */
export function upsertOverlayBox(
  list: OverlayMultiBox[],
  box: OverlayMultiBox,
): OverlayMultiBox[] {
  const index = list.findIndex((entry) => entry.box_id === box.box_id);
  if (index === -1) return [...list, box];
  return list.map((entry) => (entry.box_id === box.box_id ? box : entry));
}

/**
 * Persistent screen-level region marker.
 *
 * A borderless, transparent, always-on-top, click-through window draws the
 * currently selected capture region so the user always sees what part of the
 * screen is being translated. In multi-box mode it also draws a colored
 * border (with an ordinal label) for every configured translation box, so each
 * box's captured area stays visible at a glance. Only coordinates cross IPC;
 * the borders are pure CSS and the window never receives mouse input.
 */
export function OverlayWindow() {
  const [region, setRegion] = useState<ScreenRegion | null>(null);
  const [boxes, setBoxes] = useState<OverlayMultiBox[]>([]);

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
      onMultiBoxBoxAdded((payload) => {
        if (!disposed) {
          setBoxes((previous) =>
            upsertOverlayBox(previous, {
              box_id: payload.box_id,
              color: payload.color,
              region: payload.region,
            }),
          );
        }
      }),
      onMultiBoxBoxRemoved((payload) => {
        if (!disposed) {
          setBoxes((previous) => previous.filter((box) => box.box_id !== payload.box_id));
        }
      }),
      onMultiBoxBoxUpdated((payload) => {
        if (!disposed) {
          setBoxes((previous) =>
            previous.map((box) =>
              box.box_id === payload.box_id ? { ...box, region: payload.region } : box,
            ),
          );
        }
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

  // 水合已持久化的翻译框列表（多框会话重启后仍要显示彩色方框）。
  useEffect(() => {
    let active = true;
    void listTranslationBoxes()
      .then((list) => {
        if (!active) return;
        setBoxes(
          list.map((box) => ({ box_id: box.box_id, color: box.color, region: box.region })),
        );
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, []);

  const singleBox = region ? overlayBox(region, window.devicePixelRatio || 1) : null;
  const dpr = window.devicePixelRatio || 1;
  return (
    <main className="fixed inset-0 overflow-hidden" aria-hidden="true">
      {region && singleBox && (
        <div
          className="pointer-events-none absolute border-2 border-indigo-400"
          style={{ left: singleBox.left, top: singleBox.top, width: singleBox.width, height: singleBox.height }}
        >
          <span className="absolute -top-6 left-0 rounded bg-indigo-500 px-1.5 py-0.5 text-[10px] leading-4 font-medium text-white">
            {region.width} × {region.height}
          </span>
        </div>
      )}
      {boxes.map((box, index) => {
        const css = overlayBox(box.region, dpr);
        return (
          <div
            key={box.box_id}
            className="pointer-events-none absolute border-2"
            style={{
              left: css.left,
              top: css.top,
              width: css.width,
              height: css.height,
              borderColor: box.color,
            }}
            data-testid={`overlay-multibox-${box.box_id}`}
          >
            <span
              className="absolute -top-6 left-0 rounded px-1.5 py-0.5 text-[10px] leading-4 font-medium text-white"
              style={{ backgroundColor: box.color }}
            >
              框 {index + 1}
            </span>
          </div>
        );
      })}
    </main>
  );
}
