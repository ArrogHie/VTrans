import { create } from "zustand";
import type {
  AppConfig,
  AppStatus,
  BoxStatus,
  BoxedTranslationResult,
  LanguageCode,
  Mode,
  ModelDownloadProgress,
  ModelStatusReport,
  OcrResult,
  PipelineConfig,
  PipelineStatus,
  ProviderId,
  ScreenRegion,
  SingleResultPayload,
  TranslationBoxInfo,
  TranslationResult,
} from "../types";
import { DEFAULT_CONFIG, isAnyBoxRunning, normalizeProviderId } from "../types";

interface AppState {
  mode: Mode;
  status: PipelineStatus;
  ocrResult: OcrResult | null;
  translationResult: TranslationResult | null;
  selectedRegion: ScreenRegion | null;
  error: string | null;
  modelProgress: number | null;
  config: AppConfig;
  hydrated: boolean;
  liveConfig: PipelineConfig | null;
  livePaused: boolean;
  /** Configured multi-box translation boxes (ordered, per-window). */
  translationBoxes: TranslationBoxInfo[];
  /** Latest runtime status per box id. */
  boxStatuses: Record<number, BoxStatus>;
  /** Latest translation result per box id. */
  multiBoxResults: Record<number, BoxedTranslationResult>;
  /** Latest single-capture result shown by the result window. */
  singleResult: SingleResultPayload | null;
  /**
   * Last `get_model_status` / `retry_model_setup` snapshot, `null` until the
   * main window hydrates it. Drives the R6 startup banner and the local
   * engine availability in the provider picker.
   */
  modelStatus: ModelStatusReport | null;
  /** Last translation model download progress payload (`null` = none yet). */
  modelDownloadProgress: ModelDownloadProgress | null;
  /** Whether a translation model download is considered in flight. */
  translationModelDownloading: boolean;
  setMode: (mode: Mode) => void;
  setStatus: (status: PipelineStatus) => void;
  setOcrResult: (result: OcrResult | null) => void;
  setTranslationResult: (result: TranslationResult | null) => void;
  setSelectedRegion: (region: ScreenRegion | null) => void;
  setError: (error: string | null) => void;
  setModelProgress: (progress: number | null) => void;
  setModelStatus: (report: ModelStatusReport | null) => void;
  setModelDownloadProgress: (progress: ModelDownloadProgress | null) => void;
  setTranslationModelDownloading: (downloading: boolean) => void;
  providerSwitching: boolean;
  setProviderSwitching: (switching: boolean) => void;
  setConfig: (config: AppConfig) => void;
  setLiveConfig: (config: PipelineConfig | null) => void;
  setLivePaused: (paused: boolean) => void;
  updateLanguage: (kind: "ocr" | "source" | "target", language: LanguageCode) => void;
  setProvider: (provider: ProviderId) => void;
  applyStatus: (status: AppStatus) => void;
  resetResults: () => void;
  setTranslationBoxes: (boxes: TranslationBoxInfo[]) => void;
  upsertBox: (box: TranslationBoxInfo) => void;
  removeBox: (boxId: number) => void;
  updateBoxRegion: (boxId: number, region: ScreenRegion) => void;
  setBoxStatus: (boxId: number, status: BoxStatus) => void;
  setBoxesStatus: (boxIds: number[], status: BoxStatus) => void;
  setMultiBoxResult: (result: BoxedTranslationResult) => void;
  setSingleResult: (result: SingleResultPayload | null) => void;
  resetMultiBox: () => void;
}

export const useAppStore = create<AppState>((set) => ({
  mode: "single",
  status: "idle",
  ocrResult: null,
  translationResult: null,
  selectedRegion: null,
  error: null,
  modelProgress: null,
  providerSwitching: false,
  config: structuredClone(DEFAULT_CONFIG),
  hydrated: false,
  liveConfig: null,
  livePaused: false,
  translationBoxes: [],
  boxStatuses: {},
  multiBoxResults: {},
  singleResult: null,
  modelStatus: null,
  modelDownloadProgress: null,
  translationModelDownloading: false,
  setMode: (mode) => set({ mode, error: null }),
  setStatus: (status) => set({ status, error: typeof status === "object" ? status.error : null }),
  setOcrResult: (ocrResult) => set({ ocrResult }),
  setTranslationResult: (translationResult) => set({ translationResult }),
  setSelectedRegion: (selectedRegion) => set({ selectedRegion }),
  setError: (error) => set({ error, status: error ? { error } : "idle" }),
  setModelProgress: (modelProgress) => set({ modelProgress }),
  setModelStatus: (modelStatus) => set({ modelStatus }),
  setModelDownloadProgress: (modelDownloadProgress) => set({ modelDownloadProgress }),
  setTranslationModelDownloading: (translationModelDownloading) =>
    set({ translationModelDownloading }),
  setProviderSwitching: (switching) => set({ providerSwitching: switching }),
  setConfig: (config) => set({ config, hydrated: true }),
  setLiveConfig: (liveConfig) => set({ liveConfig }),
  setLivePaused: (livePaused) => set({ livePaused }),
  updateLanguage: (kind, language) =>
    set((state) => {
      // `ocr.language` 与 `translation.source_language` 是后端联动字段
      //（见 vtrans-app 的 set_ocr_language / set_source_language：两者
      // 总是同步赋值，由 vtrans_config::validate_language_linkage 校验）。
      // 乐观更新必须镜像后端语义，同时写入两个字段，避免 hydrate 回滚前
      // 本地 state 短暂不一致导致 UI 闪烁或与后端联动校验冲突。
      // target_language 不参与联动，行为保持不变。
      if (kind === "ocr") {
        return {
          config: {
            ...state.config,
            ocr: { ...state.config.ocr, language },
            translation: { ...state.config.translation, source_language: language },
          },
        };
      }
      if (kind === "source") {
        return {
          config: {
            ...state.config,
            ocr: { ...state.config.ocr, language },
            translation: { ...state.config.translation, source_language: language },
          },
        };
      }
      if (language === "auto") return state;
      return {
        config: {
          ...state.config,
          translation: { ...state.config.translation, target_language: language },
        },
      };
    }),
  setProvider: (provider) =>
    set((state) => ({
      config: { ...state.config, translation: { ...state.config.translation, provider } },
    })),
  applyStatus: (status) =>
    set((state) => {
      // 后端透传 provider 运行时实现 id（"openai" / "deepl" / "google" /
      // "azure" / "baidu" / "local-onnx"），映射到前端配置标识符域
      // （云端 id 原样透传，仅 "local-onnx" -> "local"）。
      const provider = normalizeProviderId(status.translation_provider);
      // Hotkey-started live sessions never publish `frontend_live_config`
      // (that is an app-module coordination item). When the backend reports
      // a running session without a local live config, reconstruct one from
      // the backend-selected region and the capture defaults so pause/stop
      // controls work immediately. An existing live config is preserved.
      //
      // 防回退：本地已有多框框在 Running（经 multibox://status 或
      // frontend_multibox_* 事件同步）时，后端快照的 live_running 可能描述
      // 同一个多框会话（后端修复后多框运行报告 mode "live"）。此时禁止
      // 凭空构造单框 liveConfig，否则悬浮球会误判单框实时在运行，
      // 「暂停·继续」解禁、停止走单框路径。boxStatuses 本身永不被水合覆盖。
      const liveConfig =
        status.live_running &&
        status.selected_region &&
        !state.liveConfig &&
        !isAnyBoxRunning(state.boxStatuses)
          ? {
              region: status.selected_region,
              capture_interval_ms: state.config.capture.interval_ms,
              difference_threshold: state.config.capture.difference_threshold,
            }
          : state.liveConfig;
      return {
        status: status.pipeline_status,
        // 后端模式是最近一次会话的权威记录：运行中的会话必然是 live，
        // 否则采用后端报告的 single/live（暂停中的 live 会话保持 live）。
        mode: status.live_running ? "live" : status.mode,
        selectedRegion: status.selected_region,
        modelProgress: status.model_progress,
        config: {
          ...state.config,
          translation: { ...state.config.translation, provider },
        },
        liveConfig,
        // A running backend session contradicts a locally paused marker;
        // clear it so the pause/resume controls reflect reality.
        livePaused: status.live_running ? false : state.livePaused,
        hydrated: true,
      };
    }),
  resetResults: () => set({ ocrResult: null, translationResult: null, error: null }),
  setTranslationBoxes: (translationBoxes) => set({ translationBoxes }),
  upsertBox: (box) =>
    set((state) => ({
      translationBoxes: state.translationBoxes.some((entry) => entry.box_id === box.box_id)
        ? state.translationBoxes.map((entry) => (entry.box_id === box.box_id ? box : entry))
        : [...state.translationBoxes, box],
    })),
  removeBox: (boxId) =>
    set((state) => {
      const boxStatuses = { ...state.boxStatuses };
      delete boxStatuses[boxId];
      const multiBoxResults = { ...state.multiBoxResults };
      delete multiBoxResults[boxId];
      return {
        translationBoxes: state.translationBoxes.filter((entry) => entry.box_id !== boxId),
        boxStatuses,
        multiBoxResults,
      };
    }),
  updateBoxRegion: (boxId, region) =>
    set((state) => ({
      translationBoxes: state.translationBoxes.map((entry) =>
        entry.box_id === boxId ? { ...entry, region } : entry,
      ),
    })),
  setBoxStatus: (boxId, status) =>
    set((state) => ({ boxStatuses: { ...state.boxStatuses, [boxId]: status } })),
  setBoxesStatus: (boxIds, status) =>
    set((state) => {
      const boxStatuses = { ...state.boxStatuses };
      for (const boxId of boxIds) boxStatuses[boxId] = status;
      return { boxStatuses };
    }),
  setMultiBoxResult: (result) =>
    set((state) => ({ multiBoxResults: { ...state.multiBoxResults, [result.box_id]: result } })),
  setSingleResult: (singleResult) => set({ singleResult }),
  resetMultiBox: () =>
    set({ translationBoxes: [], boxStatuses: {}, multiBoxResults: {}, singleResult: null }),
}));

export type { AppState };
