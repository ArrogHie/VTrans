import { describe, expect, it, vi } from "vitest";

const listen = vi.fn();
vi.mock("@tauri-apps/api/event", () => ({ listen }));

const { listenToEvent } = await import("../services/events");

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
});
