import { emit, listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  BoxAddedPayload,
  BoxRemovedPayload,
  BoxStatusPayload,
  BoxUpdatedPayload,
  BoxedTranslationResult,
  EventPayloadMap,
  ModelDownloadProgress,
  OcrResult,
  PipelineConfig,
  SingleResultPayload,
  TranslationResult,
  WarningPayload,
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

/** Stable name of the backend translation-model download progress event. */
export const MODEL_DOWNLOAD_PROGRESS = "model_download_progress";

/**
 * Listens for translation model download progress.
 *
 * Payload fields are snake_case (`bytes` / `total` / `fraction`) to match the
 * Rust DTO. The backend throttles emissions during the download and stops
 * after it settles, so callers treat progress events as the download-in-flight
 * signal.
 */
export function onModelDownloadProgress(
  callback: (progress: ModelDownloadProgress) => void,
): Promise<Unlisten> {
  return listen<ModelDownloadProgress>(MODEL_DOWNLOAD_PROGRESS, (eventPayload) =>
    callback(eventPayload.payload),
  );
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
export const FRONTEND_MULTIBOX_STARTED = "frontend_multibox_started";
export const FRONTEND_MULTIBOX_STOPPED = "frontend_multibox_stopped";

/** Stable multi-box event names emitted by `vtrans-app`. */
export const MULTIBOX_RESULT = "multibox://result";
export const MULTIBOX_BOX_ADDED = "multibox://box-added";
export const MULTIBOX_BOX_REMOVED = "multibox://box-removed";
export const MULTIBOX_BOX_UPDATED = "multibox://box-updated";
export const MULTIBOX_STATUS = "multibox://status";
export const MULTIBOX_WARNING = "multibox://warning";
export const TRANSLATION_SINGLE_RESULT = "translation://single-result";

/** Listen for a multi-box translation result tagged with its box id/color. */
export function onMultiBoxResult(
  callback: (result: BoxedTranslationResult) => void,
): Promise<Unlisten> {
  return listen<BoxedTranslationResult>(MULTIBOX_RESULT, (eventPayload) =>
    callback(eventPayload.payload),
  );
}

/** Listen for a translation box being added. */
export function onMultiBoxBoxAdded(
  callback: (payload: BoxAddedPayload) => void,
): Promise<Unlisten> {
  return listen<BoxAddedPayload>(MULTIBOX_BOX_ADDED, (eventPayload) =>
    callback(eventPayload.payload),
  );
}

/** Listen for a translation box being removed. */
export function onMultiBoxBoxRemoved(
  callback: (payload: BoxRemovedPayload) => void,
): Promise<Unlisten> {
  return listen<BoxRemovedPayload>(MULTIBOX_BOX_REMOVED, (eventPayload) =>
    callback(eventPayload.payload),
  );
}

/** Listen for a translation box region being updated. */
export function onMultiBoxBoxUpdated(
  callback: (payload: BoxUpdatedPayload) => void,
): Promise<Unlisten> {
  return listen<BoxUpdatedPayload>(MULTIBOX_BOX_UPDATED, (eventPayload) =>
    callback(eventPayload.payload),
  );
}

/** Listen for a translation box runtime status change. */
export function onMultiBoxStatus(
  callback: (payload: BoxStatusPayload) => void,
): Promise<Unlisten> {
  return listen<BoxStatusPayload>(MULTIBOX_STATUS, (eventPayload) =>
    callback(eventPayload.payload),
  );
}

/** Listen for the box-count performance warning. */
export function onMultiBoxWarning(
  callback: (payload: WarningPayload) => void,
): Promise<Unlisten> {
  return listen<WarningPayload>(MULTIBOX_WARNING, (eventPayload) =>
    callback(eventPayload.payload),
  );
}

/** Listen for a single-capture translation result (original + translated). */
export function onSingleTranslationResult(
  callback: (payload: SingleResultPayload) => void,
): Promise<Unlisten> {
  return listen<SingleResultPayload>(TRANSLATION_SINGLE_RESULT, (eventPayload) =>
    callback(eventPayload.payload),
  );
}

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
 * Payload of the frontend multi-box session events.
 *
 * Both `frontend_multibox_started` and `frontend_multibox_stopped` carry the
 * ids of the boxes that belong to the session. Receivers mirror the payload
 * into their own `boxStatuses` so every webview derives the same running
 * state without querying the backend.
 */
export interface FrontendMultiBoxSessionPayload {
  box_ids: number[];
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

/**
 * Listens for a frontend multi-box session start.
 *
 * Published by `multiBoxActions.startMultiBox` after `start_multi_realtime`
 * succeeds; every window marks the payload's boxes as running so the floating
 * ball and the main window derive the same session state.
 */
export function listenToFrontendMultiBoxStarted(
  callback: (payload: FrontendMultiBoxSessionPayload) => void,
): Promise<Unlisten> {
  return listen<FrontendMultiBoxSessionPayload>(FRONTEND_MULTIBOX_STARTED, (eventPayload) =>
    callback(eventPayload.payload),
  );
}

/**
 * Listens for a frontend multi-box session stop.
 *
 * Published by `multiBoxActions.stopMultiBox` after `stop_multi_realtime`
 * succeeds; every window marks the payload's boxes as stopped.
 */
export function listenToFrontendMultiBoxStopped(
  callback: (payload: FrontendMultiBoxSessionPayload) => void,
): Promise<Unlisten> {
  return listen<FrontendMultiBoxSessionPayload>(FRONTEND_MULTIBOX_STOPPED, (eventPayload) =>
    callback(eventPayload.payload),
  );
}
