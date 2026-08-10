import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

// MainWindow pulls in Tauri APIs transitively (regionOverlay, debugFrames,
// events). Mock the surface used at import time so SSR render does not reach
// the real Tauri runtime. useEffect hooks (IPC hydration, event subscription)
// do not run under renderToStaticMarkup, so no invoke/listen calls fire.
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

describe("MainWindow language selectors", () => {
  it("renders a single merged recognition language selector", () => {
    const html = renderToStaticMarkup(<MainWindow />);
    // 合并后的单一选择器，同时作为 OCR 识别语言与翻译源语言。
    expect(html).toContain("识别语言");
    // 自动检测选项保留。
    expect(html).toContain("自动检测");
  });

  it("no longer renders the separate OCR and source language selectors", () => {
    const html = renderToStaticMarkup(<MainWindow />);
    expect(html).not.toContain("OCR 语言");
    expect(html).not.toContain("源语言");
  });

  it("keeps the target language selector independent", () => {
    const html = renderToStaticMarkup(<MainWindow />);
    expect(html).toContain("目标语言");
  });
});
