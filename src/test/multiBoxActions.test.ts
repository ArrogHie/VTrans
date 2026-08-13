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
  addBox,
  editBox,
  hydrateBoxes,
  openResultPopup,
  removeBox,
  startMultiBox,
  stopMultiBox,
  stopSingleBox,
} = await import("../services/multiBoxActions");

const REGION = { monitor_id: "display-1", x: 0, y: 10, width: 80, height: 40 };
const BOX_INFO = { box_id: 0, region: REGION, color: "#FF6B6B" };

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
    providerSwitching: false,
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

describe("addBox", () => {
  it("selects a region, adds the box, and upserts it into the store", async () => {
    invoke.mockResolvedValueOnce(REGION).mockResolvedValueOnce(BOX_INFO);
    const result = await addBox();
    expect(result).toEqual({ ok: true, cancelled: false });
    expect(invoke).toHaveBeenCalledWith("start_region_selection", undefined);
    expect(invoke).toHaveBeenCalledWith("add_translation_box", { region: REGION });
    expect(useAppStore.getState().translationBoxes).toEqual([BOX_INFO]);
  });

  it("treats a cancelled selection as a non-error", async () => {
    invoke.mockRejectedValueOnce("state not initialized");
    const result = await addBox();
    expect(result).toEqual({ ok: false, cancelled: true });
    expect(useAppStore.getState().translationBoxes).toEqual([]);
  });

  it("surfaces an add failure through the shared store", async () => {
    invoke.mockResolvedValueOnce(REGION).mockRejectedValueOnce("limit exceeded");
    const result = await addBox();
    expect(result).toEqual({ ok: false, cancelled: false });
    expect(useAppStore.getState().status).toEqual({ error: "limit exceeded" });
  });
});

describe("editBox", () => {
  it("re-selects a region and updates the box region in the store", async () => {
    useAppStore.getState().setTranslationBoxes([BOX_INFO]);
    const newRegion = { monitor_id: "display-1", x: 1, y: 2, width: 30, height: 40 };
    invoke.mockResolvedValueOnce(newRegion).mockResolvedValueOnce(undefined);
    const result = await editBox(0);
    expect(result.ok).toBe(true);
    expect(invoke).toHaveBeenCalledWith("update_translation_box", { boxId: 0, region: newRegion });
    expect(useAppStore.getState().translationBoxes[0].region).toEqual(newRegion);
  });
});

describe("removeBox", () => {
  it("removes the box from the backend and the store", async () => {
    useAppStore.getState().setTranslationBoxes([BOX_INFO]);
    invoke.mockResolvedValueOnce(undefined);
    const result = await removeBox(0);
    expect(result.ok).toBe(true);
    expect(invoke).toHaveBeenCalledWith("remove_translation_box", { boxId: 0 });
    expect(useAppStore.getState().translationBoxes).toEqual([]);
  });
});

describe("start/stop/open", () => {
  it("starts and stops multi-box realtime", async () => {
    invoke.mockResolvedValueOnce(undefined);
    expect((await startMultiBox()).ok).toBe(true);
    expect(invoke).toHaveBeenCalledWith("start_multi_realtime", undefined);

    invoke.mockResolvedValueOnce(undefined);
    expect((await stopMultiBox()).ok).toBe(true);
    expect(invoke).toHaveBeenCalledWith("stop_multi_realtime", undefined);
  });

  it("stops a single box", async () => {
    invoke.mockResolvedValueOnce(undefined);
    expect((await stopSingleBox(5)).ok).toBe(true);
    expect(invoke).toHaveBeenCalledWith("stop_box", { boxId: 5 });
  });

  it("opens the result popup", async () => {
    invoke.mockResolvedValueOnce(undefined);
    expect((await openResultPopup()).ok).toBe(true);
    expect(invoke).toHaveBeenCalledWith("open_result_window", undefined);
  });
});

describe("hydrateBoxes", () => {
  it("loads the persisted box list into the store", async () => {
    invoke.mockResolvedValueOnce([BOX_INFO]);
    await hydrateBoxes();
    expect(invoke).toHaveBeenCalledWith("list_translation_boxes", undefined);
    expect(useAppStore.getState().translationBoxes).toEqual([BOX_INFO]);
  });
});

describe("multi-box store actions", () => {
  it("upserts a box without duplicating by box_id", () => {
    useAppStore.getState().upsertBox(BOX_INFO);
    useAppStore.getState().upsertBox({ ...BOX_INFO, region: { ...REGION, width: 99 } });
    const boxes = useAppStore.getState().translationBoxes;
    expect(boxes).toHaveLength(1);
    expect(boxes[0].region.width).toBe(99);
  });

  it("removes a box and its status/result", () => {
    useAppStore.getState().upsertBox(BOX_INFO);
    useAppStore.getState().setBoxStatus(0, "Running");
    useAppStore
      .getState()
      .setMultiBoxResult({ box_id: 0, color: "#FF6B6B", original_text: "hello", result: { translated_text: "x", provider_id: "mock", elapsed_ms: 1 }, timestamp: 1 });
    useAppStore.getState().removeBox(0);
    const state = useAppStore.getState();
    expect(state.translationBoxes).toEqual([]);
    expect(state.boxStatuses).toEqual({});
    expect(state.multiBoxResults).toEqual({});
  });

  it("updates a box region and keeps the other fields", () => {
    useAppStore.getState().upsertBox(BOX_INFO);
    const next = { monitor_id: "display-1", x: 9, y: 9, width: 10, height: 10 };
    useAppStore.getState().updateBoxRegion(0, next);
    const box = useAppStore.getState().translationBoxes[0];
    expect(box.region).toEqual(next);
    expect(box.color).toBe("#FF6B6B");
  });

  it("records the latest result per box and the single result", () => {
    useAppStore.getState().setSingleResult({ original_text: "hello", translated_text: "你好", timestamp: 1 });
    useAppStore
      .getState()
      .setMultiBoxResult({ box_id: 0, color: "#FF6B6B", original_text: "hello", result: { translated_text: "你好", provider_id: "mock", elapsed_ms: 1 }, timestamp: 1 });
    const state = useAppStore.getState();
    expect(state.singleResult?.translated_text).toBe("你好");
    expect(state.multiBoxResults[0].result.translated_text).toBe("你好");
    expect(state.multiBoxResults[0].original_text).toBe("hello");
  });

  it("resets all multi-box state", () => {
    useAppStore.getState().upsertBox(BOX_INFO);
    useAppStore.getState().setBoxStatus(0, "Running");
    useAppStore.getState().setSingleResult({ original_text: "a", translated_text: "b", timestamp: 1 });
    useAppStore.getState().resetMultiBox();
    const state = useAppStore.getState();
    expect(state.translationBoxes).toEqual([]);
    expect(state.boxStatuses).toEqual({});
    expect(state.multiBoxResults).toEqual({});
    expect(state.singleResult).toBeNull();
  });
});
