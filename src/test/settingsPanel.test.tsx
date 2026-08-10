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

  it("allows an empty model for google where it is optional", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.translation.provider = "google";
    config.translation.api_model = "   ";
    expect(validateSettings(config)).toBeNull();
  });

  it("rejects an empty azure region but allows an absent one", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.translation.provider = "azure";
    config.translation.region = "   ";
    expect(validateSettings(config)).toContain("区域");

    config.translation.region = null;
    expect(validateSettings(config)).toBeNull();
  });

  it("rejects a baidu configuration without an app id", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.translation.provider = "baidu";
    config.translation.app_id = null;
    expect(validateSettings(config)).toContain("百度 APP ID");
  });

  it("ignores the endpoint and model for the local provider", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.translation.provider = "local";
    config.translation.api_endpoint = "ftp://example.invalid";
    config.translation.api_model = "";
    expect(validateSettings(config)).toBeNull();
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

  it("rejects a floating ball opacity outside 0.3..1.0", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.floating_ball.opacity = 0.2;
    expect(validateSettings(config)).toContain("悬浮球透明度");

    config.floating_ball.opacity = 1.1;
    expect(validateSettings(config)).toContain("悬浮球透明度");
  });

  it("rejects a floating ball size outside 32..72 or non-integer", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.floating_ball.size_px = 24;
    expect(validateSettings(config)).toContain("悬浮球大小");

    config.floating_ball.size_px = 80;
    expect(validateSettings(config)).toContain("悬浮球大小");

    config.floating_ball.size_px = 48.5;
    expect(validateSettings(config)).toContain("悬浮球大小");
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
    expect(html).toContain("悬浮球透明度");
    expect(html).toContain("悬浮球大小");
    expect(html).toContain("弹窗背景透明度");
    expect(html).toContain("弹窗字体大小");
  });

  it("renders the deepL plan selector and endpoint for the deepl provider", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.translation.provider = "deepl";
    config.translation.api_endpoint = "https://api-free.deepl.com/v2/translate";
    const html = renderToStaticMarkup(
      <SettingsPanel config={config} onSaved={() => {}} onClose={() => {}} />,
    );
    expect(html).toContain("DeepL 套餐");
    expect(html).toContain("DeepL Free");
    expect(html).toContain("DeepL Pro");
    expect(html).toContain("自定义端点");
    expect(html).toContain('value="https://api-free.deepl.com/v2/translate"');
    expect(html).toContain("保存 Key");
  });

  it("renders an optional model input for the google provider", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.translation.provider = "google";
    const html = renderToStaticMarkup(
      <SettingsPanel config={config} onSaved={() => {}} onClose={() => {}} />,
    );
    expect(html).toContain("API 模型名（可选）");
    expect(html).toContain("保存 Key");
  });

  it("renders the azure region input alongside the endpoint", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.translation.provider = "azure";
    config.translation.region = "eastasia";
    const html = renderToStaticMarkup(
      <SettingsPanel config={config} onSaved={() => {}} onClose={() => {}} />,
    );
    expect(html).toContain("区域（如 eastasia）");
    expect(html).toContain('value="eastasia"');
    expect(html).toContain("保存 Key");
  });

  it("renders the baidu app id and secret form without a key-only flow", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.translation.provider = "baidu";
    config.translation.app_id = "2026081000000000";
    const html = renderToStaticMarkup(
      <SettingsPanel config={config} onSaved={() => {}} onClose={() => {}} />,
    );
    expect(html).toContain("百度 APP ID");
    expect(html).toContain("百度 Secret");
    expect(html).toContain("保存凭据");
    expect(html).not.toContain("保存 Key");
    expect(html).not.toContain("API 模型名");
  });

  it("hides cloud api fields when the local provider is selected", () => {
    const config = structuredClone(DEFAULT_CONFIG);
    config.translation.provider = "local";
    const html = renderToStaticMarkup(
      <SettingsPanel config={config} onSaved={() => {}} onClose={() => {}} />,
    );
    expect(html).toContain("本地 ONNX 模型不使用云端 API 参数");
    expect(html).not.toContain("API 端点");
    expect(html).not.toContain('placeholder="sk-..."');
    expect(html).not.toContain("保存 Key");
  });
});
