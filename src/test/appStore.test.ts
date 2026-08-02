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
      pipeline_status: "idle",
      ocr_provider: "pp-ocr",
      translation_provider: "local",
      selected_region: null,
      live_running: false,
      model_progress: null,
    });
    expect(useAppStore.getState().config.translation.provider).toBe("local");
  });
});
