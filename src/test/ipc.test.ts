import { describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const {
  cancelRegionSelection,
  captureOnce,
  getAppConfig,
  setApiKey,
  setOcrLanguage,
  setProviderCredentials,
  setTranslationProvider,
  setSourceLanguage,
  setTargetLanguage,
  startLiveTranslation,
  updateFloatingBallAppearance,
  updateLiveRegion,
  updateResultWindowAppearance,
} = await import("../services/tauri");

describe("tauri IPC service", () => {
  it("passes the screen region under the command argument name", async () => {
    invoke.mockResolvedValueOnce({ lines: [], merged_text: "", detected_language: null, elapsed_ms: 1 });
    const region = { monitor_id: "display-1", x: 0, y: 10, width: 80, height: 40 };
    await captureOnce(region);
    expect(invoke).toHaveBeenCalledWith("capture_once", { region });
  });

  it("cancels a pending selector without arguments", async () => {
    invoke.mockResolvedValueOnce(undefined);
    await cancelRegionSelection();
    expect(invoke).toHaveBeenCalledWith("cancel_region_selection", undefined);
  });

  it("serializes live pipeline configuration as config", async () => {
    invoke.mockResolvedValueOnce(undefined);
    const config = { region: { monitor_id: "display-1", x: 1, y: 2, width: 3, height: 4 }, capture_interval_ms: 500, difference_threshold: 0.03 };
    await startLiveTranslation(config);
    expect(invoke).toHaveBeenCalledWith("start_live_translation", { config });
  });

  it("passes the confirmation mode alongside the region", async () => {
    invoke.mockResolvedValueOnce(undefined);
    const region = { monitor_id: "display-1", x: 0, y: 10, width: 80, height: 40 };
    await updateLiveRegion(region, "single");
    // 后端参数为 `region` / `mode`，Tauri 2 默认映射为同名 camelCase。
    expect(invoke).toHaveBeenCalledWith("update_live_region", { region, mode: "single" });

    invoke.mockResolvedValueOnce(undefined);
    await updateLiveRegion(region, "live");
    expect(invoke).toHaveBeenCalledWith("update_live_region", { region, mode: "live" });
  });

  it("passes the OCR language under the command argument name", async () => {
    invoke.mockResolvedValueOnce(undefined);
    await setOcrLanguage("ja");
    expect(invoke).toHaveBeenCalledWith("set_ocr_language", { language: "ja" });
  });

  it("passes the source language under the command argument name", async () => {
    invoke.mockResolvedValueOnce(undefined);
    await setSourceLanguage("ja");
    expect(invoke).toHaveBeenCalledWith("set_source_language", { language: "ja" });
  });

  it("passes the target language under the command argument name", async () => {
    invoke.mockResolvedValueOnce(undefined);
    await setTargetLanguage("en");
    expect(invoke).toHaveBeenCalledWith("set_target_language", { language: "en" });
  });

  it("passes the provider id under the Tauri camelCase argument name", async () => {
    invoke.mockResolvedValueOnce(undefined);
    await setTranslationProvider("local");
    // 后端参数名为 `provider_id`，Tauri 2 默认映射为 camelCase `providerId`。
    expect(invoke).toHaveBeenCalledWith("set_translation_provider", { providerId: "local" });
  });

  it("passes the API key under the Tauri camelCase argument name", async () => {
    invoke.mockResolvedValueOnce(undefined);
    await setApiKey("sk-test-1234");
    // 后端参数名为 `api_key`，Tauri 2 默认映射为 camelCase `apiKey`。
    expect(invoke).toHaveBeenCalledWith("set_api_key", { apiKey: "sk-test-1234" });
  });

  it("passes provider credentials with only the provided fields", async () => {
    invoke.mockResolvedValueOnce(undefined);
    await setProviderCredentials("openai", { apiKey: "sk-test-1234" });
    // 后端参数为 `provider_id` / `api_key` / `app_id` / `secret`，Tauri 2
    // 默认映射为 camelCase；未提供的可选字段不出现在载荷中。
    expect(invoke).toHaveBeenCalledWith("set_provider_credentials", {
      providerId: "openai",
      apiKey: "sk-test-1234",
    });
  });

  it("passes the baidu app id and secret as two credential fields", async () => {
    invoke.mockResolvedValueOnce(undefined);
    await setProviderCredentials("baidu", {
      appId: "2026081000000000",
      secret: "sk-secret",
    });
    expect(invoke).toHaveBeenCalledWith("set_provider_credentials", {
      providerId: "baidu",
      appId: "2026081000000000",
      secret: "sk-secret",
    });
  });

  it("requests the full application configuration without arguments", async () => {
    invoke.mockResolvedValueOnce({});
    await getAppConfig();
    expect(invoke).toHaveBeenCalledWith("get_app_config", undefined);
  });

  it("passes the mini-bar appearance under Tauri camelCase argument names", async () => {
    invoke.mockResolvedValueOnce(undefined);
    await updateResultWindowAppearance(0.8, 18);
    // 后端参数为 `opacity` / `font_size_px`，Tauri 2 默认映射为
    // camelCase `opacity` / `fontSizePx`。
    expect(invoke).toHaveBeenCalledWith("update_result_window_appearance", {
      opacity: 0.8,
      fontSizePx: 18,
    });
  });

  it("passes the floating ball appearance under Tauri camelCase argument names", async () => {
    invoke.mockResolvedValueOnce(undefined);
    await updateFloatingBallAppearance(0.75, 56);
    // 后端参数为 `opacity` / `size_px`，Tauri 2 默认映射为
    // camelCase `opacity` / `sizePx`。
    expect(invoke).toHaveBeenCalledWith("update_floating_ball_appearance", {
      opacity: 0.75,
      sizePx: 56,
    });
  });
});
