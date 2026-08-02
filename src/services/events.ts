import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { EventPayloadMap, OcrResult, PipelineConfig } from "../types";

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
  const entries = Object.entries(handlers) as Array<[
    keyof EventPayloadMap,
    (payload: EventPayloadMap[keyof EventPayloadMap]) => void,
  ]>;
  const unlisteners = await Promise.all(
    entries.map(([event, callback]) =>
      listen<EventPayloadMap[keyof EventPayloadMap]>(
        event,
        (eventPayload) => callback(eventPayload.payload),
      ),
    ),
  );
  return () => {
    for (const unlisten of unlisteners) unlisten();
  };
}

export const FRONTEND_OCR_RESULT = "frontend_ocr_result";
export const FRONTEND_LIVE_CONFIG = "frontend_live_config";
export const FRONTEND_LIVE_PAUSED = "frontend_live_paused";
export const FRONTEND_LIVE_STOPPED = "frontend_live_stopped";

/** Listen for a single-capture result shared between Tauri webviews. */
export function listenToFrontendOcrResult(callback: (result: OcrResult) => void): Promise<Unlisten> {
  return listen<OcrResult>(FRONTEND_OCR_RESULT, (eventPayload) => callback(eventPayload.payload));
}

/** Listen for live-session configuration shared between Tauri webviews. */
export function listenToFrontendLiveConfig(callback: (config: PipelineConfig) => void): Promise<Unlisten> {
  return listen<PipelineConfig>(FRONTEND_LIVE_CONFIG, (eventPayload) => callback(eventPayload.payload));
}

/** Listen for a frontend pause marker before the backend stop event arrives. */
export function listenToFrontendLivePaused(callback: () => void): Promise<Unlisten> {
  return listen(FRONTEND_LIVE_PAUSED, callback);
}

/** Listen for an explicit frontend live-session stop. */
export function listenToFrontendLiveStopped(callback: () => void): Promise<Unlisten> {
  return listen(FRONTEND_LIVE_STOPPED, callback);
}
