import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { SettingsPanel, validateSettings } from "../components/SettingsPanel";
import { DEFAULT_CONFIG } from "../types";

describe("validateSettings", () => {
  it("accepts the default configuration", () => {
    expect(validateSettings(structuredClone(DEFAULT_CONFIG))).toBeNull();
  });

  it("rejects a non-positive capture interval", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.capture.interval_ms = 0;
    expect(validateSettings(config)).toContain("捕获间隔");
  });

  it("rejects a threshold outside 0..1", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.capture.difference_threshold = 1.5;
    expect(validateSettings(config)).toContain("差异阈值");
  });

  it("rejects a non-http api endpoint", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.translation.api_endpoint = "ftp://example.com";
    expect(validateSettings(config)).toContain("http");
  });

  it("rejects an empty api model", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.translation.api_model = "   ";
    expect(validateSettings(config)).toContain("模型名");
  });

  it("rejects a negative retry count", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.translation.max_retries = -1;
    expect(validateSettings(config)).toContain("重试");
  });

  it("rejects an empty hotkey", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.hotkeys.stop_live = "";
    expect(validateSettings(config)).toContain("快捷键");
  });

  it("rejects an opacity outside 0.3..1.0", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.result_window.opacity = 0.2;
    expect(validateSettings(config)).toContain("透明度");

    config.result_window.opacity = 1.1;
    expect(validateSettings(config)).toContain("透明度");
  });

  it("rejects a font size outside 12..24 or non-integer", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.result_window.font_size_px = 11;
    expect(validateSettings(config)).toContain("字体大小");

    config.result_window.font_size_px = 25;
    expect(validateSettings(config)).toContain("字体大小");

    config.result_window.font_size_px = 14.5;
    expect(validateSettings(config)).toContain("字体大小");
  });
});

describe("SettingsPanel", () => {
  it("renders editable fields initialized from the provided config", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.translation.api_endpoint = "https://example.com/v1/chat";
    config.translation.api_model = "test-model";
    const html = renderToStaticMarkup(
      <SettingsPanel config={config} onSaved={() => {}} onClose={() => {}} />,
    );
    expect(html).toContain('value="https://example.com/v1/chat"');
    expect(html).toContain('value="test-model"');
    expect(html).toContain("API Key");
    expect(html).toContain("保存 Key");
    expect(html).toContain("保存设置");
    expect(html).toContain("显示悬浮球");
    expect(html).toContain("弹窗背景透明度");
    expect(html).toContain("弹窗字体大小");
  });
});
