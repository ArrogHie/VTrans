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
 * Stroke width of the overlay border in CSS pixels.
 *
 * Must match the Tailwind `border-2` class used to paint the stroke: CSS
 * borders are drawn inside the element box, so this value is also the exact
 * amount each side of the box rectangle must be outset to keep every stroke
 * pixel outside the captured region.
 */
export const OVERLAY_BORDER_PX = 2;

/**
 * Border geometry for an overlay marker.
 *
 * Extends {@link OverlayBox} with the per-side outward offset that was
 * actually applied: each side of the region rectangle is outset by up to
 * {@link OVERLAY_BORDER_PX} so the `border-2` stroke (painted inside the
 * element box) covers only pixels outside the captured region. Sides that sit
 * within one stroke width of the viewport edge have their outset reduced so
 * the stroke stays inside the overlay window and remains visible.
 */
export interface OverlayBorderBox extends OverlayBox {
  /** Outward offset applied on the left side, in CSS pixels (0..OVERLAY_BORDER_PX). */
  insetLeft: number;
  /** Outward offset applied on the top side, in CSS pixels (0..OVERLAY_BORDER_PX). */
  insetTop: number;
  /** Outward offset applied on the right side, in CSS pixels (0..OVERLAY_BORDER_PX). */
  insetRight: number;
  /** Outward offset applied on the bottom side, in CSS pixels (0..OVERLAY_BORDER_PX). */
  insetBottom: number;
}

/** Clamps an outset offset to the stroke width, never negative. */
function clampOutset(space: number): number {
  return Math.min(OVERLAY_BORDER_PX, Math.max(0, space));
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
 * Outsets a region box by the overlay border width so the `border-2` stroke
 * is drawn entirely outside the captured region, clamped to the viewport.
 *
 * The CSS border is painted inside the element rectangle, so an element
 * matching the region rectangle exactly puts the whole 2px stroke inside the
 * captured area — and WGC captures the monitor with no window exclusion, so
 * those stroke pixels would be OCR'd and translated on every frame. Outsetting
 * each side by the full stroke width makes the stroke's inner edge coincide
 * with the region's outer edge: no stroke pixel falls inside the capture area.
 *
 * Each side is clamped independently to the viewport (which equals the
 * monitor for the overlay window): when the region sits within
 * {@link OVERLAY_BORDER_PX} of an edge, the outset is reduced by the missing
 * space. At an exactly flush edge the offset becomes 0 and the 2px stroke is
 * painted inside the region — a visibility-first tradeoff, because there is
 * no room outside the window: the border stays visible at the cost of a
 * ≤2px strip re-entering the capture area on that side only.
 */
export function overlayBorderBox(
  box: OverlayBox,
  viewport: { width: number; height: number },
): OverlayBorderBox {
  const insetLeft = clampOutset(box.left);
  const insetTop = clampOutset(box.top);
  const insetRight = clampOutset(viewport.width - (box.left + box.width));
  const insetBottom = clampOutset(viewport.height - (box.top + box.height));
  return {
    left: box.left - insetLeft,
    top: box.top - insetTop,
    width: box.width + insetLeft + insetRight,
    height: box.height + insetTop + insetBottom,
    insetLeft,
    insetTop,
    insetRight,
    insetBottom,
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
 *
 * The border element is the region rectangle outset by
 * {@link overlayBorderBox}, so the 2px stroke paints only outside the
 * captured area and never pollutes the monitor-level capture; single live
 * mode and multi-box mode share the same {@link OverlayFrame} rendering path.
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
  // The overlay webview viewport equals the monitor in CSS pixels; unknown
  // sizes (never in the browser) disable clamping rather than collapsing the
  // whole border onto the region.
  const viewport = {
    width: window.innerWidth || Number.POSITIVE_INFINITY,
    height: window.innerHeight || Number.POSITIVE_INFINITY,
  };
  const singleBorder = singleBox ? overlayBorderBox(singleBox, viewport) : null;
  return (
    <main className="fixed inset-0 overflow-hidden" aria-hidden="true">
      {region && singleBorder && (
        <OverlayFrame
          geometry={singleBorder}
          color="#818cf8"
          labelColor="#6366f1"
          label={`${region.width} × ${region.height}`}
        />
      )}
      {boxes.map((box, index) => (
        <OverlayFrame
          key={box.box_id}
          geometry={overlayBorderBox(overlayBox(box.region, dpr), viewport)}
          color={box.color}
          labelColor={box.color}
          label={`框 ${index + 1}`}
          testId={`overlay-multibox-${box.box_id}`}
        />
      ))}
    </main>
  );
}

interface OverlayFrameProps {
  geometry: OverlayBorderBox;
  /** Border color, identical for single live mode and multi-box mode. */
  color: string;
  /** Size label background color. */
  labelColor: string;
  label: string;
  testId?: string;
}

/**
 * One bordered region marker: the outset rectangle plus its size label.
 *
 * The element rectangle is the region outset by the stroke width
 * ({@link overlayBorderBox}), so the `border-2` stroke only paints outside
 * the captured region. The size label hangs 24px above the element top
 * (`-top-6`) with a 20px height, so its bottom edge ends 4px above the
 * element and never overlaps the top stroke band.
 */
export function OverlayFrame({ geometry, color, labelColor, label, testId }: OverlayFrameProps) {
  return (
    <div
      className="pointer-events-none absolute border-2"
      style={{
        left: geometry.left,
        top: geometry.top,
        width: geometry.width,
        height: geometry.height,
        borderColor: color,
      }}
      data-testid={testId}
    >
      <span
        className="absolute -top-6 left-0 rounded px-1.5 py-0.5 text-[10px] leading-4 font-medium text-white"
        style={{ backgroundColor: labelColor }}
      >
        {label}
      </span>
    </div>
  );
}
