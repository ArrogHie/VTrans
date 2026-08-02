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
  pipeline_status: PipelineStatus;
  ocr_provider: string;
  translation_provider: string;
  selected_region: ScreenRegion | null;
  live_running: boolean;
  model_progress: number | null;
}

export interface AppConfig {
  capture: CaptureConfig;
  ocr: OcrConfig;
  translation: TranslationConfig;
  result_window: ResultWindowConfig;
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
}

export interface HotkeyConfig {
  select_and_translate: string;
  live_translate: string;
  stop_live: string;
}

export interface VerifyReport {
  valid: boolean;
  checked_files?: number;
  missing_files?: string[];
  invalid_files?: string[];
  [key: string]: unknown;
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
  region_selected: ResultPayload<ScreenRegion>;
};

export const DEFAULT_CONFIG: AppConfig = {
  capture: { interval_ms: 500, difference_threshold: 0.03 },
  ocr: { language: "auto", min_confidence: 0.55 },
  translation: {
    provider: "api",
    source_language: "auto",
    target_language: "zh-CN",
    timeout_seconds: 30,
    api_endpoint: "",
    api_model: "",
    max_retries: 2,
  },
  result_window: { always_on_top: true },
  hotkeys: {
    select_and_translate: "Alt+Shift+A",
    live_translate: "Alt+Shift+L",
    stop_live: "Alt+Shift+S",
  },
  log_level: "info",
  model_dir: null,
  version: 1,
};

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
