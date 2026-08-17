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
const { TRANSLATION_MODEL_ENTRY_ID } = await import("../types");
import type { ModelStatusReport } from "../types";

function reportWith(overrides: Partial<ModelStatusReport>): ModelStatusReport {
  return {
    entries: [
      { id: "ppocr-det-v6", state: "ready", optional: false },
      { id: "opus-mt-en-zh-tokenizer", state: "ready", optional: false },
      { id: TRANSLATION_MODEL_ENTRY_ID, state: "missing", optional: true },
    ],
    ocr_ready: true,
    translation_ready: false,
    ...overrides,
  };
}

/**
 * Presets `modelStatus` and renders the main window.
 *
 * `renderToStaticMarkup` reads the SSR server snapshot, which zustand v4
 * serves from `getInitialState()` — so the preset mutates that object (same
 * pattern as the existing MainWindow SSR tests) instead of `setState`.
 */
function renderMainWithModelStatus(modelStatus: ModelStatusReport | null): string {
  Object.assign(useAppStore.getInitialState(), {
    modelStatus,
    modelDownloadProgress: null,
    translationModelDownloading: false,
  });
  return renderToStaticMarkup(<MainWindow />);
}

describe("MainWindow model setup banner", () => {
  it("shows the persistent banner when ocr_ready is false", () => {
    const html = renderMainWithModelStatus(reportWith({ ocr_ready: false }));
    expect(html).toContain("OCR 模型未就位，翻译功能不可用");
    expect(html).toContain("重试");
  });

  it("shows the persistent banner when a non-optional entry is invalid", () => {
    const html = renderMainWithModelStatus(
      reportWith({
        entries: [
          { id: "ppocr-det-v6", state: "ready", optional: false },
          { id: "opus-mt-en-zh-tokenizer", state: "invalid", optional: false },
          { id: TRANSLATION_MODEL_ENTRY_ID, state: "missing", optional: true },
        ],
      }),
    );
    expect(html).toContain("OCR 模型未就位，翻译功能不可用");
  });

  it("hides the banner while the model setup is healthy (retry success)", () => {
    const html = renderMainWithModelStatus(
      reportWith({
        entries: [
          { id: "ppocr-det-v6", state: "ready", optional: false },
          { id: "opus-mt-en-zh-tokenizer", state: "ready", optional: false },
          { id: TRANSLATION_MODEL_ENTRY_ID, state: "ready", optional: true },
        ],
        translation_ready: true,
      }),
    );
    expect(html).not.toContain("OCR 模型未就位，翻译功能不可用");
    expect(html).not.toContain("重试中");
  });

  it("hides the banner when an optional entry alone is invalid", () => {
    const html = renderMainWithModelStatus(
      reportWith({
        entries: [
          { id: "ppocr-det-v6", state: "ready", optional: false },
          { id: "opus-mt-en-zh-tokenizer", state: "ready", optional: false },
          { id: TRANSLATION_MODEL_ENTRY_ID, state: "invalid", optional: true },
        ],
      }),
    );
    expect(html).not.toContain("OCR 模型未就位，翻译功能不可用");
  });

  it("does not show the banner before the first status report arrives", () => {
    const html = renderMainWithModelStatus(null);
    expect(html).not.toContain("OCR 模型未就位，翻译功能不可用");
  });

  it("does not block the rest of the main window while the banner is visible", () => {
    const html = renderMainWithModelStatus(reportWith({ ocr_ready: false }));
    // 设置、框选入口仍可见。
    expect(html).toContain("选择并翻译");
    expect(html).toContain('title="设置"');
  });
});
