import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import type { AppState } from "../stores/appStore";

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ label: "result" }),
}));
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  WebviewWindow: { getByLabel: () => Promise.resolve(null) },
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: () => Promise.resolve() }));
vi.mock("@tauri-apps/api/event", () => ({
  emit: () => Promise.resolve(),
  listen: () => Promise.resolve(() => undefined),
}));
vi.mock("@tauri-apps/api/dpi", () => ({
  PhysicalPosition: class {
    constructor(public x: number, public y: number) {}
  },
  PhysicalSize: class {
    constructor(public width: number, public height: number) {}
  },
}));

const { ResultWindow } = await import("../windows/ResultWindow");
const { useAppStore } = await import("../stores/appStore");

/**
 * Seeds the Zustand server snapshot that SSR rendering reads.
 *
 * `renderToStaticMarkup` uses the snapshot captured at store creation, so
 * `setState` is invisible to it. The snapshot object is also the live state
 * until the first `setState`, so mutating it here keeps both snapshots in
 * sync for the duration of these render-only tests.
 */
function seedServerSnapshot(partial: Partial<AppState>) {
  Object.assign(useAppStore.getInitialState(), partial);
}

const REGION = { monitor_id: "m0", x: 0, y: 0, width: 100, height: 100 };
const BOX = { box_id: 0, region: REGION, color: "#FF6B6B" };
const BOXED = {
  box_id: 0,
  color: "#FF6B6B",
  original_text: "hello",
  result: { translated_text: "你好", provider_id: "mock", elapsed_ms: 1 },
  timestamp: 1,
};

describe("ResultWindow multi-box layout", () => {
  it("renders stacked colored results and a running title when engaged", () => {
    seedServerSnapshot({
      translationBoxes: [BOX],
      boxStatuses: { 0: "Running" },
      multiBoxResults: { 0: BOXED },
      singleResult: null,
    });
    const html = renderToStaticMarkup(<ResultWindow />);
    expect(html).toContain("运行中");
    expect(html).toContain("2px solid #FF6B6B");
    expect(html).toContain("你好");
    expect(html).toContain('data-testid="multibox-results"');
    // 多框模式下不渲染单次翻译的原文折叠开关。
    expect(html).not.toContain('data-testid="result-source-toggle"');
    // 每框原文默认折叠：有原文的框渲染逐框开关，原文内容不直接展示。
    expect(html).toContain('data-testid="multibox-original-toggle-0"');
    expect(html).not.toContain('data-testid="multibox-original-0"');
    expect(html).not.toContain("hello");
  });

  it("shows a stopped title after the multi-box session stops", () => {
    seedServerSnapshot({
      translationBoxes: [BOX],
      boxStatuses: { 0: "Stopped" },
      multiBoxResults: { 0: BOXED },
      singleResult: null,
    });
    const html = renderToStaticMarkup(<ResultWindow />);
    expect(html).toContain("已停止");
    expect(html).not.toContain("运行中");
  });

  it("falls back to the single mini-bar when multi-box is not engaged", () => {
    seedServerSnapshot({
      translationBoxes: [],
      boxStatuses: {},
      multiBoxResults: {},
      singleResult: { original_text: "hello", translated_text: "你好", timestamp: 1 },
    });
    const html = renderToStaticMarkup(<ResultWindow />);
    expect(html).toContain('data-testid="result-source-toggle"');
    expect(html).toContain('data-testid="result-translation-text"');
    expect(html).not.toContain('data-testid="multibox-results"');
  });
});
