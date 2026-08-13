import { describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const {
  addTranslationBox,
  listTranslationBoxes,
  openResultWindow,
  removeTranslationBox,
  startMultiRealtime,
  stopBox,
  stopMultiRealtime,
  updateTranslationBox,
} = await import("../services/tauri");

const REGION = { monitor_id: "display-1", x: 0, y: 10, width: 80, height: 40 };
const BOX_INFO = { box_id: 0, region: REGION, color: "#FF6B6B" };

describe("multi-box IPC service", () => {
  it("adds a translation box and returns its info", async () => {
    invoke.mockResolvedValueOnce(BOX_INFO);
    const info = await addTranslationBox(REGION);
    expect(info).toEqual(BOX_INFO);
    expect(invoke).toHaveBeenCalledWith("add_translation_box", { region: REGION });
  });

  it("removes a box under the Tauri camelCase boxId argument", async () => {
    invoke.mockResolvedValueOnce(undefined);
    await removeTranslationBox(7);
    // 后端参数为 `box_id`，Tauri 2 默认映射为 camelCase `boxId`。
    expect(invoke).toHaveBeenCalledWith("remove_translation_box", { boxId: 7 });
  });

  it("updates a box region under boxId + region", async () => {
    invoke.mockResolvedValueOnce(undefined);
    await updateTranslationBox(3, REGION);
    expect(invoke).toHaveBeenCalledWith("update_translation_box", { boxId: 3, region: REGION });
  });

  it("lists translation boxes without arguments", async () => {
    invoke.mockResolvedValueOnce([BOX_INFO]);
    const boxes = await listTranslationBoxes();
    expect(boxes).toEqual([BOX_INFO]);
    expect(invoke).toHaveBeenCalledWith("list_translation_boxes", undefined);
  });

  it("starts and stops multi-box real-time translation without arguments", async () => {
    invoke.mockResolvedValueOnce(undefined);
    await startMultiRealtime();
    expect(invoke).toHaveBeenCalledWith("start_multi_realtime", undefined);

    invoke.mockResolvedValueOnce(undefined);
    await stopMultiRealtime();
    expect(invoke).toHaveBeenCalledWith("stop_multi_realtime", undefined);
  });

  it("stops a single box under the boxId argument", async () => {
    invoke.mockResolvedValueOnce(undefined);
    await stopBox(9);
    expect(invoke).toHaveBeenCalledWith("stop_box", { boxId: 9 });
  });

  it("opens the result popup without arguments", async () => {
    invoke.mockResolvedValueOnce(undefined);
    await openResultWindow();
    expect(invoke).toHaveBeenCalledWith("open_result_window", undefined);
  });
});
