import { create } from "zustand";
import type {
  AppConfig,
  AppStatus,
  LanguageCode,
  Mode,
  OcrResult,
  PipelineConfig,
  PipelineStatus,
  ProviderId,
  ScreenRegion,
  TranslationResult,
} from "../types";
import { DEFAULT_CONFIG, normalizeProviderId } from "../types";

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
  setMode: (mode: Mode) => void;
  setStatus: (status: PipelineStatus) => void;
  setOcrResult: (result: OcrResult | null) => void;
  setTranslationResult: (result: TranslationResult | null) => void;
  setSelectedRegion: (region: ScreenRegion | null) => void;
  setError: (error: string | null) => void;
  setModelProgress: (progress: number | null) => void;
  setConfig: (config: AppConfig) => void;
  setLiveConfig: (config: PipelineConfig | null) => void;
  setLivePaused: (paused: boolean) => void;
  updateLanguage: (kind: "ocr" | "source" | "target", language: LanguageCode) => void;
  setProvider: (provider: ProviderId) => void;
  applyStatus: (status: AppStatus) => void;
  resetResults: () => void;
}

export const useAppStore = create<AppState>((set) => ({
  mode: "single",
  status: "idle",
  ocrResult: null,
  translationResult: null,
  selectedRegion: null,
  error: null,
  modelProgress: null,
  config: structuredClone(DEFAULT_CONFIG),
  hydrated: false,
  liveConfig: null,
  livePaused: false,
  setMode: (mode) => set({ mode, error: null }),
  setStatus: (status) => set({ status, error: typeof status === "object" ? status.error : null }),
  setOcrResult: (ocrResult) => set({ ocrResult }),
  setTranslationResult: (translationResult) => set({ translationResult }),
  setSelectedRegion: (selectedRegion) => set({ selectedRegion }),
  setError: (error) => set({ error, status: error ? { error } : "idle" }),
  setModelProgress: (modelProgress) => set({ modelProgress }),
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
      // 后端透传 provider 实现 id（"api" / "local-onnx"），映射到前端
      // 配置标识符域（"api" / "local"）。
      const provider = normalizeProviderId(status.translation_provider);
      // Hotkey-started live sessions never publish `frontend_live_config`
      // (that is an app-module coordination item). When the backend reports
      // a running session without a local live config, reconstruct one from
      // the backend-selected region and the capture defaults so pause/stop
      // controls work immediately. An existing live config is preserved.
      const liveConfig =
        status.live_running && status.selected_region && !state.liveConfig
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
}));

export type { AppState };
