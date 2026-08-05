import type { DebugFramePayload } from "../types";
import { listenToEvent, type Unlisten } from "./events";

/**
 * Debug-only event that streams capture thumbnails while Debug mode is on.
 *
 * The backend only emits this event while Debug mode is enabled, so no
 * subscription is ever installed in normal production behavior.
 */
export const DEBUG_FRAME_UPDATED_EVENT = "debug_frame_updated" as const;

/**
 * Latest-value frame accumulator.
 *
 * Every push overwrites the previous value; the store never grows. This is
 * the unit that guarantees "keep only the newest frame" regardless of how
 * many events arrive between renders.
 */
export interface LatestFrameStore<T> {
  /** Replaces the stored value. */
  push(value: T): void;
  /** Returns the newest pushed value, or `null` when empty. */
  read(): T | null;
  /** Drops the stored value, releasing the cached frame. */
  clear(): void;
}

/** Creates an empty latest-value store. */
export function createLatestFrameStore<T>(): LatestFrameStore<T> {
  let latest: T | null = null;
  return {
    push(value: T) {
      latest = value;
    },
    read() {
      return latest;
    },
    clear() {
      latest = null;
    },
  };
}

/**
 * Subscribes to `debug_frame_updated` and forwards every payload.
 *
 * Returns a cleanup function that removes the listener. Callers are
 * responsible for latest-value semantics (see {@link createLatestFrameStore}).
 */
export function subscribeToDebugFrames(
  onFrame: (frame: DebugFramePayload) => void,
): Promise<Unlisten> {
  return listenToEvent(DEBUG_FRAME_UPDATED_EVENT, onFrame);
}
