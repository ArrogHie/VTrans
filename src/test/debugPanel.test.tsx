import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { DebugPanel, formatDebugTimestamp, truncateForDisplay } from "../components/DebugPanel";
import type { DebugFramePayload } from "../types";

const frame: DebugFramePayload = {
  image: "base64-thumbnail-bytes",
  region: { monitor_id: "\\.\\DISPLAY1", x: 400, y: 300, width: 800, height: 400 },
  frame_index: 42,
  timestamp_ms: 1785911487496,
};

describe("DebugPanel", () => {
  it("marks the panel as Debug-only display with no persistence", () => {
    const html = renderToStaticMarkup(<DebugPanel frame={null} />);
    expect(html).toContain("Debug 模式 · 仅显示不保存");
    expect(html).toContain('data-testid="debug-panel"');
  });

  it("shows a waiting placeholder before the first frame arrives", () => {
    const html = renderToStaticMarkup(<DebugPanel frame={null} />);
    expect(html).toContain("等待捕获帧…");
    expect(html).not.toContain("data:image/jpeg;base64,");
  });

  it("renders the latest thumbnail as an inline JPEG data URL", () => {
    const html = renderToStaticMarkup(<DebugPanel frame={frame} />);
    expect(html).toContain('src="data:image/jpeg;base64,base64-thumbnail-bytes"');
    expect(html).toContain("OCR 前的捕获帧缩略图");
  });

  it("overlays frame index, region coordinates, size, monitor and timestamp", () => {
    const html = renderToStaticMarkup(<DebugPanel frame={frame} />);
    expect(html).toContain("帧 #42");
    expect(html).toContain("位置 (400, 300)");
    expect(html).toContain("尺寸 800 × 400");
    expect(html).toContain("\\.\\DISPLAY1");
    expect(html).toContain(formatDebugTimestamp(frame.timestamp_ms));
  });

  it("shows the latest OCR text when provided", () => {
    const html = renderToStaticMarkup(<DebugPanel frame={frame} ocrText="  Hello, world!  " />);
    expect(html).toContain("最近识别：Hello, world!");
    expect(html).toContain('data-testid="debug-ocr-text"');
  });

  it("omits the OCR cross-check line when no text is available", () => {
    const html = renderToStaticMarkup(<DebugPanel frame={frame} ocrText={null} />);
    expect(html).not.toContain("最近识别");
  });
});

describe("truncateForDisplay", () => {
  it("keeps short text unchanged and trimmed", () => {
    expect(truncateForDisplay("  你好  ")).toBe("你好");
  });

  it("caps long text with an ellipsis", () => {
    const long = "x".repeat(200);
    const truncated = truncateForDisplay(long, 120);
    expect(truncated).toHaveLength(121);
    expect(truncated.endsWith("…")).toBe(true);
  });
});

describe("formatDebugTimestamp", () => {
  it("formats a millisecond timestamp as a local wall-clock time", () => {
    const formatted = formatDebugTimestamp(frame.timestamp_ms);
    expect(formatted).toMatch(/^\d{2}:\d{2}:\d{2}$/);
  });
});
