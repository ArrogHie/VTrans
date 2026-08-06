import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { FloatingBall } from "../windows/FloatingBall";

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ label: "floater" }),
  availableMonitors: () => Promise.resolve([]),
}));

vi.mock("@tauri-apps/api/dpi", () => ({
  LogicalSize: class {
    constructor(public width: number, public height: number) {}
  },
  PhysicalPosition: class {
    constructor(public x: number, public y: number) {}
  },
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: () => Promise.resolve() }));
vi.mock("@tauri-apps/api/event", () => ({
  emit: () => Promise.resolve(),
  listen: () => Promise.resolve(() => undefined),
}));

describe("FloatingBall", () => {
  it("renders the collapsed ball without the menu", () => {
    const html = renderToStaticMarkup(<FloatingBall />);
    expect(html).toContain('aria-label="悬浮球"');
    expect(html).toContain('data-testid="floating-ball"');
    // Bug 2：不再依赖 data-tauri-drag-region（deep 会吞掉点击），
    // 拖动与点击由手动判别（mousedown/mousemove 阈值）完成。
    expect(html).not.toContain("data-tauri-drag-region");
    expect(html).not.toContain("框选翻译");
    expect(html).not.toContain('data-testid="floating-ball-menu"');
  });

  it("keeps the window container free of scrollbars in both states", () => {
    const html = renderToStaticMarkup(<FloatingBall />);
    // Bug 1：容器 overflow-hidden，展开/收起都不会出现滚动条。
    expect(html).toContain("overflow-hidden");
  });

  it("sizes the ball through the CSS custom property, not inline styles", () => {
    const html = renderToStaticMarkup(<FloatingBall />);
    // 直径/透明度由 CSS 变量驱动（.floater-ball），无窗口 opacity API。
    expect(html).toContain("floater-ball");
    expect(html).not.toContain("style=");
  });

  it("renders the expanded menu with appearance controls", () => {
    const html = renderToStaticMarkup(<FloatingBall initialOpen />);
    expect(html).toContain('data-testid="floating-ball-menu"');
    expect(html).toContain('data-testid="floater-appearance"');
    expect(html).toContain('data-testid="floater-opacity-slider"');
    expect(html).toContain('data-testid="floater-size-slider"');
    expect(html).toContain("透明度");
    expect(html).toContain("大小（48px）");
    expect(html).toContain("框选翻译");
    expect(html).toContain("实时翻译");
    expect(html).toContain("暂停·继续");
    expect(html).toContain("打开主窗口");
  });
});
