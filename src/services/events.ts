import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { EventPayloadMap } from "../types";

export type Unlisten = UnlistenFn;

/** Listen to a typed backend event. */
export function listenToEvent<K extends keyof EventPayloadMap>(
  event: K,
  callback: (payload: EventPayloadMap[K]) => void,
): Promise<Unlisten> {
  return listen<EventPayloadMap[K]>(event, (eventPayload) => callback(eventPayload.payload));
}

/** Register all pipeline events and return one cleanup function. */
export async function subscribeToBackendEvents(
  handlers: Partial<{
    [K in keyof EventPayloadMap]: (payload: EventPayloadMap[K]) => void;
  }>,
): Promise<Unlisten> {
  const entries = Object.entries(handlers) as [keyof EventPayloadMap, (payload: any) => void][];
  const unlisteners = await Promise.all(
    entries.map(([event, callback]) => listenToEvent(event, callback)),
  );
  return () => {
    for (const unlisten of unlisteners) unlisten();
  };
}


export const FRONTEND_OCR_RESULT = "frontend_ocr_result";

export function listenToFrontendOcrResult(callback: (result: import("../types").OcrResult) => void): Promise<Unlisten> {
  return listen<import("../types").OcrResult>(FRONTEND_OCR_RESULT, (eventPayload) => callback(eventPayload.payload));
}
