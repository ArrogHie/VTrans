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

// toggleLiveFromFloater 经 multiBoxActions 进入多框路径；overlay 定位/隐藏
// 的行为由 regionOverlay.test.ts 覆盖，这里只需隔离副作用。
const showMultiBoxOverlay = vi.fn();
const hideRegionOverlay = vi.fn();
vi.mock("../services/regionOverlay", () => ({ showMultiBoxOverlay, hideRegionOverlay }));

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
const BOX = { box_id: 0, region: REGION, color: "#FF6B6B" };

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
    translationBoxes: [],
    boxStatuses: {},
    multiBoxResults: {},
    singleResult: null,
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

  it("stops the multi-box session when any box is running", async () => {
    // BUGFIX-4：悬浮球与主窗口共享同一多框会话——任一框 Running 时点击
    // 「停止实时翻译」走 stop_multi_realtime，而不是另起单框实时。
    useAppStore.getState().setTranslationBoxes([BOX]);
    useAppStore.getState().setBoxStatus(0, "Running");
    invoke.mockResolvedValueOnce(undefined);

    const result = await toggleLiveFromFloater();
    expect(result.ok).toBe(true);
    expect(invoke).toHaveBeenCalledWith("stop_multi_realtime", undefined);
    expect(invoke).not.toHaveBeenCalledWith("stop_live_translation", undefined);
    expect(useAppStore.getState().boxStatuses).toEqual({ 0: "Stopped" });
    expect(emit).toHaveBeenCalledWith("frontend_multibox_stopped", { box_ids: [0] });
    // 单框会话状态不被触碰。
    expect(useAppStore.getState().liveConfig).toBeNull();
  });

  it("starts the multi-box session when boxes exist and nothing runs", async () => {
    // 未运行且有框：与主窗口 live 模式一致，直接启动多框会话。
    useAppStore.getState().setTranslationBoxes([BOX]);
    showMultiBoxOverlay.mockResolvedValue(undefined);
    invoke.mockResolvedValueOnce(undefined);

    const result = await toggleLiveFromFloater();
    expect(result.ok).toBe(true);
    expect(invoke).toHaveBeenCalledWith("start_multi_realtime", undefined);
    expect(useAppStore.getState().boxStatuses).toEqual({ 0: "Running" });
    expect(emit).toHaveBeenCalledWith("frontend_multibox_started", { box_ids: [0] });
    // 不触发任何单框路径。
    expect(invoke).not.toHaveBeenCalledWith("start_region_selection", undefined);
    expect(invoke).not.toHaveBeenCalledWith("start_live_translation", expect.any(Object));
  });

  it("keeps the region-selection start when no boxes exist", async () => {
    // 未运行且无框：保持原单框路径（先框选再启动单框实时）。
    invoke.mockResolvedValueOnce(REGION).mockResolvedValueOnce(undefined);
    const result = await toggleLiveFromFloater();
    expect(result.ok).toBe(true);
    expect(invoke).toHaveBeenCalledWith("start_region_selection", undefined);
    expect(invoke).toHaveBeenCalledWith("start_live_translation", expect.any(Object));
    expect(invoke).not.toHaveBeenCalledWith("start_multi_realtime", undefined);
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
