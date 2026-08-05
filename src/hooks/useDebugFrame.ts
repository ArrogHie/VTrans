import { useEffect, useRef, useState } from "react";
import type { DebugFramePayload } from "../types";
import {
  createLatestFrameStore,
  subscribeToDebugFrames,
  type LatestFrameStore,
} from "../services/debugFrames";
import type { Unlisten } from "../services/events";

/**
 * Subscribes to debug capture frames while `enabled` is true and returns
 * only the latest frame.
 *
 * Frames that arrive faster than the renderer can flush are coalesced into a
 * single state update per animation frame, always carrying the newest
 * payload. The subscription is removed and the cached frame is dropped as
 * soon as the hook is disabled or the owning component unmounts, so the
 * debug panel never accumulates frames and never retains them after exit.
 */
export function useDebugFrame(enabled: boolean): DebugFramePayload | null {
  const storeRef = useRef<LatestFrameStore<DebugFramePayload> | null>(null);
  if (storeRef.current === null) {
    storeRef.current = createLatestFrameStore<DebugFramePayload>();
  }
  const store = storeRef.current;
  const [frame, setFrame] = useState<DebugFramePayload | null>(null);

  useEffect(() => {
    if (!enabled) {
      // Debug mode is off: release the cached frame immediately.
      store.clear();
      setFrame(null);
      return;
    }

    let disposed = false;
    let unlisten: Unlisten | undefined;
    let scheduled: number | undefined;

    const flush = () => {
      scheduled = undefined;
      if (disposed) return;
      setFrame(store.read());
    };
    const scheduleFlush = () => {
      if (scheduled !== undefined) return;
      scheduled = window.requestAnimationFrame(flush);
    };

    void subscribeToDebugFrames((payload) => {
      if (disposed) return;
      store.push(payload);
      scheduleFlush();
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    });

    return () => {
      disposed = true;
      if (scheduled !== undefined) window.cancelAnimationFrame(scheduled);
      unlisten?.();
      store.clear();
    };
  }, [enabled, store]);

  return frame;
}
