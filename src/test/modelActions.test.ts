import { beforeEach, describe, expect, it, vi } from "vitest";

const tauriMocks = vi.hoisted(() => ({
  downloadTranslationModel: vi.fn(),
  cancelTranslationModelDownload: vi.fn(),
  deleteTranslationModel: vi.fn(),
  getModelStatus: vi.fn(),
  retryModelSetup: vi.fn(),
  loadLocalModels: vi.fn(),
}));

vi.mock("../services/tauri", () => tauriMocks);

const {
  applyModelDownloadProgress,
  cancelModelDownload,
  deleteModel,
  downloadModel,
  refreshModelStatus,
  retryModelSetup,
  verifyLocalModels,
} = await import("../services/modelActions");
const { useAppStore } = await import("../stores/appStore");
import type { ModelStatusReport } from "../types";

const READY_REPORT: ModelStatusReport = {
  entries: [
    { id: "ppocr-det-v6", state: "ready", optional: false },
    { id: "opus-mt-en-zh-int8", state: "ready", optional: true },
  ],
  ocr_ready: true,
  translation_ready: true,
};

const MISSING_REPORT: ModelStatusReport = {
  entries: [
    { id: "ppocr-det-v6", state: "ready", optional: false },
    { id: "opus-mt-en-zh-int8", state: "missing", optional: true },
  ],
  ocr_ready: true,
  translation_ready: false,
};

beforeEach(() => {
  vi.clearAllMocks();
  useAppStore.setState(useAppStore.getInitialState());
});

describe("refreshModelStatus", () => {
  it("mirrors the fetched report into the store and returns it", async () => {
    tauriMocks.getModelStatus.mockResolvedValueOnce(MISSING_REPORT);
    await expect(refreshModelStatus()).resolves.toEqual(MISSING_REPORT);
    expect(useAppStore.getState().modelStatus).toEqual(MISSING_REPORT);
  });

  it("clears the download marker and progress once the model is ready", async () => {
    useAppStore.getState().setTranslationModelDownloading(true);
    useAppStore.getState().setModelDownloadProgress({ bytes: 10, total: 20, fraction: 0.5 });
    tauriMocks.getModelStatus.mockResolvedValueOnce(READY_REPORT);
    await refreshModelStatus();
    expect(useAppStore.getState().translationModelDownloading).toBe(false);
    expect(useAppStore.getState().modelDownloadProgress).toBeNull();
  });

  it("keeps the download marker while the model is still missing", async () => {
    useAppStore.getState().setTranslationModelDownloading(true);
    tauriMocks.getModelStatus.mockResolvedValueOnce(MISSING_REPORT);
    await refreshModelStatus();
    expect(useAppStore.getState().translationModelDownloading).toBe(true);
  });
});

describe("downloadModel", () => {
  it("marks the download in flight, invokes the command, then refreshes the terminal status", async () => {
    tauriMocks.downloadTranslationModel.mockResolvedValueOnce(undefined);
    tauriMocks.getModelStatus.mockResolvedValueOnce(READY_REPORT);
    const promise = downloadModel();
    // 发起即置「下载中」，不等 promise 结算。
    expect(useAppStore.getState().translationModelDownloading).toBe(true);
    await expect(promise).resolves.toEqual(READY_REPORT);
    expect(tauriMocks.downloadTranslationModel).toHaveBeenCalledTimes(1);
    expect(tauriMocks.getModelStatus).toHaveBeenCalledTimes(1);
    // 下载 promise resolve 后重新 get_model_status 刷新终态。
    const order = [tauriMocks.downloadTranslationModel, tauriMocks.getModelStatus].map(
      (mock) => mock.mock.invocationCallOrder[0],
    );
    expect(order[0]).toBeLessThan(order[1]);
    expect(useAppStore.getState().translationModelDownloading).toBe(false);
    expect(useAppStore.getState().modelStatus).toEqual(READY_REPORT);
  });

  it("clears the download marker when the command fails", async () => {
    tauriMocks.downloadTranslationModel.mockRejectedValueOnce(new Error("网络错误"));
    await expect(downloadModel()).rejects.toThrow("网络错误");
    expect(useAppStore.getState().translationModelDownloading).toBe(false);
    // 失败不调用状态刷新（由调用方决定补救）。
    expect(tauriMocks.getModelStatus).not.toHaveBeenCalled();
  });
});

describe("cancelModelDownload", () => {
  it("cancels, clears the marker, and refreshes the terminal status", async () => {
    tauriMocks.cancelTranslationModelDownload.mockResolvedValueOnce(undefined);
    tauriMocks.getModelStatus.mockResolvedValueOnce(MISSING_REPORT);
    useAppStore.getState().setTranslationModelDownloading(true);
    await expect(cancelModelDownload()).resolves.toEqual(MISSING_REPORT);
    expect(useAppStore.getState().translationModelDownloading).toBe(false);
    expect(tauriMocks.getModelStatus).toHaveBeenCalledTimes(1);
  });

  it("still clears the marker when the cancel command fails", async () => {
    tauriMocks.cancelTranslationModelDownload.mockRejectedValueOnce(new Error("无进行中的下载"));
    useAppStore.getState().setTranslationModelDownloading(true);
    await expect(cancelModelDownload()).rejects.toThrow("无进行中的下载");
    expect(useAppStore.getState().translationModelDownloading).toBe(false);
  });
});

describe("deleteModel", () => {
  it("deletes, clears the marker, and refreshes the terminal status", async () => {
    tauriMocks.deleteTranslationModel.mockResolvedValueOnce(undefined);
    tauriMocks.getModelStatus.mockResolvedValueOnce(MISSING_REPORT);
    useAppStore.getState().setTranslationModelDownloading(true);
    await expect(deleteModel()).resolves.toEqual(MISSING_REPORT);
    expect(tauriMocks.deleteTranslationModel).toHaveBeenCalledTimes(1);
    expect(useAppStore.getState().translationModelDownloading).toBe(false);
  });
});

describe("retryModelSetup", () => {
  it("re-runs the model setup and mirrors the fresh report into the store", async () => {
    tauriMocks.retryModelSetup.mockResolvedValueOnce(READY_REPORT);
    await expect(retryModelSetup()).resolves.toEqual(READY_REPORT);
    expect(useAppStore.getState().modelStatus).toEqual(READY_REPORT);
  });
});

describe("verifyLocalModels", () => {
  it("reports 本地模型校验通过 when nothing failed and nothing was skipped", async () => {
    tauriMocks.loadLocalModels.mockResolvedValueOnce({
      checked: 3,
      passed: 3,
      skipped: [],
      failed: [],
    });
    await expect(verifyLocalModels()).resolves.toBe("本地模型校验通过");
  });

  it("flags the translation model as not installed when only optional entries were skipped", async () => {
    // 发行部署后：翻译模型未下载 → skipped 非空且 failed 为空，不再误报「校验通过」。
    tauriMocks.loadLocalModels.mockResolvedValueOnce({
      checked: 3,
      passed: 2,
      skipped: ["opus-mt-en-zh-int8"],
      failed: [],
    });
    await expect(verifyLocalModels()).resolves.toBe(
      "OCR 模型校验通过，翻译模型未安装（请在设置中下载）",
    );
  });

  it("reports 本地模型需要检查 when any entry failed, even alongside skipped entries", async () => {
    tauriMocks.loadLocalModels.mockResolvedValueOnce({
      checked: 3,
      passed: 1,
      skipped: ["opus-mt-en-zh-int8"],
      failed: ["ppocr-rec-v5"],
    });
    await expect(verifyLocalModels()).resolves.toBe("本地模型需要检查");
  });

  it("lets IPC failures propagate so the caller can render the error message", async () => {
    tauriMocks.loadLocalModels.mockRejectedValueOnce(new Error("模型校验失败"));
    await expect(verifyLocalModels()).rejects.toThrow("模型校验失败");
  });
});

describe("applyModelDownloadProgress", () => {
  it("stores the progress and marks the download in flight while the model is missing", () => {
    useAppStore.getState().setModelStatus(MISSING_REPORT);
    applyModelDownloadProgress({ bytes: 10, total: 40, fraction: 0.25 });
    const state = useAppStore.getState();
    expect(state.modelDownloadProgress).toEqual({ bytes: 10, total: 40, fraction: 0.25 });
    expect(state.translationModelDownloading).toBe(true);
  });

  it("marks the download in flight before any report has been fetched", () => {
    applyModelDownloadProgress({ bytes: 5, total: 20, fraction: 0.25 });
    expect(useAppStore.getState().translationModelDownloading).toBe(true);
  });

  it("does not re-mark in flight once the model is known ready (late event)", () => {
    useAppStore.getState().setModelStatus(READY_REPORT);
    applyModelDownloadProgress({ bytes: 20, total: 20, fraction: 1 });
    const state = useAppStore.getState();
    expect(state.translationModelDownloading).toBe(false);
    expect(state.modelDownloadProgress?.fraction).toBe(1);
  });
});
