import { beforeEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_CONFIG } from "../types";
import { useAppStore } from "../stores/appStore";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const emit = vi.fn();
vi.mock("@tauri-apps/api/event", () => ({ emit }));

const getByLabel = vi.fn();
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  WebviewWindow: { getByLabel: (...args: unknown[]) => getByLabel(...args) },
}));

const {
  selectAndTranslateOnce,
  selectRegionForLive,
  startLive,
  stopLive,
  toggleLiveFromFloater,
  toggleLivePause,
} = await import("../services/translateActions");

const REGION = { monitor_id: "display-1", x: 0, y: 10, width: 80, height: 40 };
const OCR_RESULT = { lines: [], merged_text: "hello", detected_language: null, elapsed_ms: 3 };

beforeEach(() => {
  vi.clearAllMocks();
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
  getByLabel.mockResolvedValue({
    show: vi.fn().mockResolvedValue(undefined),
    hide: vi.fn().mockResolvedValue(undefined),
    setFocus: vi.fn().mockResolvedValue(undefined),
  });
});

describe("selectAndTranslateOnce", () => {
  it("selects a region and runs a single capture", async () => {
    invoke
      .mockResolvedValueOnce(REGION)
      .mockResolvedValueOnce(OCR_RESULT);
    const result = await selectAndTranslateOnce();
    expect(result).toEqual({ ok: true, cancelled: false });
    expect(invoke).toHaveBeenCalledWith("start_region_selection", undefined);
    expect(invoke).toHaveBeenCalledWith("capture_once", { region: REGION });
    expect(useAppStore.getState().status).toBe("completed");
    expect(useAppStore.getState().ocrResult).toEqual(OCR_RESULT);
    expect(emit).toHaveBeenCalledWith("frontend_ocr_result", OCR_RESULT);
  });

  it("treats a cancelled selection as a non-error", async () => {
    invoke.mockRejectedValueOnce("state not initialized");
    const result = await selectAndTranslateOnce();
    expect(result).toEqual({ ok: false, cancelled: true });
    expect(useAppStore.getState().status).toBe("idle");
  });

  it("surfaces capture failures through the shared store", async () => {
    invoke.mockResolvedValueOnce(REGION).mockRejectedValueOnce("capture error");
    const result = await selectAndTranslateOnce();
    expect(result).toEqual({ ok: false, cancelled: false });
    expect(useAppStore.getState().status).toEqual({ error: "capture error" });
  });
});

describe("selectRegionForLive", () => {
  it("starts a live session after selecting a region", async () => {
    invoke.mockResolvedValueOnce(REGION).mockResolvedValueOnce(undefined);
    const result = await selectRegionForLive();
    expect(result.ok).toBe(true);
    expect(invoke).toHaveBeenCalledWith("start_live_translation", {
      config: { region: REGION, capture_interval_ms: 500, difference_threshold: 0.03 },
    });
    expect(useAppStore.getState().mode).toBe("live");
    expect(useAppStore.getState().liveConfig?.region).toEqual(REGION);
    expect(emit).toHaveBeenCalledWith(
      "frontend_live_config",
      expect.objectContaining({ region: REGION }),
    );
  });
});

describe("startLive", () => {
  it("reports a missing region through the store", async () => {
    const result = await startLive();
    expect(result.ok).toBe(false);
    expect(useAppStore.getState().status).toEqual({ error: "请先选择翻译区域" });
  });

  it("starts the live session with the selected region", async () => {
    useAppStore.getState().setSelectedRegion(REGION);
    invoke.mockResolvedValueOnce(undefined);
    const result = await startLive();
    expect(result.ok).toBe(true);
    expect(useAppStore.getState().status).toBe("capturing");
  });
});

describe("toggleLiveFromFloater", () => {
  it("starts live with a region selection when idle and no region exists", async () => {
    invoke.mockResolvedValueOnce(REGION).mockResolvedValueOnce(undefined);
    const result = await toggleLiveFromFloater();
    expect(result.ok).toBe(true);
    expect(useAppStore.getState().mode).toBe("live");
  });

  it("stops a running live session", async () => {
    useAppStore.getState().setMode("live");
    useAppStore.getState().setLiveConfig({
      region: REGION,
      capture_interval_ms: 500,
      difference_threshold: 0.03,
    });
    invoke.mockResolvedValueOnce(undefined);
    const result = await toggleLiveFromFloater();
    expect(result.ok).toBe(true);
    expect(useAppStore.getState().mode).toBe("single");
    expect(useAppStore.getState().liveConfig).toBeNull();
    expect(emit).toHaveBeenCalledWith("frontend_live_stopped");
  });
});

describe("toggleLivePause / stopLive", () => {
  it("pauses and resumes a live session", async () => {
    useAppStore.getState().setMode("live");
    useAppStore.getState().setLiveConfig({
      region: REGION,
      capture_interval_ms: 500,
      difference_threshold: 0.03,
    });
    invoke.mockResolvedValueOnce(undefined);
    await toggleLivePause();
    expect(useAppStore.getState().livePaused).toBe(true);
    expect(invoke).toHaveBeenCalledWith("stop_live_translation", undefined);

    invoke.mockResolvedValueOnce(undefined);
    await toggleLivePause();
    expect(useAppStore.getState().livePaused).toBe(false);
    expect(invoke).toHaveBeenCalledWith("start_live_translation", expect.any(Object));
  });

  it("resets to single mode after a real stop", async () => {
    useAppStore.getState().setMode("live");
    useAppStore.getState().setLiveConfig({
      region: REGION,
      capture_interval_ms: 500,
      difference_threshold: 0.03,
    });
    invoke.mockResolvedValueOnce(undefined);
    const result = await stopLive();
    expect(result.ok).toBe(true);
    expect(useAppStore.getState().mode).toBe("single");
    expect(useAppStore.getState().liveConfig).toBeNull();
  });
});
