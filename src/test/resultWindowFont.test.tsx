import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { ResultWindow } from "../windows/ResultWindow";

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

describe("ResultWindow mini-bar", () => {
  const tagAround = (html: string, testId: string): string => {
    const index = html.indexOf(`data-testid="${testId}"`);
    expect(index).toBeGreaterThan(-1);
    return html.slice(Math.max(0, index - 220), index + 80);
  };

  it("lets the --result-font-size variable drive the translation text", () => {
    const html = renderToStaticMarkup(<ResultWindow />);
    const translation = tagAround(html, "result-translation-text");
    // Bug 2：text-sm 会覆盖 .result-text 的 font-size 变量，必须移除；
    // leading 保留以维持行距。
    expect(translation).toContain("result-text");
    expect(translation).toContain("leading-6");
    expect(translation).not.toContain("text-sm");
  });

  it("lets the --result-font-size variable drive the source text too", () => {
    const html = renderToStaticMarkup(<ResultWindow initialSourceOpen />);
    const source = tagAround(html, "result-source-text");
    expect(source).toContain("result-text");
    expect(source).toContain("leading-5");
    expect(source).not.toContain("text-xs");
  });

  it("keeps the whole top bar as a drag region and adapts the close button", () => {
    const html = renderToStaticMarkup(<ResultWindow />);
    // Bug 3：无原生标题栏后，顶栏整体可拖动；关闭按钮有独立的危险色悬停。
    // Tauri 2.11.5 中裸属性只对元素自身点击生效，必须用 "deep" 让整个
    // 顶栏（含标题文本与空白区域）都可拖动，按钮仍阻断拖动保持可点击。
    expect(html).toContain('data-tauri-drag-region="deep"');
    expect(html).toContain("result-close-button");
    expect(html).toContain("select-none");
  });

  it("keeps the frameless rounded-corner shell", () => {
    const html = renderToStaticMarkup(<ResultWindow />);
    expect(html).toContain("rounded-xl");
    expect(html).toContain("shadow-lg");
  });
});
