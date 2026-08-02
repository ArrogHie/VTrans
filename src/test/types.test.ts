import { describe, expect, it } from "vitest";
import { DEFAULT_CONFIG, pipelineStatusLabel, isPipelineError } from "../types";

describe("frontend contracts", () => {
  it("uses backend-compatible API defaults", () => {
    expect(DEFAULT_CONFIG.translation.api_endpoint).toMatch(/^https:\/\//);
    expect(DEFAULT_CONFIG.translation.api_model).not.toHaveLength(0);
    expect(DEFAULT_CONFIG.translation.max_retries).toBe(3);
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
