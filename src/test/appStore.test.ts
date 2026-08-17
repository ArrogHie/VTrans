import { beforeEach, describe, expect, it } from "vitest";
import { DEFAULT_CONFIG } from "../types";
import { useAppStore } from "../stores/appStore";

beforeEach(() => {
  useAppStore.setState({
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
  });
});

describe("appStore", () => {
  const region = { monitor_id: "display-1", x: 0, y: 0, width: 640, height: 480 };

  it("updates mode and status immutably", () => {
    useAppStore.getState().setMode("live");
    useAppStore.getState().setStatus("capturing");
    expect(useAppStore.getState().mode).toBe("live");
    expect(useAppStore.getState().status).toBe("capturing");
  });

  it("updates nested language settings without replacing other settings", () => {
    const before = useAppStore.getState().config;
    useAppStore.getState().updateLanguage("target", "ja");
    const after = useAppStore.getState().config;
    expect(after.translation.target_language).toBe("ja");
    expect(after.capture).toEqual(before.capture);
    expect(after.translation.provider).toBe(before.translation.provider);
  });

  it("syncs ocr.language and translation.source_language when switching ocr language", () => {
    // 后端 set_ocr_language 同时写入 ocr.language 与 translation.source_language
    //（联动字段），乐观更新必须镜像该语义，避免 hydrate 回滚前本地短暂不一致。
    useAppStore.getState().updateLanguage("ocr", "ja");
    const config = useAppStore.getState().config;
    expect(config.ocr.language).toBe("ja");
    expect(config.translation.source_language).toBe("ja");
  });

  it("syncs translation.source_language and ocr.language when switching source language", () => {
    // 后端 set_source_language 同样同时写入两个字段。
    useAppStore.getState().updateLanguage("source", "en");
    const config = useAppStore.getState().config;
    expect(config.translation.source_language).toBe("en");
    expect(config.ocr.language).toBe("en");
  });

  it("keeps the linked fields equal across repeated ocr/source switches", () => {
    for (const language of ["auto", "ja", "en", "zh-CN"] as const) {
      useAppStore.getState().updateLanguage("ocr", language);
      const afterOcr = useAppStore.getState().config;
      expect(afterOcr.ocr.language).toBe(language);
      expect(afterOcr.translation.source_language).toBe(language);
    }
    for (const language of ["en", "zh-CN", "ja", "auto"] as const) {
      useAppStore.getState().updateLanguage("source", language);
      const afterSource = useAppStore.getState().config;
      expect(afterSource.ocr.language).toBe(language);
      expect(afterSource.translation.source_language).toBe(language);
    }
  });

  it("does not touch the linked fields when switching target language", () => {
    // target_language 不参与联动：切换 target 不应改动 ocr.language 或
    // translation.source_language。
    useAppStore.getState().updateLanguage("ocr", "ja");
    const before = useAppStore.getState().config;
    useAppStore.getState().updateLanguage("target", "en");
    const after = useAppStore.getState().config;
    expect(after.translation.target_language).toBe("en");
    expect(after.ocr.language).toBe(before.ocr.language);
    expect(after.translation.source_language).toBe(before.translation.source_language);
  });

  it("ignores an auto target language switch and leaves config unchanged", () => {
    useAppStore.getState().updateLanguage("ocr", "ja");
    const before = useAppStore.getState().config;
    useAppStore.getState().updateLanguage("target", "auto");
    expect(useAppStore.getState().config).toBe(before);
  });

  it("shares live configuration and pause state across window adapters", () => {
    const config = {
      region: { monitor_id: "display-1", x: 0, y: 0, width: 100, height: 80 },
      capture_interval_ms: 500,
      difference_threshold: 0.03,
    };
    useAppStore.getState().setLiveConfig(config);
    useAppStore.getState().setLivePaused(true);
    expect(useAppStore.getState().liveConfig).toEqual(config);
    expect(useAppStore.getState().livePaused).toBe(true);
  });

  it("represents errors as both visible error and pipeline error status", () => {
    useAppStore.getState().setError("后端不可用");
    expect(useAppStore.getState().error).toBe("后端不可用");
    expect(useAppStore.getState().status).toEqual({ error: "后端不可用" });
  });

  it("tracks provider switching state for busy UI feedback", () => {
    expect(useAppStore.getState().providerSwitching).toBe(false);
    useAppStore.getState().setProviderSwitching(true);
    expect(useAppStore.getState().providerSwitching).toBe(true);
    useAppStore.getState().setProviderSwitching(false);
    expect(useAppStore.getState().providerSwitching).toBe(false);
  });

  it("stores model loading progress driven by backend events", () => {
    useAppStore.getState().setModelProgress(0.35);
    expect(useAppStore.getState().modelProgress).toBe(0.35);
    useAppStore.getState().setModelProgress(1);
    expect(useAppStore.getState().modelProgress).toBe(1);
    // 切换完成/失败后清空，避免下次切换闪烁旧百分比。
    useAppStore.getState().setModelProgress(null);
    expect(useAppStore.getState().modelProgress).toBeNull();
  });

  it("stores the model status snapshot, download progress and in-flight marker immutably", () => {
    const report = {
      entries: [{ id: "opus-mt-en-zh-int8", state: "missing" as const, optional: true }],
      ocr_ready: true,
      translation_ready: false,
    };
    const progress = { bytes: 10, total: 20, fraction: 0.5 };
    useAppStore.getState().setModelStatus(report);
    expect(useAppStore.getState().modelStatus).toEqual(report);
    useAppStore.getState().setModelDownloadProgress(progress);
    expect(useAppStore.getState().modelDownloadProgress).toEqual(progress);
    useAppStore.getState().setTranslationModelDownloading(true);
    expect(useAppStore.getState().translationModelDownloading).toBe(true);
    useAppStore.getState().setTranslationModelDownloading(false);
    expect(useAppStore.getState().translationModelDownloading).toBe(false);
    useAppStore.getState().setModelStatus(null);
    expect(useAppStore.getState().modelStatus).toBeNull();
  });

  it("hydrates the selected translation provider from backend status", () => {
    useAppStore.getState().applyStatus({
      mode: "single",
      pipeline_status: "idle",
      ocr_provider: "pp-ocr",
      translation_provider: "local-onnx",
      selected_region: null,
      live_running: false,
      model_progress: null,
      debug_mode: false,
    });
    expect(useAppStore.getState().config.translation.provider).toBe("local");
  });

  it("maps unknown provider ids back to the openai default", () => {
    useAppStore.getState().applyStatus({
      mode: "single",
      pipeline_status: "idle",
      ocr_provider: "pp-ocr",
      translation_provider: "unexpected-provider",
      selected_region: null,
      live_running: false,
      model_progress: null,
      debug_mode: false,
    });
    expect(useAppStore.getState().config.translation.provider).toBe("openai");
  });

  it("passes cloud provider runtime ids through unchanged", () => {
    for (const provider of ["openai", "deepl", "google", "azure", "baidu"]) {
      useAppStore.getState().applyStatus({
        mode: "single",
        pipeline_status: "idle",
        ocr_provider: "pp-ocr",
        translation_provider: provider,
        selected_region: null,
        live_running: false,
        model_progress: null,
        debug_mode: false,
      });
      expect(useAppStore.getState().config.translation.provider).toBe(provider);
    }
  });

  it("constructs a live config fallback for hotkey-started sessions", () => {
    useAppStore.getState().applyStatus({
      mode: "live",
      pipeline_status: "capturing",
      ocr_provider: "pp-ocr",
      translation_provider: "openai",
      selected_region: region,
      live_running: true,
      model_progress: null,
      debug_mode: false,
    });
    const state = useAppStore.getState();
    expect(state.mode).toBe("live");
    expect(state.livePaused).toBe(false);
    expect(state.liveConfig).toEqual({
      region,
      capture_interval_ms: 500,
      difference_threshold: 0.03,
    });
  });

  it("keeps live config null when a running session has no selected region", () => {
    useAppStore.getState().applyStatus({
      mode: "live",
      pipeline_status: "capturing",
      ocr_provider: "pp-ocr",
      translation_provider: "openai",
      selected_region: null,
      live_running: true,
      model_progress: null,
      debug_mode: false,
    });
    expect(useAppStore.getState().liveConfig).toBeNull();
  });

  it("preserves an existing live config instead of overwriting it", () => {
    const existing = {
      region: { monitor_id: "display-2", x: 10, y: 20, width: 100, height: 80 },
      capture_interval_ms: 750,
      difference_threshold: 0.05,
    };
    useAppStore.getState().setLiveConfig(existing);
    useAppStore.getState().applyStatus({
      mode: "live",
      pipeline_status: "capturing",
      ocr_provider: "pp-ocr",
      translation_provider: "openai",
      selected_region: region,
      live_running: true,
      model_progress: null,
      debug_mode: false,
    });
    expect(useAppStore.getState().liveConfig).toEqual(existing);
  });

  it("clears a stale paused marker when the backend reports a running session", () => {
    const existing = {
      region,
      capture_interval_ms: 500,
      difference_threshold: 0.03,
    };
    useAppStore.getState().setLiveConfig(existing);
    useAppStore.getState().setLivePaused(true);
    useAppStore.getState().applyStatus({
      mode: "live",
      pipeline_status: "capturing",
      ocr_provider: "pp-ocr",
      translation_provider: "openai",
      selected_region: region,
      live_running: true,
      model_progress: null,
      debug_mode: false,
    });
    expect(useAppStore.getState().livePaused).toBe(false);
    expect(useAppStore.getState().liveConfig).toEqual(existing);
  });

  it("keeps the live config across a paused backend state for resume", () => {
    const existing = {
      region,
      capture_interval_ms: 500,
      difference_threshold: 0.03,
    };
    useAppStore.getState().setLiveConfig(existing);
    useAppStore.getState().setLivePaused(true);
    useAppStore.getState().applyStatus({
      mode: "live",
      pipeline_status: "idle",
      ocr_provider: "pp-ocr",
      translation_provider: "openai",
      selected_region: region,
      live_running: false,
      model_progress: null,
      debug_mode: false,
    });
    expect(useAppStore.getState().liveConfig).toEqual(existing);
    expect(useAppStore.getState().livePaused).toBe(true);
  });

  it("hydrates the session mode from the backend snapshot", () => {
    useAppStore.getState().applyStatus({
      mode: "single",
      pipeline_status: "idle",
      ocr_provider: "pp-ocr",
      translation_provider: "openai",
      selected_region: null,
      live_running: false,
      model_progress: null,
      debug_mode: false,
    });
    expect(useAppStore.getState().mode).toBe("single");
  });

  it("keeps live mode for a paused backend session", () => {
    useAppStore.getState().applyStatus({
      mode: "live",
      pipeline_status: "idle",
      ocr_provider: "pp-ocr",
      translation_provider: "openai",
      selected_region: region,
      live_running: false,
      model_progress: null,
      debug_mode: false,
    });
    expect(useAppStore.getState().mode).toBe("live");
  });

  it("does not fabricate a single-live config while a multi-box session is running", () => {
    // BUGFIX-4：多框运行态经跨窗口事件同步进 boxStatuses。后端修复后多框
    // 运行会报告 mode "live"（甚至 live_running），水合不得据此凭空构造
    // 单框 liveConfig——否则悬浮球会误以为单框实时在运行（暂停按钮解禁、
    // 停止走单框路径）。
    useAppStore.getState().setBoxStatus(0, "Running");
    useAppStore.getState().applyStatus({
      mode: "live",
      pipeline_status: "capturing",
      ocr_provider: "pp-ocr",
      translation_provider: "openai",
      selected_region: region,
      live_running: true,
      model_progress: null,
      debug_mode: false,
    });
    const state = useAppStore.getState();
    expect(state.liveConfig).toBeNull();
    expect(state.boxStatuses).toEqual({ 0: "Running" });
  });

  it("never overwrites multi-box statuses during hydration", () => {
    useAppStore.getState().setBoxStatus(0, "Running");
    useAppStore.getState().setBoxStatus(1, "Stopped");
    useAppStore.getState().applyStatus({
      mode: "single",
      pipeline_status: "idle",
      ocr_provider: "pp-ocr",
      translation_provider: "openai",
      selected_region: null,
      live_running: false,
      model_progress: null,
      debug_mode: false,
    });
    expect(useAppStore.getState().boxStatuses).toEqual({ 0: "Running", 1: "Stopped" });
  });

  it("still reconstructs a hotkey live config when no box is running", () => {
    // 保护只针对多框运行态：无框运行时的原有热键兜底行为保持不变。
    useAppStore.getState().setBoxStatus(0, "Stopped");
    useAppStore.getState().applyStatus({
      mode: "live",
      pipeline_status: "capturing",
      ocr_provider: "pp-ocr",
      translation_provider: "openai",
      selected_region: region,
      live_running: true,
      model_progress: null,
      debug_mode: false,
    });
    expect(useAppStore.getState().liveConfig).toEqual({
      region,
      capture_interval_ms: 500,
      difference_threshold: 0.03,
    });
  });
});
