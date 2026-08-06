/** Shared JSON contracts exchanged with vtrans-app over Tauri IPC. */

export type Mode = "single" | "live";
export type ProviderId = "api" | "local";
export type LanguageCode = "auto" | "zh-CN" | "ja" | "en";
export type PipelineStatus =
  | "idle"
  | "capturing"
  | "ocr_in_progress"
  | "translating"
  | "completed"
  | { error: string };

export interface ScreenRegion {
  monitor_id: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface OcrLine {
  text: string;
  confidence: number;
  polygon: [[number, number], [number, number], [number, number], [number, number]];
  reading_order: number;
}

export interface OcrResult {
  lines: OcrLine[];
  merged_text: string;
  detected_language: LanguageCode | null;
  elapsed_ms: number;
}

export interface TranslationResult {
  translated_text: string;
  provider_id: string;
  elapsed_ms: number;
}

export interface PipelineConfig {
  region: ScreenRegion;
  capture_interval_ms: number;
  difference_threshold: number;
}

export interface AppStatus {
  /** Backend session mode (`"single"` or `"live"`); drives hydration. */
  mode: Mode;
  pipeline_status: PipelineStatus;
  ocr_provider: string;
  translation_provider: string;
  selected_region: ScreenRegion | null;
  live_running: boolean;
  model_progress: number | null;
  debug_mode: boolean;
}

export interface DebugFramePayload {
  /** Base64-encoded JPEG thumbnail (longest edge ≤ 480 px). */
  image: string;
  region: ScreenRegion;
  frame_index: number;
  timestamp_ms: number;
}

export interface AppConfig {
  capture: CaptureConfig;
  ocr: OcrConfig;
  translation: TranslationConfig;
  result_window: ResultWindowConfig;
  floating_ball: FloatingBallConfig;
  hotkeys: HotkeyConfig;
  log_level: string;
  model_dir: string | null;
  version: number;
}

export interface CaptureConfig {
  interval_ms: number;
  difference_threshold: number;
}

export interface OcrConfig {
  language: LanguageCode;
  min_confidence: number;
}

export interface TranslationConfig {
  provider: ProviderId;
  source_language: LanguageCode;
  target_language: Exclude<LanguageCode, "auto">;
  timeout_seconds: number;
  api_endpoint: string;
  api_model: string;
  max_retries: number;
}

export interface ResultWindowConfig {
  always_on_top: boolean;
  /** Background opacity of the mini-bar result popup (0.3–1.0). */
  opacity: number;
  /** Translation text font size in pixels (12–24). */
  font_size_px: number;
}

export interface FloatingBallConfig {
  /** Whether the floating ball is visible after startup. */
  enabled: boolean;
}

export interface HotkeyConfig {
  select_and_translate: string;
  live_translate: string;
  stop_live: string;
}

export interface VerifyReport {
  checked: number;
  passed: number;
  failed: string[];
}

export interface PipelineErrorPayload {
  message: string;
  recoverable: boolean;
}

export interface ModelProgressPayload {
  model_id: string;
  progress: number;
}

export interface TimestampPayload {
  timestamp: number;
}

export interface ResultPayload<T> {
  result: T;
}

export interface StatusPayload {
  status: string;
}

export interface StoppedPayload {
  reason: string;
}

export type EventPayloadMap = {
  capture_status_changed: StatusPayload;
  ocr_started: TimestampPayload;
  ocr_completed: ResultPayload<OcrResult>;
  translation_started: TimestampPayload;
  translation_completed: ResultPayload<TranslationResult>;
  pipeline_error: PipelineErrorPayload;
  live_session_stopped: StoppedPayload;
  model_loading_progress: ModelProgressPayload;
  region_selected: ScreenRegion;
  overlay_region_updated: ScreenRegion;
  overlay_hidden: null;
  debug_frame_updated: DebugFramePayload;
};

export const DEFAULT_CONFIG: AppConfig = {
  capture: { interval_ms: 500, difference_threshold: 0.03 },
  ocr: { language: "auto", min_confidence: 0.55 },
  translation: {
    provider: "api",
    source_language: "auto",
    target_language: "zh-CN",
    timeout_seconds: 30,
    api_endpoint: "https://api.openai.com/v1/chat/completions",
    api_model: "gpt-4o-mini",
    max_retries: 3,
  },
  result_window: { always_on_top: true, opacity: 0.95, font_size_px: 14 },
  floating_ball: { enabled: false },
  hotkeys: {
    select_and_translate: "Alt+Shift+A",
    live_translate: "Alt+Shift+R",
    stop_live: "Alt+Shift+S",
  },
  log_level: "info",
  model_dir: null,
  version: 2,
};

/** Allowed range for the mini-bar background opacity. */
export const RESULT_OPACITY_MIN = 0.3;
/** Allowed range for the mini-bar background opacity. */
export const RESULT_OPACITY_MAX = 1.0;
/** Allowed range for the mini-bar translation font size. */
export const RESULT_FONT_SIZE_MIN = 12;
/** Allowed range for the mini-bar translation font size. */
export const RESULT_FONT_SIZE_MAX = 24;

export function isPipelineError(status: PipelineStatus): status is { error: string } {
  return typeof status === "object" && status !== null && "error" in status;
}

export function pipelineStatusLabel(status: PipelineStatus): string {
  if (isPipelineError(status)) return status.error;
  return (
    {
      idle: "就绪",
      capturing: "采集中",
      ocr_in_progress: "识别中",
      translating: "翻译中",
      completed: "已完成",
    } satisfies Record<Exclude<PipelineStatus, { error: string }>, string>
  )[status];
}

/**
 * Normalizes a backend translation provider identifier to the frontend
 * provider value domain.
 *
 * `vtrans-app` reports `AppStatus.translation_provider` using the provider
 * implementation id: `"api"` for the API provider and `"local-onnx"` for the
 * local ONNX provider. The frontend stores and persists the configuration
 * identifier `"api" | "local"` only, so the runtime id must be mapped back.
 * Unknown values fall back to `"api"` to keep the UI in a valid state.
 */
export function normalizeProviderId(raw: string): ProviderId {
  if (raw === "local-onnx") return "local";
  return "api";
}

/**
 * Reports whether the local ONNX translation model supports the configured
 * language pair.
 *
 * The bundled manifest (`opus-mt-en-zh-int8`) currently declares a single
 * `en -> zh-CN` pair and cannot auto-detect the source language. Any other
 * source/target combination (including `auto`) must be served by the API
 * provider; the UI surfaces this constraint so translation never fails
 * silently.
 */
export function isLocalPairSupported(
  config: Pick<AppConfig, "translation">,
): boolean {
  if (config.translation.provider !== "local") return true;
  return (
    config.translation.source_language === "en" &&
    config.translation.target_language === "zh-CN"
  );
}
