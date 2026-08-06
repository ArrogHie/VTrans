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
    config: structuredClone(DEFAULT_CONFIG),
    hydrated: false,
    liveConfig: null,
    livePaused: false,
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

  it("maps unknown provider ids back to the api default", () => {
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
    expect(useAppStore.getState().config.translation.provider).toBe("api");
  });

  it("constructs a live config fallback for hotkey-started sessions", () => {
    useAppStore.getState().applyStatus({
      mode: "live",
      pipeline_status: "capturing",
      ocr_provider: "pp-ocr",
      translation_provider: "api",
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
      translation_provider: "api",
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
      translation_provider: "api",
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
      translation_provider: "api",
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
      translation_provider: "api",
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
      translation_provider: "api",
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
      translation_provider: "api",
      selected_region: region,
      live_running: false,
      model_progress: null,
      debug_mode: false,
    });
    expect(useAppStore.getState().mode).toBe("live");
  });
});
