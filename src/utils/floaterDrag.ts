/** Drag-vs-click distance threshold in CSS pixels. */
export const FLOATER_DRAG_THRESHOLD_PX = 4;

/** A mouse position in CSS pixels. */
export interface FloaterDragPoint {
  x: number;
  y: number;
}

/** Minimal mouse-event surface consumed by the drag discriminator. */
export interface FloaterDragMouseEvent {
  button?: number;
  clientX: number;
  clientY: number;
}

/** Mouse handlers wired onto the floating ball button. */
export interface FloaterDragHandlers {
  onMouseDown(event: FloaterDragMouseEvent): void;
  onMouseMove(event: FloaterDragMouseEvent): void;
  onMouseUp(): void;
  onClick(): void;
}

/** Callbacks backing the drag-vs-click decision. */
export interface FloaterDragOptions {
  /** Distance in CSS pixels that separates a click from a drag. */
  threshold?: number;
  /** Starts a native window drag; called once per drag gesture. */
  startDragging(): void;
  /** Toggles the menu; called only for gestures below the threshold. */
  onToggle(): void;
}

/**
 * Creates drag-vs-click discriminators for the floating ball button.
 *
 * The button must be draggable and clickable at the same time. Tauri's
 * `data-tauri-drag-region="deep"` attribute would turn every press into a
 * window drag, swallowing the click, so the decision is made manually:
 * `mousedown` records the press origin, `mousemove` starts a native drag
 * only after the pointer travels past the threshold, and `click` toggles
 * the menu only when no drag happened.
 *
 * State is captured in the returned handler object; creating a fresh one per
 * render keeps the `open` snapshot in the toggle closure without effects.
 */
export function createFloaterDragHandlers({
  threshold = FLOATER_DRAG_THRESHOLD_PX,
  startDragging,
  onToggle,
}: FloaterDragOptions): FloaterDragHandlers {
  let pressStart: FloaterDragPoint | null = null;
  let dragging = false;

  return {
    onMouseDown(event) {
      if (event.button !== 0) return;
      pressStart = { x: event.clientX, y: event.clientY };
      dragging = false;
    },
    onMouseMove(event) {
      if (dragging || !pressStart) return;
      const dx = event.clientX - pressStart.x;
      const dy = event.clientY - pressStart.y;
      if (Math.hypot(dx, dy) >= threshold) {
        dragging = true;
        pressStart = null;
        startDragging();
      }
    },
    onMouseUp() {
      pressStart = null;
    },
    onClick() {
      if (dragging) {
        dragging = false;
        return;
      }
      onToggle();
    },
  };
}
