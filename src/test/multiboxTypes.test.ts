import { describe, expect, it } from "vitest";
import {
  DEFAULT_CONFIG,
  boxCountWarningText,
  boxStatusLabel,
  isAnyBoxRunning,
  isBoxError,
  isMultiBoxEngaged,
  isSingleLiveRunning,
  shouldWarnBoxCount,
} from "../types";

describe("multi-box type helpers", () => {
  it("recognizes the serialized Error variant", () => {
    expect(isBoxError({ Error: "capture failed" })).toBe(true);
    expect(isBoxError("Running")).toBe(false);
    expect(isBoxError("Stopped")).toBe(false);
  });

  it("maps every box status to a stable Chinese label", () => {
    expect(boxStatusLabel("Running")).toBe("运行中");
    expect(boxStatusLabel("Stopped")).toBe("已停止");
    expect(boxStatusLabel({ Error: "boom" })).toBe("错误");
  });

  it("engages multi-box mode only when running or producing results", () => {
    expect(isMultiBoxEngaged({}, 0)).toBe(false);
    expect(isMultiBoxEngaged({ 0: "Running" }, 0)).toBe(true);
    expect(isMultiBoxEngaged({ 0: "Stopped" }, 1)).toBe(true);
    expect(isMultiBoxEngaged({ 0: "Stopped" }, 0)).toBe(false);
    expect(isMultiBoxEngaged({ 0: { Error: "x" } }, 0)).toBe(false);
  });

  it("reports a running multi-box session only when any box is Running", () => {
    expect(isAnyBoxRunning({})).toBe(false);
    expect(isAnyBoxRunning({ 0: "Stopped", 1: "Stopped" })).toBe(false);
    expect(isAnyBoxRunning({ 0: "Stopped", 1: "Running" })).toBe(true);
    expect(isAnyBoxRunning({ 0: "Running" })).toBe(true);
    // Error 状态不视为运行中。
    expect(isAnyBoxRunning({ 0: { Error: "capture failed" } })).toBe(false);
  });

  it("reports a single-live session from mode and live config", () => {
    const config = {
      region: { monitor_id: "m0", x: 0, y: 0, width: 10, height: 10 },
      capture_interval_ms: 500,
      difference_threshold: 0.03,
    };
    expect(isSingleLiveRunning("live", config)).toBe(true);
    // 暂停中的单框会话（config 仍在）依然可停止。
    expect(isSingleLiveRunning("live", null)).toBe(false);
    expect(isSingleLiveRunning("single", config)).toBe(false);
    expect(isSingleLiveRunning("single", null)).toBe(false);
  });

  it("warns only when the count reaches a non-zero threshold", () => {
    expect(shouldWarnBoxCount(4, 4)).toBe(true);
    expect(shouldWarnBoxCount(5, 4)).toBe(true);
    expect(shouldWarnBoxCount(3, 4)).toBe(false);
    // threshold 0 disables the warning.
    expect(shouldWarnBoxCount(10, 0)).toBe(false);
  });

  it("builds the warning text from the threshold", () => {
    expect(boxCountWarningText(4)).toBe("翻译框过多可能导致卡顿，建议不超过 4 个");
  });
});

describe("multi-box config defaults", () => {
  it("ships an empty box list with backend-compatible limits", () => {
    expect(DEFAULT_CONFIG.translation_boxes).toEqual([]);
    expect(DEFAULT_CONFIG.max_boxes).toBe(8);
    expect(DEFAULT_CONFIG.warning_threshold).toBe(4);
  });

  it("keeps the config schema version in sync with the backend", () => {
    // 与 vtrans-config 的 CURRENT_CONFIG_VERSION（6，v6 新增多框字段）一致。
    expect(DEFAULT_CONFIG.version).toBe(6);
  });
});
