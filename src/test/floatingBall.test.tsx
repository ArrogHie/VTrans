import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { FloatingBall } from "../windows/FloatingBall";
import { useAppStore } from "../stores/appStore";

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

/** Seeds the store snapshot that SSR rendering reads (same pattern as mainWindowModeSections.test.tsx). */
function renderMenu(overrides: Partial<ReturnType<typeof useAppStore.getInitialState>> = {}) {
  Object.assign(useAppStore.getInitialState(), {
    mode: "single",
    liveConfig: null,
    livePaused: false,
    boxStatuses: {},
    ...overrides,
  });
  return renderToStaticMarkup(<FloatingBall initialOpen />);
}

/** Returns the `<button>` element (opening tag included) whose label follows the given text. */
function buttonFor(html: string, label: string): string {
  const marker = `${label}</button>`;
  const at = html.indexOf(marker);
  const start = html.lastIndexOf("<button", at);
  if (at === -1 || start === -1) {
    throw new Error(`button with label "${label}" not found`);
  }
  return html.slice(start, at + marker.length);
}

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

  it("positions the ball and the menu through the shared CSS classes", () => {
    const collapsed = renderToStaticMarkup(<FloatingBall />);
    expect(collapsed).toContain("floater-ball");
    const expanded = renderToStaticMarkup(<FloatingBall initialOpen />);
    expect(expanded).toContain("floater-menu-panel");
    // 定位交给 CSS（--floater-padding / --floater-size），组件不内联坐标。
    expect(expanded).not.toContain('class="floater-menu-panel absolute');
  });

  it("renders the expanded menu with appearance controls", () => {
    const html = renderMenu();
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

describe("FloatingBall live-sync menu", () => {
  // BUGFIX-4：悬浮球与主窗口共享同一多框会话，菜单状态由共享推导函数
  // isAnyBoxRunning / isSingleLiveRunning 派生。
  it("shows the idle state and a disabled pause button without any session", () => {
    const html = renderMenu();
    expect(html).toContain(">实时翻译</button>");
    expect(html).not.toContain(">停止实时翻译</button>");
    expect(buttonFor(html, "暂停·继续")).toContain("disabled");
  });

  it("shows the stop state when any multi-box is running", () => {
    const html = renderMenu({ boxStatuses: { 0: "Running" } });
    expect(html).toContain(">停止实时翻译</button>");
    expect(html).not.toContain(">实时翻译</button>");
    // 多框运行中暂停按钮禁用：暂停只作用于单框会话。
    expect(buttonFor(html, "暂停·继续")).toContain("disabled");
  });

  it("shows the stop state from a single-live session when every box is stopped", () => {
    const html = renderMenu({
      mode: "live",
      liveConfig: {
        region: { monitor_id: "m0", x: 0, y: 0, width: 10, height: 10 },
        capture_interval_ms: 500,
        difference_threshold: 0.03,
      },
      boxStatuses: { 0: "Stopped" },
    });
    expect(html).toContain(">停止实时翻译</button>");
    // 单框会话运行中（且无框运行）时暂停·继续可用。
    expect(buttonFor(html, "暂停·继续")).not.toContain("disabled");
  });

  it("stays idle when every box is stopped and no single-live session exists", () => {
    const html = renderMenu({ boxStatuses: { 0: "Stopped", 1: "Stopped" } });
    expect(html).toContain(">实时翻译</button>");
    expect(buttonFor(html, "暂停·继续")).toContain("disabled");
  });

  it("keeps the pause button enabled for a paused single-live session", () => {
    const html = renderMenu({
      mode: "live",
      livePaused: true,
      liveConfig: {
        region: { monitor_id: "m0", x: 0, y: 0, width: 10, height: 10 },
        capture_interval_ms: 500,
        difference_threshold: 0.03,
      },
    });
    expect(html).toContain(">停止实时翻译</button>");
    expect(html).toContain("继续");
    expect(buttonFor(html, "继续")).not.toContain("disabled");
  });
});
