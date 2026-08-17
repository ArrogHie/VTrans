import { describe, expect, it, vi } from "vitest";

const listen = vi.fn();
vi.mock("@tauri-apps/api/event", () => ({ listen }));

const { MODEL_DOWNLOAD_PROGRESS, onModelDownloadProgress } = await import("../services/events");

describe("model download progress event", () => {
  it("exposes the stable backend event name", () => {
    expect(MODEL_DOWNLOAD_PROGRESS).toBe("model_download_progress");
  });

  it("registers a listener on the backend event and unwraps the payload", async () => {
    const callback = vi.fn();
    listen.mockImplementationOnce(
      async (name: string, handler: (event: { payload: unknown }) => void) => {
        expect(name).toBe("model_download_progress");
        handler({ payload: { bytes: 1024, total: 4096, fraction: 0.25 } });
        return vi.fn();
      },
    );
    await onModelDownloadProgress(callback);
    // 事件 payload 字段与 Rust DTO 一致（snake_case，无解包包装）。
    expect(callback).toHaveBeenCalledWith({ bytes: 1024, total: 4096, fraction: 0.25 });
  });

  it("returns the unlisten function for cleanup", async () => {
    const callback = vi.fn();
    const unlisten = vi.fn();
    listen.mockImplementationOnce(
      async (_name: string, _handler: (event: { payload: unknown }) => void) => unlisten,
    );
    const cleanup = await onModelDownloadProgress(callback);
    cleanup();
    expect(unlisten).toHaveBeenCalledOnce();
  });
});
