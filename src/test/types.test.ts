import { describe, expect, it } from "vitest";
import {
  DEFAULT_CONFIG,
  normalizeProviderId,
  pipelineStatusLabel,
  isPipelineError,
} from "../types";

describe("frontend contracts", () => {
  it("uses backend-compatible API defaults", () => {
    expect(DEFAULT_CONFIG.translation.api_endpoint).toMatch(/^https:\/\//);
    expect(DEFAULT_CONFIG.translation.api_model).not.toHaveLength(0);
    expect(DEFAULT_CONFIG.translation.max_retries).toBe(3);
    expect(DEFAULT_CONFIG.hotkeys.live_translate).toBe("Alt+Shift+R");
  });

  it("matches the model verification report shape", () => {
    const report = { checked: 2, passed: 2, failed: [] };
    expect(report.failed.length === 0).toBe(true);
  });
});

describe("pipeline status helpers", () => {
  it("maps stable backend status codes to UI labels", () => {
    expect(pipelineStatusLabel("ocr_in_progress")).toBe("识别中");
    expect(pipelineStatusLabel("completed")).toBe("已完成");
  });

  it("recognizes serialized error variants", () => {
    const status = { error: "模型缺失" } as const;
    expect(isPipelineError(status)).toBe(true);
    expect(pipelineStatusLabel(status)).toBe("模型缺失");
  });
});

describe("normalizeProviderId", () => {
  it("maps the local ONNX runtime id to the local config value", () => {
    expect(normalizeProviderId("local-onnx")).toBe("local");
  });

  it("keeps the api runtime id unchanged", () => {
    expect(normalizeProviderId("api")).toBe("api");
  });

  it("falls back to api for unknown runtime ids", () => {
    expect(normalizeProviderId("unknown-provider")).toBe("api");
  });
});
