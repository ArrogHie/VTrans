import { describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const {
  cancelRegionSelection,
  captureOnce,
  getAppConfig,
  setApiKey,
  setTranslationProvider,
  setSourceLanguage,
  setTargetLanguage,
  startLiveTranslation,
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

  it("requests the full application configuration without arguments", async () => {
    invoke.mockResolvedValueOnce({});
    await getAppConfig();
    expect(invoke).toHaveBeenCalledWith("get_app_config", undefined);
  });
});
