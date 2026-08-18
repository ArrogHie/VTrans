/** Shared JSON contracts exchanged with vtrans-app over Tauri IPC. */

export type Mode = "single" | "live";
export type ProviderId = "openai" | "deepl" | "google" | "azure" | "baidu" | "local";
export type TranslationQuality = "fast" | "balanced";
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

/** A translation box as returned by the multi-box IPC commands. */
export interface TranslationBoxInfo {
  box_id: number;
  region: ScreenRegion;
  color: string;
}

/**
 * Runtime status of a single translation box.
 *
 * Mirrors `vtrans_pipeline::BoxStatus`'s serde representation: the unit
 * variants serialize as plain strings (`"Running"` / `"Stopped"`) and the
 * `Error` newtype variant serializes as `{ "Error": message }`.
 */
export type BoxStatus = "Running" | "Stopped" | { Error: string };

/** A multi-box translation result tagged with its originating box. */
export interface BoxedTranslationResult {
  box_id: number;
  color: string;
  /**
   * OCR-recognized source text for this box (same text sent to translation).
   * Empty when OCR failed or produced no text; the UI omits the original
   * area entirely in that case.
   */
  original_text: string;
  result: TranslationResult;
  timestamp: number;
}

/** Payload of the `translation://single-result` event. */
export interface SingleResultPayload {
  original_text: string;
  translated_text: string;
  timestamp: number;
}

/** Payload of `multibox://box-added`. */
export interface BoxAddedPayload {
  box_id: number;
  color: string;
  region: ScreenRegion;
}

/** Payload of `multibox://box-removed`. */
export interface BoxRemovedPayload {
  box_id: number;
}

/** Payload of `multibox://box-updated`. */
export interface BoxUpdatedPayload {
  box_id: number;
  region: ScreenRegion;
}

/** Payload of `multibox://status`. */
export interface BoxStatusPayload {
  box_id: number;
  status: BoxStatus;
}

/** Payload of `multibox://warning`. */
export interface WarningPayload {
  current_count: number;
  max_count: number;
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
  /** Persisted multi-box translation entries (field name is `id`, per config schema). */
  translation_boxes: TranslationBoxConfigEntry[];
  /** Maximum number of concurrent translation boxes. */
  max_boxes: number;
  /** Active-box count at which the UI should warn (0 disables the warning). */
  warning_threshold: number;
  version: number;
}

/** A translation box entry as stored inside `AppConfig.translation_boxes`. */
export interface TranslationBoxConfigEntry {
  id: number;
  region: ScreenRegion;
  color: string;
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
  /**
   * Azure Translator region (e.g. `"eastasia"`); only used by the `azure`
   * provider. `null` omits the region header.
   */
  region: string | null;
  /**
   * Baidu Translate APP ID; only used by the `baidu` provider and required
   * (non-empty) when that provider is selected. Not sensitive: the matching
   * Secret lives in the OS credential store.
   */
  app_id: string | null;
  /** Translation quality preset consumed by the local provider. */
  quality: TranslationQuality;
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
  /** Background opacity of the floating ball (0.3–1.0). */
  opacity: number;
  /** Diameter of the floating ball in pixels (32–72). */
  size_px: number;
}

export interface HotkeyConfig {
  select_and_translate: string;
  live_translate: string;
  stop_live: string;
}

export interface VerifyReport {
  checked: number;
  passed: number;
  /**
   * Entries skipped because they are optional and missing on disk (e.g. the
   * not-yet-downloaded translation model). Skipped entries do not count as
   * failures but must not be reported as "verified" either.
   */
  skipped: string[];
  failed: string[];
}

/**
 * User-facing verdict for a `load_local_models` report.
 *
 * Failed entries take precedence. A report with no failures but skipped
 * optional entries (translation model not installed) must not claim the whole
 * local model set passed.
 */
export function verifyReportMessage(report: VerifyReport): string {
  if (report.failed.length > 0) return "本地模型需要检查";
  if (report.skipped.length > 0) return "OCR 模型校验通过，翻译模型未安装（请在设置中下载）";
  return "本地模型校验通过";
}

export interface PipelineErrorPayload {
  message: string;
  recoverable: boolean;
}

export interface ModelProgressPayload {
  model_id: string;
  progress: number;
}

/**
 * Runtime state of one model entry as reported by `get_model_status` /
 * `retry_model_setup` (`vtrans-app` 侧 `ModelState` 的 serde 表示）。
 *
 * `ready` = 存在且 sha256 通过；`missing` = 缺失；`invalid` = 存在但校验失败。
 * optional 条目缺失时报告 `missing`（前端显示「未安装」而非「校验失败」）。
 */
export type ModelState = "ready" | "missing" | "invalid";

/** One manifest model entry inside `ModelStatusReport`. */
export interface ModelEntryStatus {
  id: string;
  state: ModelState;
  optional: boolean;
}

/**
 * Read-only model file snapshot returned by `get_model_status` /
 * `retry_model_setup`（字段与 Rust DTO 一一对应，snake_case）。
 */
export interface ModelStatusReport {
  entries: ModelEntryStatus[];
  ocr_ready: boolean;
  translation_ready: boolean;
}

/** Payload of the `model_download_progress` event (snake_case fields). */
export interface ModelDownloadProgress {
  bytes: number;
  total: number;
  fraction: number;
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
  model_download_progress: ModelDownloadProgress;
  region_selected: ScreenRegion;
  overlay_region_updated: ScreenRegion;
  overlay_hidden: null;
  debug_frame_updated: DebugFramePayload;
};

export const DEFAULT_CONFIG: AppConfig = {
  capture: { interval_ms: 500, difference_threshold: 0.03 },
  ocr: { language: "auto", min_confidence: 0.55 },
  translation: {
    provider: "openai",
    region: null,
    app_id: null,
    quality: "fast",
    source_language: "auto",
    target_language: "zh-CN",
    timeout_seconds: 30,
    api_endpoint: "https://api.openai.com/v1/chat/completions",
    api_model: "gpt-4o-mini",
    max_retries: 3,
  },
  result_window: { always_on_top: true, opacity: 0.95, font_size_px: 14 },
  floating_ball: { enabled: false, opacity: 1, size_px: 48 },
  hotkeys: {
    select_and_translate: "Alt+Shift+A",
    live_translate: "Alt+Shift+R",
    stop_live: "Alt+Shift+S",
  },
  log_level: "info",
  model_dir: null,
  translation_boxes: [],
  max_boxes: 8,
  warning_threshold: 4,
  version: 6,
};

/** Allowed range for the mini-bar background opacity. */
export const RESULT_OPACITY_MIN = 0.3;
/** Allowed range for the mini-bar background opacity. */
export const RESULT_OPACITY_MAX = 1.0;
/** Allowed range for the mini-bar translation font size. */
export const RESULT_FONT_SIZE_MIN = 12;
/** Allowed range for the mini-bar translation font size. */
export const RESULT_FONT_SIZE_MAX = 24;
/** Allowed range for the floating ball background opacity. */
export const FLOATER_OPACITY_MIN = 0.3;
/** Allowed range for the floating ball background opacity. */
export const FLOATER_OPACITY_MAX = 1.0;
/** Allowed range for the floating ball diameter in pixels. */
export const FLOATER_SIZE_MIN = 32;
/** Allowed range for the floating ball diameter in pixels. */
export const FLOATER_SIZE_MAX = 72;

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

/** Cloud translation provider identifiers (excludes the local provider). */
export const CLOUD_PROVIDER_IDS: readonly ProviderId[] = [
  "openai",
  "deepl",
  "google",
  "azure",
  "baidu",
];

/** Reports whether the provider talks to a remote HTTP(S) API. */
export function isCloudProvider(provider: ProviderId): boolean {
  return provider !== "local";
}

/**
 * Normalizes a backend translation provider identifier to the frontend
 * provider value domain.
 *
 * `vtrans-app` reports `AppStatus.translation_provider` using the runtime
 * implementation id: cloud providers use their configuration id
 * (`"openai"` / `"deepl"` / `"google"` / `"azure"` / `"baidu"`), while the
 * local ONNX provider reports `"local-onnx"` for the configuration value
 * `"local"`. The frontend maps `"local-onnx"` back to `"local"` and passes
 * the cloud ids through unchanged. Unknown values fall back to `"openai"`
 * (the default provider) to keep the UI in a valid state.
 */
export function normalizeProviderId(raw: string): ProviderId {
  if (raw === "local-onnx") return "local";
  if ((CLOUD_PROVIDER_IDS as readonly string[]).includes(raw)) return raw as ProviderId;
  return "openai";
}

/**
 * Reports whether the local ONNX translation model supports the configured
 * language pair.
 *
 * The bundled manifest (`opus-mt-en-zh-int8`) currently declares a single
 * `en -> zh-CN` pair and cannot auto-detect the source language. Any other
 * source/target combination (including `auto`) must be served by a cloud
 * provider; the UI surfaces this constraint so translation never fails
 * silently. Cloud providers always pass through.
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

/** Reports whether a box status is the serialized `Error` variant. */
export function isBoxError(status: BoxStatus): status is { Error: string } {
  return typeof status === "object" && status !== null && "Error" in status;
}

/**
 * Manifest id of the downloadable local translation model entry.
 *
 * Matches `translation.model.id` in `src-tauri/resources/models/manifest.json`
 * (owned by vtrans-app)。The optional-entry fallback below keeps the card
 * working if the id ever changes.
 */
export const TRANSLATION_MODEL_ENTRY_ID = "opus-mt-en-zh-int8";

/**
 * Locates the downloadable translation model entry inside a status report.
 *
 * Prefers the exact manifest id, then falls back to the first optional entry
 * (the translation model is the only optional entry today) so a renamed id
 * does not silently break the download card. Returns `null` when the report
 * carries no such entry.
 */
export function findTranslationModelEntry(
  report: ModelStatusReport,
): ModelEntryStatus | null {
  return (
    report.entries.find((entry) => entry.id === TRANSLATION_MODEL_ENTRY_ID) ??
    report.entries.find((entry) => entry.optional) ??
    null
  );
}

/**
 * Reports whether the model setup needs user attention (R6 startup banner).
 *
 * `true` when OCR is not ready or any non-optional entry failed verification.
 * Optional entries (the downloadable translation model) never raise the
 * banner; they are surfaced by the settings download card instead.
 */
export function hasModelSetupProblems(report: ModelStatusReport): boolean {
  if (!report.ocr_ready) return true;
  return report.entries.some((entry) => !entry.optional && entry.state === "invalid");
}

/** Why the local engine option is unavailable in the provider picker. */
export type LocalProviderBlockReason = "missing" | "invalid" | "downloading";

/**
 * Derives whether switching to the local engine must be blocked.
 *
 * `null` means the local option is selectable. A `null` report (not yet
 * hydrated) never blocks, avoiding a flicker before the first
 * `get_model_status` resolves; a download in flight always blocks (double
 * insurance with the backend rejection). A missing translation entry (older
 * backend) is treated as unknown and does not block.
 */
export function localProviderBlockReason(
  report: ModelStatusReport | null,
  downloading: boolean,
): LocalProviderBlockReason | null {
  if (downloading) return "downloading";
  if (report === null) return null;
  const entry = findTranslationModelEntry(report);
  if (!entry || entry.state === "ready") return null;
  return entry.state === "invalid" ? "invalid" : "missing";
}

/**
 * Reports whether any translation box is currently running.
 *
 * The multi-box session is considered running when at least one box reports
 * `"Running"`. Error/Stopped entries never count. Shared by the main window
 * (live-mode start/stop controls) and the floating ball (menu state), and by
 * the store hydration path to avoid fabricating a single-live session over a
 * running multi-box session.
 */
export function isAnyBoxRunning(statuses: Record<number, BoxStatus>): boolean {
  return Object.values(statuses).some((status) => status === "Running");
}

/**
 * Reports whether the single-region live session is running (or paused).
 *
 * A live config exists whenever the session has been started and not stopped,
 * including while paused, so a paused session still counts as running here:
 * the stop control must remain available. Multi-box sessions are deliberately
 * excluded — their running state derives from `isAnyBoxRunning`.
 */
export function isSingleLiveRunning(
  mode: Mode,
  liveConfig: PipelineConfig | null,
): boolean {
  return mode === "live" && liveConfig !== null;
}

/** Maps a box status to its Chinese UI label. */
export function boxStatusLabel(status: BoxStatus): string {
  if (isBoxError(status)) return "错误";
  return status === "Running" ? "运行中" : "已停止";
}

/** Whether the multi-box session is actively engaged (running or has results). */
export function isMultiBoxEngaged(
  statuses: Record<number, BoxStatus>,
  resultCount: number,
): boolean {
  if (resultCount > 0) return true;
  return Object.values(statuses).some((status) => status === "Running");
}

/** Whether the active-box count should surface a performance warning. */
export function shouldWarnBoxCount(count: number, warningThreshold: number): boolean {
  return warningThreshold > 0 && count >= warningThreshold;
}

/** Human-readable warning text for an over-threshold box count. */
export function boxCountWarningText(warningThreshold: number): string {
  return `翻译框过多可能导致卡顿，建议不超过 ${warningThreshold} 个`;
}
