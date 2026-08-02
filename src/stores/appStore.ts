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
import { DEFAULT_CONFIG } from "../types";

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
      if (kind === "ocr") return { config: { ...state.config, ocr: { ...state.config.ocr, language } } };
      if (kind === "source") {
        return {
          config: {
            ...state.config,
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
    set((state) => ({
      status: status.pipeline_status,
      mode: status.live_running ? "live" : state.mode,
      selectedRegion: status.selected_region,
      modelProgress: status.model_progress,
      hydrated: true,
    })),
  resetResults: () => set({ ocrResult: null, translationResult: null, error: null }),
}));

export type { AppState };
