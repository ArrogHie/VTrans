import { describe, expect, it, vi } from "vitest";

const listen = vi.fn();
vi.mock("@tauri-apps/api/event", () => ({ listen }));

const {
  listenToEvent,
  onOcrCompleted,
  onPipelineError,
  onTranslationCompleted,
} = await import("../services/events");

describe("event service", () => {
  it("unwraps Tauri event payloads before invoking callbacks", async () => {
    const callback = vi.fn();
    const unlisten = vi.fn();
    listen.mockImplementationOnce(async (_name: string, handler: (event: { payload: unknown }) => void) => {
      handler({ payload: { message: "短错误", recoverable: true } });
      return unlisten;
    });
    const cleanup = await listenToEvent("pipeline_error", callback);
    expect(callback).toHaveBeenCalledWith({ message: "短错误", recoverable: true });
    cleanup();
    expect(unlisten).toHaveBeenCalledOnce();
  });

  it("passes region_selected payloads as raw screen regions", async () => {
    const callback = vi.fn();
    listen.mockImplementationOnce(async (_name: string, handler: (event: { payload: unknown }) => void) => {
      handler({ payload: { monitor_id: "display-1", x: 0, y: 0, width: 10, height: 20 } });
      return vi.fn();
    });
    await listenToEvent("region_selected", callback);
    expect(callback).toHaveBeenCalledWith({ monitor_id: "display-1", x: 0, y: 0, width: 10, height: 20 });
  });

  it("onOcrCompleted unwraps the result payload", async () => {
    const callback = vi.fn();
    listen.mockImplementationOnce(async (_name: string, handler: (event: { payload: unknown }) => void) => {
      handler({ payload: { result: { lines: [], merged_text: "hello", detected_language: null, elapsed_ms: 3 } } });
      return vi.fn();
    });
    await onOcrCompleted(callback);
    expect(callback).toHaveBeenCalledWith(
      expect.objectContaining({ merged_text: "hello" }),
    );
  });

  it("onTranslationCompleted unwraps the result payload", async () => {
    const callback = vi.fn();
    listen.mockImplementationOnce(async (_name: string, handler: (event: { payload: unknown }) => void) => {
      handler({ payload: { result: { translated_text: "你好", provider_id: "mock", elapsed_ms: 7 } } });
      return vi.fn();
    });
    await onTranslationCompleted(callback);
    expect(callback).toHaveBeenCalledWith(
      expect.objectContaining({ translated_text: "你好" }),
    );
  });

  it("onPipelineError forwards the message string", async () => {
    const callback = vi.fn();
    listen.mockImplementationOnce(async (_name: string, handler: (event: { payload: unknown }) => void) => {
      handler({ payload: { message: "识别超时", recoverable: true } });
      return vi.fn();
    });
    await onPipelineError(callback);
    expect(callback).toHaveBeenCalledWith("识别超时");
  });
});
