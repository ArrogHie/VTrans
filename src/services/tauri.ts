import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import type {
  AppConfig,
  AppStatus,
  OcrResult,
  PipelineConfig,
  ScreenRegion,
  VerifyReport,
} from "../types";

/** A frontend-safe representation of an IPC failure. */
export function getIpcErrorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  if (typeof error === "object" && error !== null && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string") return message;
  }
  return "操作失败，请稍后重试";
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    console.warn(`[vtrans] IPC command failed: ${command}: ${getIpcErrorMessage(error)}`);
    throw error;
  }
}

/** Opens the selector window and waits until the user confirms a region. */
export function startRegionSelection(): Promise<ScreenRegion> {
  return call<ScreenRegion>("start_region_selection");
}

/** Cancels the pending selector request. */
export function cancelRegionSelection(): Promise<void> {
  return call<void>("cancel_region_selection");
}

/** Runs a single capture/OCR/translation pass for a region. */
export function captureOnce(region: ScreenRegion): Promise<OcrResult> {
  return call<OcrResult>("capture_once", { region });
}

/** Starts the backend live translation task. */
export function startLiveTranslation(config: PipelineConfig): Promise<void> {
  return call<void>("start_live_translation", { config });
}

/** Stops the backend live translation task. */
export function stopLiveTranslation(): Promise<void> {
  return call<void>("stop_live_translation");
}

/** Updates the active region or completes a pending region selection. */
export function updateLiveRegion(region: ScreenRegion): Promise<void> {
  return call<void>("update_live_region", { region });
}

/** Persists the selected OCR language. */
export function setOcrLanguage(language: AppConfig["ocr"]["language"]): Promise<void> {
  return call<void>("set_ocr_language", { language });
}

/** Persists the selected translation provider. */
export function setTranslationProvider(providerId: AppConfig["translation"]["provider"]): Promise<void> {
  return call<void>("set_translation_provider", { provider_id: providerId });
}

/** Verifies installed local model files. */
export function loadLocalModels(): Promise<VerifyReport> {
  return call<VerifyReport>("load_local_models");
}

/** Persists the complete application settings object. */
export function saveSettings(settings: AppConfig): Promise<void> {
  return call<void>("save_settings", { settings });
}

/** Returns a frontend-safe application status snapshot. */
export function getAppStatus(): Promise<AppStatus> {
  return call<AppStatus>("get_app_status");
}

/** Publishes a single-capture result to the other Tauri webviews. */
export function publishFrontendOcrResult(result: OcrResult): Promise<void> {
  return emit("frontend_ocr_result", result);
}

/** Publishes live-session configuration to the other Tauri webviews. */
export function publishFrontendLiveConfig(config: PipelineConfig): Promise<void> {
  return emit("frontend_live_config", config);
}

/** Marks an intentional pause before stopping the backend live task. */
export function publishFrontendLivePaused(): Promise<void> {
  return emit("frontend_live_paused");
}

/** Marks an intentional stop after the backend live task stops. */
export function publishFrontendLiveStopped(): Promise<void> {
  return emit("frontend_live_stopped");
}

/** Shows and focuses the preconfigured result webview. */
export async function showResultWindow(): Promise<void> {
  const window = await WebviewWindow.getByLabel("result");
  if (!window) {
    console.warn("[vtrans] result window is not configured");
    return;
  }
  await window.show();
  await window.setFocus();
}

/**
 * Normalizes a selection rectangle to physical pixels.
 *
 * The selector webview reports logical CSS pixels. Tauri's capture contract
 * uses physical pixels, so the conversion is kept in one tested helper.
 */
export function toPhysicalRegion(
  monitorId: string,
  start: { x: number; y: number },
  end: { x: number; y: number },
  devicePixelRatio = window.devicePixelRatio || 1,
): ScreenRegion | null {
  const left = Math.min(start.x, end.x);
  const top = Math.min(start.y, end.y);
  const width = Math.abs(end.x - start.x);
  const height = Math.abs(end.y - start.y);
  const physicalWidth = Math.round(width * devicePixelRatio);
  const physicalHeight = Math.round(height * devicePixelRatio);
  if (physicalWidth === 0 || physicalHeight === 0) return null;
  return {
    monitor_id: monitorId,
    x: Math.round(left * devicePixelRatio),
    y: Math.round(top * devicePixelRatio),
    width: physicalWidth,
    height: physicalHeight,
  };
}
