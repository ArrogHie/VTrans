import { describe, expect, it } from "vitest";
import {
  DEFAULT_CONFIG,
  isLocalPairSupported,
  normalizeProviderId,
  pipelineStatusLabel,
  isPipelineError,
} from "../types";

describe("frontend contracts", () => {
  it("uses backend-compatible API defaults", () => {
    expect(DEFAULT_CONFIG.translation.api_endpoint).toMatch(/^https:\/\//);
    expect(DEFAULT_CONFIG.translation.api_model).not.toHaveLength(0);
    expect(DEFAULT_CONFIG.translation.max_retries).toBe(3);
    expect(DEFAULT_CONFIG.translation.provider).toBe("openai");
    expect(DEFAULT_CONFIG.translation.region).toBeNull();
    expect(DEFAULT_CONFIG.translation.app_id).toBeNull();
    expect(DEFAULT_CONFIG.translation.quality).toBe("fast");
    expect(DEFAULT_CONFIG.hotkeys.live_translate).toBe("Alt+Shift+R");
  });

  it("uses the backend-compatible result window appearance defaults", () => {
    expect(DEFAULT_CONFIG.result_window.always_on_top).toBe(true);
    expect(DEFAULT_CONFIG.result_window.opacity).toBe(0.95);
    expect(DEFAULT_CONFIG.result_window.font_size_px).toBe(14);
  });

  it("uses the backend-compatible floating ball defaults", () => {
    expect(DEFAULT_CONFIG.floating_ball.enabled).toBe(false);
    expect(DEFAULT_CONFIG.floating_ball.opacity).toBe(1);
    expect(DEFAULT_CONFIG.floating_ball.size_px).toBe(48);
  });

  it("matches the backend config schema version", () => {
    // 与 vtrans-config 的 CURRENT_CONFIG_VERSION（6）保持一致：任何
    // “未水合即保存”路径都必须携带后端接受的版本，否则 save_settings
    // 会被校验拒绝。
    expect(DEFAULT_CONFIG.version).toBe(6);
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

  it("passes every cloud provider runtime id through unchanged", () => {
    expect(normalizeProviderId("openai")).toBe("openai");
    expect(normalizeProviderId("deepl")).toBe("deepl");
    expect(normalizeProviderId("google")).toBe("google");
    expect(normalizeProviderId("azure")).toBe("azure");
    expect(normalizeProviderId("baidu")).toBe("baidu");
  });

  it("falls back to the openai default for unknown runtime ids", () => {
    expect(normalizeProviderId("unknown-provider")).toBe("openai");
    expect(normalizeProviderId("api")).toBe("openai");
    expect(normalizeProviderId("")).toBe("openai");
  });
});

describe("isLocalPairSupported", () => {
  it("allows any pair on cloud providers", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.translation.source_language = "ja";
    expect(isLocalPairSupported(config)).toBe(true);

    for (const provider of ["openai", "deepl", "google", "azure", "baidu"] as const) {
      config.translation.provider = provider;
      config.translation.source_language = "ja";
      expect(isLocalPairSupported(config)).toBe(true);
    }
  });

  it("allows en -> zh-CN on the local provider", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.translation.provider = "local";
    config.translation.source_language = "en";
    config.translation.target_language = "zh-CN";
    expect(isLocalPairSupported(config)).toBe(true);
  });

  it("flags unsupported source languages on the local provider", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.translation.provider = "local";
    config.translation.source_language = "ja";
    expect(isLocalPairSupported(config)).toBe(false);
  });

  it("flags auto source on the local provider", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.translation.provider = "local";
    config.translation.source_language = "auto";
    expect(isLocalPairSupported(config)).toBe(false);
  });

  it("flags non-zh-CN targets on the local provider", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.translation.provider = "local";
    config.translation.source_language = "en";
    config.translation.target_language = "ja";
    expect(isLocalPairSupported(config)).toBe(false);
  });
});
