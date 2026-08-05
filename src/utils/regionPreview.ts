import type { ScreenRegion } from "../types";

export interface PreviewBox {
  left: number;
  top: number;
  width: number;
  height: number;
}

/**
 * Scales a screen region into a fixed-size preview box, preserving aspect
 * ratio and centering it.
 *
 * The main window has no monitor geometry from the backend, so the preview is
 * a proportional schematic (shape + coordinates) rather than an exact overlay.
 */
export function regionPreviewBox(
  region: Pick<ScreenRegion, "width" | "height">,
  maxWidth: number,
  maxHeight: number,
): PreviewBox {
  const safeWidth = Math.max(region.width, 1);
  const safeHeight = Math.max(region.height, 1);
  const scale = Math.min(maxWidth / safeWidth, maxHeight / safeHeight, 1);
  const width = Math.round(safeWidth * scale);
  const height = Math.round(safeHeight * scale);
  return {
    left: Math.round((maxWidth - width) / 2),
    top: Math.round((maxHeight - height) / 2),
    width,
    height,
  };
}
