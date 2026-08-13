import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

// Same Tauri mocks as mainWindowLanguage.test.tsx: MainWindow pulls in Tauri
// APIs transitively, and SSR rendering never fires useEffect IPC calls.
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ label: "main" }),
  availableMonitors: () => Promise.resolve([]),
}));

vi.mock("@tauri-apps/api/event", () => ({
  emit: () => Promise.resolve(),
  listen: () => Promise.resolve(() => undefined),
}));

vi.mock("@tauri-apps/api/dpi", () => ({
  LogicalSize: class {
    constructor(public width: number, public height: number) {}
  },
  PhysicalPosition: class {
    constructor(public x: number, public y: number) {}
  },
  PhysicalSize: class {
    constructor(public width: number, public height: number) {}
  },
}));

vi.mock("@tauri-apps/api/webviewWindow", () => ({
  WebviewWindow: {
    getByLabel: () => Promise.resolve(null),
  },
}));

const { MainWindow } = await import("../windows/MainWindow");
const { useAppStore } = await import("../stores/appStore");

/** Seeds the store snapshot that SSR rendering reads (same pattern as resultWindowMultiBox.test.tsx). */
function renderWithMode(mode: "single" | "live") {
  Object.assign(useAppStore.getInitialState(), { mode });
  return renderToStaticMarkup(<MainWindow />);
}

describe("MainWindow mode sections", () => {
  it("renders only the region section in single mode", () => {
    const html = renderWithMode("single");
    // 「翻译区域」区块：选择屏幕区域 + 底部「选择并翻译」。
    expect(html).toContain("翻译区域");
    expect(html).toContain("选择屏幕区域");
    expect(html).toContain("选择并翻译");
    // 多框列表整体不渲染。
    expect(html).not.toContain("新增翻译框");
    expect(html).not.toContain("开始多框实时");
    expect(html).not.toContain("multibox-empty");
  });

  it("renders only the multi-box list in live mode", () => {
    const html = renderWithMode("live");
    expect(html).toContain("新增翻译框");
    expect(html).toContain("开始多框实时");
    expect(html).toContain("multibox-empty");
    // 「翻译区域」区块整体不渲染。
    expect(html).not.toContain("翻译区域");
    expect(html).not.toContain("选择屏幕区域");
    expect(html).not.toContain("选择并翻译");
  });

  it("keeps the single-box live controls in live mode", () => {
    const html = renderWithMode("live");
    expect(html).toContain("开始实时");
    expect(html).toContain("暂停");
    expect(html).toContain("停止");
  });

  it("hides the single-box live controls in single mode", () => {
    const html = renderWithMode("single");
    expect(html).not.toContain("开始实时");
    expect(html).not.toContain("继续实时");
  });

  it("keeps the result popup button in both modes", () => {
    expect(renderWithMode("single")).toContain("打开翻译弹窗");
    expect(renderWithMode("live")).toContain("打开翻译弹窗");
  });
});
