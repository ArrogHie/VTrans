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

const BOX_0 = {
  box_id: 0,
  region: { monitor_id: "m0", x: 10, y: 20, width: 300, height: 400 },
  color: "#FF6B6B",
};

/** Seeds the store snapshot that SSR rendering reads (same pattern as resultWindowMultiBox.test.tsx). */
function renderWithMode(
  mode: "single" | "live",
  overrides: Partial<ReturnType<typeof useAppStore.getInitialState>> = {},
) {
  Object.assign(useAppStore.getInitialState(), {
    mode,
    translationBoxes: [],
    boxStatuses: {},
    ...overrides,
  });
  return renderToStaticMarkup(<MainWindow />);
}

/** Returns the `<button>` element (opening tag included) whose label follows the given text. */
function buttonFor(html: string, label: string): string {
  // Match the label as the button's final text node, so substrings like
  // 「已停止」 in badges or warnings cannot hijack the lookup.
  const marker = `${label}</button>`;
  const at = html.indexOf(marker);
  const start = html.lastIndexOf("<button", at);
  if (at === -1 || start === -1) {
    throw new Error(`button with label "${label}" not found`);
  }
  return html.slice(start, at + marker.length);
}

describe("MainWindow mode sections", () => {
  it("renders only the region section in single mode", () => {
    const html = renderWithMode("single");
    // 「翻译区域」区块只保留「选择并翻译」一个按钮（BUGFIX-2：删除重复的
    // 「选择屏幕区域」，二者行为相同）。
    expect(html).toContain("翻译区域");
    expect(html).not.toContain("选择屏幕区域");
    expect(html).toContain("选择并翻译");
    // 多框列表整体不渲染。
    expect(html).not.toContain("新增翻译框");
    expect(html).not.toContain("开始多框实时");
    expect(html).not.toContain("multibox-empty");
  });

  it("renders only the multi-box list in live mode", () => {
    const html = renderWithMode("live");
    expect(html).toContain("新增翻译框");
    expect(html).toContain("multibox-empty");
    // 会话级启停按钮已从列表移出，统一由 live 模式底部控制行提供。
    expect(html).not.toContain("开始多框实时");
    // 「翻译区域」区块整体不渲染。
    expect(html).not.toContain("翻译区域");
    expect(html).not.toContain("选择屏幕区域");
    expect(html).not.toContain("选择并翻译");
  });

  it("drives the multi-box session from the live-mode bottom controls", () => {
    const html = renderWithMode("live");
    expect(html).toContain("开始实时");
    expect(html).toContain("停止");
    // 多框会话没有暂停概念；单框实时会话由热键/悬浮球/结果弹窗控制。
    expect(html).not.toContain("暂停");
    expect(html).not.toContain("继续实时");
  });

  it("disables both multi-box controls when no boxes exist", () => {
    const html = renderWithMode("live");
    expect(buttonFor(html, "开始实时")).toContain("disabled");
    expect(buttonFor(html, "停止")).toContain("disabled");
  });

  it("enables start and disables stop when boxes exist but none is running", () => {
    const html = renderWithMode("live", { translationBoxes: [BOX_0], boxStatuses: { 0: "Stopped" } });
    expect(buttonFor(html, "开始实时")).not.toContain("disabled");
    expect(buttonFor(html, "停止")).toContain("disabled");
  });

  it("disables start and enables stop while a box is running", () => {
    const html = renderWithMode("live", { translationBoxes: [BOX_0], boxStatuses: { 0: "Running" } });
    expect(buttonFor(html, "开始实时")).toContain("disabled");
    expect(buttonFor(html, "停止")).not.toContain("disabled");
  });

  it("hides the multi-box live controls in single mode", () => {
    const html = renderWithMode("single");
    expect(html).not.toContain("开始实时");
    expect(html).not.toContain("继续实时");
    expect(html).not.toContain("暂停");
  });

  it("keeps the result popup button in both modes", () => {
    expect(renderWithMode("single")).toContain("打开翻译弹窗");
    expect(renderWithMode("live")).toContain("打开翻译弹窗");
  });
});
