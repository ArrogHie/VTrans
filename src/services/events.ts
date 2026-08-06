import { emit, listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  EventPayloadMap,
  OcrResult,
  PipelineConfig,
  TranslationResult,
} from "../types";

export type Unlisten = UnlistenFn;

/** Listen to a typed backend event. */
export function listenToEvent<K extends keyof EventPayloadMap>(
  event: K,
  callback: (payload: EventPayloadMap[K]) => void,
): Promise<Unlisten> {
  return listen<EventPayloadMap[K]>(event, (eventPayload) => callback(eventPayload.payload));
}

/** Listen for a completed OCR result and receive the unwrapped result. */
export function onOcrCompleted(callback: (result: OcrResult) => void): Promise<Unlisten> {
  return listenToEvent("ocr_completed", ({ result }) => callback(result));
}

/** Listen for a completed translation and receive the unwrapped result. */
export function onTranslationCompleted(
  callback: (result: TranslationResult) => void,
): Promise<Unlisten> {
  return listenToEvent("translation_completed", ({ result }) => callback(result));
}

/** Listen for a pipeline error and receive the human-readable message. */
export function onPipelineError(callback: (message: string) => void): Promise<Unlisten> {
  return listenToEvent("pipeline_error", ({ message }) => callback(message));
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
export const FRONTEND_FLOATER_ENABLED = "frontend_floater_enabled";

/** Listen for a single-capture result shared between Tauri webviews. */
export function listenToFrontendOcrResult(callback: (result: OcrResult) => void): Promise<Unlisten> {
  return listen<OcrResult>(FRONTEND_OCR_RESULT, (eventPayload) => callback(eventPayload.payload));
}

/** Listen for live-session configuration shared between Tauri webviews. */
export function listenToFrontendLiveConfig(callback: (config: PipelineConfig) => void): Promise<Unlisten> {
  return listen<PipelineConfig>(FRONTEND_LIVE_CONFIG, (eventPayload) => callback(eventPayload.payload));
}

/** Listen for a frontend pause marker published after the backend live task stopped. */
export function listenToFrontendLivePaused(callback: () => void): Promise<Unlisten> {
  return listen(FRONTEND_LIVE_PAUSED, callback);
}

/** Listen for an explicit frontend live-session stop. */
export function listenToFrontendLiveStopped(callback: () => void): Promise<Unlisten> {
  return listen(FRONTEND_LIVE_STOPPED, callback);
}

/** Payload of the internal floating-ball visibility event. */
export interface FloaterEnabledPayload {
  enabled: boolean;
}

/**
 * Tells every webview whether the floating ball should be visible.
 *
 * This is a frontend-only event: the main window setting panel publishes it
 * when the switch changes, and the floater webview listens for it to show or
 * hide itself immediately. It never crosses the Rust boundary and carries no
 * sensitive data.
 */
export function publishFrontendFloaterEnabled(enabled: boolean): Promise<void> {
  return emit<FloaterEnabledPayload>(FRONTEND_FLOATER_ENABLED, { enabled });
}

/** Listens for floating-ball visibility changes from other webviews. */
export function listenToFrontendFloaterEnabled(
  callback: (payload: FloaterEnabledPayload) => void,
): Promise<Unlisten> {
  return listen<FloaterEnabledPayload>(FRONTEND_FLOATER_ENABLED, (eventPayload) =>
    callback(eventPayload.payload),
  );
}
