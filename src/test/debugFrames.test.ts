import { describe, expect, it, vi } from "vitest";

const listen = vi.fn();
vi.mock("@tauri-apps/api/event", () => ({ listen }));

const {
  createLatestFrameStore,
  DEBUG_FRAME_UPDATED_EVENT,
  subscribeToDebugFrames,
} = await import("../services/debugFrames");

const frame = {
  image: "thumbnail-bytes",
  region: { monitor_id: "\\.\\DISPLAY1", x: 400, y: 300, width: 800, height: 400 },
  frame_index: 1,
  timestamp_ms: 1785911487496,
};

describe("createLatestFrameStore", () => {
  it("starts empty", () => {
    const store = createLatestFrameStore<number>();
    expect(store.read()).toBeNull();
  });

  it("overwrites the previous value on every push", () => {
    const store = createLatestFrameStore<number>();
    store.push(1);
    store.push(2);
    store.push(3);
    // 只保留最新值，绝不累积。
    expect(store.read()).toBe(3);
  });

  it("keeps only the newest debug frame after a burst", () => {
    const store = createLatestFrameStore<typeof frame>();
    for (let index = 1; index <= 100; index += 1) {
      store.push({ ...frame, frame_index: index });
    }
    expect(store.read()).toEqual({ ...frame, frame_index: 100 });
  });

  it("releases the cached value on clear", () => {
    const store = createLatestFrameStore<number>();
    store.push(7);
    store.clear();
    expect(store.read()).toBeNull();
  });
});

describe("subscribeToDebugFrames", () => {
  it("subscribes to the debug_frame_updated event name", async () => {
    listen.mockResolvedValueOnce(vi.fn());
    await subscribeToDebugFrames(() => {});
    expect(listen).toHaveBeenCalledWith(DEBUG_FRAME_UPDATED_EVENT, expect.any(Function));
    expect(DEBUG_FRAME_UPDATED_EVENT).toBe("debug_frame_updated");
  });

  it("forwards the unwrapped payload to the callback", async () => {
    const callback = vi.fn();
    listen.mockImplementationOnce(
      async (_name: string, handler: (event: { payload: unknown }) => void) => {
        handler({ payload: frame });
        return vi.fn();
      },
    );
    await subscribeToDebugFrames(callback);
    expect(callback).toHaveBeenCalledWith(frame);
  });

  it("returns a cleanup function that removes the listener", async () => {
    const unlisten = vi.fn();
    listen.mockResolvedValueOnce(unlisten);
    const cleanup = await subscribeToDebugFrames(() => {});
    cleanup();
    expect(unlisten).toHaveBeenCalledOnce();
  });
});
