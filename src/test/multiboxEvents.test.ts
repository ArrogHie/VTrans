import { describe, expect, it, vi } from "vitest";

const listen = vi.fn();
const emit = vi.fn();
vi.mock("@tauri-apps/api/event", () => ({ emit, listen }));

const {
  MULTIBOX_BOX_ADDED,
  MULTIBOX_BOX_REMOVED,
  MULTIBOX_BOX_UPDATED,
  MULTIBOX_RESULT,
  MULTIBOX_STATUS,
  MULTIBOX_WARNING,
  TRANSLATION_SINGLE_RESULT,
  onMultiBoxBoxAdded,
  onMultiBoxBoxRemoved,
  onMultiBoxBoxUpdated,
  onMultiBoxResult,
  onMultiBoxStatus,
  onMultiBoxWarning,
  onSingleTranslationResult,
} = await import("../services/events");

describe("multi-box event service", () => {
  it("exposes stable multi-box event names", () => {
    expect(MULTIBOX_RESULT).toBe("multibox://result");
    expect(MULTIBOX_BOX_ADDED).toBe("multibox://box-added");
    expect(MULTIBOX_BOX_REMOVED).toBe("multibox://box-removed");
    expect(MULTIBOX_BOX_UPDATED).toBe("multibox://box-updated");
    expect(MULTIBOX_STATUS).toBe("multibox://status");
    expect(MULTIBOX_WARNING).toBe("multibox://warning");
    expect(TRANSLATION_SINGLE_RESULT).toBe("translation://single-result");
  });

  it("forwards the boxed translation result payload", async () => {
    const callback = vi.fn();
    const result = {
      box_id: 0,
      color: "#FF6B6B",
      result: { translated_text: "你好", provider_id: "mock", elapsed_ms: 3 },
      timestamp: 1_700_000_000_000,
    };
    listen.mockImplementationOnce(
      async (_name: string, handler: (event: { payload: unknown }) => void) => {
        handler({ payload: result });
        return vi.fn();
      },
    );
    await onMultiBoxResult(callback);
    expect(callback).toHaveBeenCalledWith(result);
  });

  it("forwards the box-added payload", async () => {
    const callback = vi.fn();
    const payload = { box_id: 1, color: "#4ECDC4", region: { monitor_id: "m0", x: 1, y: 2, width: 3, height: 4 } };
    listen.mockImplementationOnce(
      async (_name: string, handler: (event: { payload: unknown }) => void) => {
        handler({ payload });
        return vi.fn();
      },
    );
    await onMultiBoxBoxAdded(callback);
    expect(callback).toHaveBeenCalledWith(payload);
  });

  it("forwards the box-removed payload", async () => {
    const callback = vi.fn();
    listen.mockImplementationOnce(
      async (_name: string, handler: (event: { payload: unknown }) => void) => {
        handler({ payload: { box_id: 2 } });
        return vi.fn();
      },
    );
    await onMultiBoxBoxRemoved(callback);
    expect(callback).toHaveBeenCalledWith({ box_id: 2 });
  });

  it("forwards the box-updated payload", async () => {
    const callback = vi.fn();
    const payload = { box_id: 3, region: { monitor_id: "m0", x: 5, y: 6, width: 7, height: 8 } };
    listen.mockImplementationOnce(
      async (_name: string, handler: (event: { payload: unknown }) => void) => {
        handler({ payload });
        return vi.fn();
      },
    );
    await onMultiBoxBoxUpdated(callback);
    expect(callback).toHaveBeenCalledWith(payload);
  });

  it("forwards the box status payload", async () => {
    const callback = vi.fn();
    listen.mockImplementationOnce(
      async (_name: string, handler: (event: { payload: unknown }) => void) => {
        handler({ payload: { box_id: 4, status: "Running" } });
        return vi.fn();
      },
    );
    await onMultiBoxStatus(callback);
    expect(callback).toHaveBeenCalledWith({ box_id: 4, status: "Running" });
  });

  it("forwards the warning payload", async () => {
    const callback = vi.fn();
    listen.mockImplementationOnce(
      async (_name: string, handler: (event: { payload: unknown }) => void) => {
        handler({ payload: { current_count: 4, max_count: 8 } });
        return vi.fn();
      },
    );
    await onMultiBoxWarning(callback);
    expect(callback).toHaveBeenCalledWith({ current_count: 4, max_count: 8 });
  });

  it("forwards the single-result payload", async () => {
    const callback = vi.fn();
    const payload = { original_text: "hello", translated_text: "你好", timestamp: 1_700_000_000_000 };
    listen.mockImplementationOnce(
      async (_name: string, handler: (event: { payload: unknown }) => void) => {
        handler({ payload });
        return vi.fn();
      },
    );
    await onSingleTranslationResult(callback);
    expect(callback).toHaveBeenCalledWith(payload);
  });
});
