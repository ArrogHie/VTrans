import { describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const {
  cancelTranslationModelDownload,
  deleteTranslationModel,
  downloadTranslationModel,
  getModelStatus,
  retryModelSetup,
} = await import("../services/tauri");

describe("model download IPC service", () => {
  it("starts the translation model download without arguments", async () => {
    invoke.mockResolvedValueOnce(undefined);
    await downloadTranslationModel();
    expect(invoke).toHaveBeenCalledWith("download_translation_model", undefined);
  });

  it("cancels a translation model download without arguments", async () => {
    invoke.mockResolvedValueOnce(undefined);
    await cancelTranslationModelDownload();
    expect(invoke).toHaveBeenCalledWith("cancel_translation_model_download", undefined);
  });

  it("deletes the translation model without arguments", async () => {
    invoke.mockResolvedValueOnce(undefined);
    await deleteTranslationModel();
    expect(invoke).toHaveBeenCalledWith("delete_translation_model", undefined);
  });

  it("fetches the model status snapshot and returns the report", async () => {
    const report = {
      entries: [{ id: "opus-mt-en-zh-int8", state: "missing", optional: true }],
      ocr_ready: true,
      translation_ready: false,
    };
    invoke.mockResolvedValueOnce(report);
    await expect(getModelStatus()).resolves.toEqual(report);
    expect(invoke).toHaveBeenCalledWith("get_model_status", undefined);
  });

  it("re-runs the model setup and returns the fresh report", async () => {
    const report = {
      entries: [
        { id: "ppocr-det-v6", state: "ready", optional: false },
        { id: "opus-mt-en-zh-int8", state: "ready", optional: true },
      ],
      ocr_ready: true,
      translation_ready: true,
    };
    invoke.mockResolvedValueOnce(report);
    await expect(retryModelSetup()).resolves.toEqual(report);
    expect(invoke).toHaveBeenCalledWith("retry_model_setup", undefined);
  });
});
