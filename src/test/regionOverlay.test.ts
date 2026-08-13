import { beforeEach, describe, expect, it, vi } from "vitest";

const getByLabel = vi.fn();
const emitMock = vi.fn();
const availableMonitorsMock = vi.fn();

vi.mock("@tauri-apps/api/webviewWindow", () => ({
  WebviewWindow: { getByLabel: (...args: unknown[]) => getByLabel(...args) },
}));
vi.mock("@tauri-apps/api/event", () => ({
  emit: (...args: unknown[]) => emitMock(...args),
}));
vi.mock("@tauri-apps/api/window", () => ({
  availableMonitors: (...args: unknown[]) => availableMonitorsMock(...args),
}));

const { hideRegionOverlay, showMultiBoxOverlay, showRegionOverlay } = await import("../services/regionOverlay");

function createWindowMock() {
  return {
    setPosition: vi.fn().mockResolvedValue(undefined),
    setSize: vi.fn().mockResolvedValue(undefined),
    setIgnoreCursorEvents: vi.fn().mockResolvedValue(undefined),
    show: vi.fn().mockResolvedValue(undefined),
    hide: vi.fn().mockResolvedValue(undefined),
  };
}

const REGION = {
  monitor_id: "\\\\.\\DISPLAY2",
  x: 120,
  y: 240,
  width: 480,
  height: 320,
};

const MONITORS = [
  { name: "\\\\.\\DISPLAY1", position: { x: 0, y: 0 }, size: { width: 1920, height: 1080 }, scaleFactor: 1 },
  { name: "\\\\.\\DISPLAY2", position: { x: 1920, y: 0 }, size: { width: 2560, height: 1440 }, scaleFactor: 1 },
];

describe("regionOverlay", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    availableMonitorsMock.mockResolvedValue(MONITORS);
  });

  it("positions the overlay on the region's monitor and publishes the region", async () => {
    const windowMock = createWindowMock();
    getByLabel.mockResolvedValue(windowMock);

    await showRegionOverlay(REGION);

    expect(getByLabel).toHaveBeenCalledWith("overlay");
    // The window covers the whole monitor; the border is drawn at the
    // region's monitor-relative offset inside the window, so the window
    // origin is the monitor origin, not the region origin.
    expect(windowMock.setPosition).toHaveBeenCalledWith(expect.objectContaining({ x: 1920, y: 0 }));
    expect(windowMock.setSize).toHaveBeenCalledWith(expect.objectContaining({ width: 2560, height: 1440 }));
    expect(windowMock.setIgnoreCursorEvents).toHaveBeenCalledWith(true);
    expect(emitMock).toHaveBeenCalledWith("overlay_region_updated", REGION);
    expect(windowMock.show).toHaveBeenCalledOnce();
  });

  it("falls back to the first monitor when the region monitor is gone", async () => {
    const windowMock = createWindowMock();
    getByLabel.mockResolvedValue(windowMock);

    await showRegionOverlay({ ...REGION, monitor_id: "\\\\.\\DISPLAY3" });

    expect(windowMock.setPosition).toHaveBeenCalledWith(expect.objectContaining({ x: 0, y: 0 }));
    expect(windowMock.setSize).toHaveBeenCalledWith(expect.objectContaining({ width: 1920, height: 1080 }));
  });

  it("does nothing when the overlay window is not configured", async () => {
    getByLabel.mockResolvedValue(null);

    await expect(showRegionOverlay(REGION)).resolves.toBeUndefined();
    expect(emitMock).not.toHaveBeenCalled();
  });

  it("does nothing when no monitor is available", async () => {
    const windowMock = createWindowMock();
    getByLabel.mockResolvedValue(windowMock);
    availableMonitorsMock.mockResolvedValue([]);

    await expect(showRegionOverlay(REGION)).resolves.toBeUndefined();
    expect(windowMock.setPosition).not.toHaveBeenCalled();
  });

  it("hides the overlay and clears its content", async () => {
    const windowMock = createWindowMock();
    getByLabel.mockResolvedValue(windowMock);

    await hideRegionOverlay();

    expect(emitMock).toHaveBeenCalledWith("overlay_hidden");
    expect(windowMock.hide).toHaveBeenCalledOnce();
  });

  it("swallows window failures instead of breaking the caller", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const windowMock = createWindowMock();
    windowMock.setSize.mockRejectedValue(new Error("permission denied"));
    getByLabel.mockResolvedValue(windowMock);

    await expect(showRegionOverlay(REGION)).resolves.toBeUndefined();
    expect(warn).toHaveBeenCalled();
    warn.mockRestore();
  });
});

describe("showMultiBoxOverlay", () => {
  it("positions the overlay on the first box's monitor without showing it", async () => {
    const windowMock = createWindowMock();
    getByLabel.mockResolvedValue(windowMock);

    await showMultiBoxOverlay([
      { region: { ...REGION, monitor_id: "\\\\.\\DISPLAY2" } },
      { region: { ...REGION, monitor_id: "\\\\.\\DISPLAY1" } },
    ]);

    expect(getByLabel).toHaveBeenCalledWith("overlay");
    expect(windowMock.setPosition).toHaveBeenCalledWith(expect.objectContaining({ x: 1920, y: 0 }));
    expect(windowMock.setSize).toHaveBeenCalledWith(expect.objectContaining({ width: 2560, height: 1440 }));
    expect(windowMock.setIgnoreCursorEvents).toHaveBeenCalledWith(true);
    // 只定位不 show：后端 start_multi_realtime 负责显示，避免空 overlay 闪烁；
    // 也不发布 region，多框边框由 box-added/box-updated 事件驱动。
    expect(windowMock.show).not.toHaveBeenCalled();
    expect(emitMock).not.toHaveBeenCalled();
  });

  it("falls back to the first monitor when the box monitor is gone", async () => {
    const windowMock = createWindowMock();
    getByLabel.mockResolvedValue(windowMock);

    await showMultiBoxOverlay([{ region: { ...REGION, monitor_id: "\\\\.\\DISPLAY3" } }]);

    expect(windowMock.setPosition).toHaveBeenCalledWith(expect.objectContaining({ x: 0, y: 0 }));
    expect(windowMock.setSize).toHaveBeenCalledWith(expect.objectContaining({ width: 1920, height: 1080 }));
  });

  it("does nothing when there are no boxes", async () => {
    const windowMock = createWindowMock();
    getByLabel.mockResolvedValue(windowMock);

    await expect(showMultiBoxOverlay([])).resolves.toBeUndefined();
    expect(windowMock.setPosition).not.toHaveBeenCalled();
  });

  it("does nothing when the overlay window is not configured", async () => {
    getByLabel.mockResolvedValue(null);

    await expect(showMultiBoxOverlay([{ region: REGION }])).resolves.toBeUndefined();
    expect(emitMock).not.toHaveBeenCalled();
  });

  it("swallows positioning failures instead of breaking the caller", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const windowMock = createWindowMock();
    windowMock.setPosition.mockRejectedValue(new Error("permission denied"));
    getByLabel.mockResolvedValue(windowMock);

    await expect(showMultiBoxOverlay([{ region: REGION }])).resolves.toBeUndefined();
    expect(warn).toHaveBeenCalled();
    warn.mockRestore();
  });
});
