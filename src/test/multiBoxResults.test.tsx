import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { MultiBoxResults } from "../components/MultiBoxResults";
import type { BoxedTranslationResult } from "../types";

const result = (
  boxId: number,
  color: string,
  text: string,
  originalText = "",
): BoxedTranslationResult => ({
  box_id: boxId,
  color,
  original_text: originalText,
  result: { translated_text: text, provider_id: "mock", elapsed_ms: 1 },
  timestamp: 1,
});

const BOXES = [
  { box_id: 0, color: "#FF6B6B" },
  { box_id: 1, color: "#4ECDC4" },
];

describe("MultiBoxResults", () => {
  it("stacks boxes top-to-bottom with colored borders and dividers", () => {
    const html = renderToStaticMarkup(
      <MultiBoxResults
        boxes={BOXES}
        results={{ 0: result(0, "#FF6B6B", "你好"), 1: result(1, "#4ECDC4", "世界") }}
        statuses={{ 0: "Running", 1: "Stopped" }}
      />,
    );
    expect(html).toContain("框 1");
    expect(html).toContain("框 2");
    expect(html).toContain("2px solid #FF6B6B");
    expect(html).toContain("2px solid #4ECDC4");
    // 两个框之间恰好一条分隔线。
    expect((html.match(/multibox-divider/g) ?? []).length).toBe(1);
    expect(html).toContain("你好");
    expect(html).toContain("世界");
    expect(html).toContain("运行中");
    expect(html).toContain("已停止");
  });

  it("collapses the original text by default behind a per-box toggle", () => {
    const html = renderToStaticMarkup(
      <MultiBoxResults
        boxes={BOXES}
        results={{
          0: result(0, "#FF6B6B", "你好", "hello"),
          1: result(1, "#4ECDC4", "世界", "world"),
        }}
        statuses={{ 0: "Running", 1: "Running" }}
      />,
    );
    // 每个有原文的框渲染折叠开关，但原文区域默认折叠。
    expect(html).toContain('data-testid="multibox-original-toggle-0"');
    expect(html).toContain('data-testid="multibox-original-toggle-1"');
    expect(html).toContain('aria-expanded="false"');
    expect(html).toContain("lucide-chevron-right");
    expect(html).not.toContain('data-testid="multibox-original-0"');
    expect(html).not.toContain('data-testid="multibox-original-1"');
    expect(html).not.toContain("hello");
    expect(html).not.toContain("world");
  });

  it("renders the original text above the translation when expanded", () => {
    const html = renderToStaticMarkup(
      <MultiBoxResults
        boxes={BOXES}
        results={{
          0: result(0, "#FF6B6B", "你好", "hello"),
          1: result(1, "#4ECDC4", "世界", "world"),
        }}
        statuses={{ 0: "Running", 1: "Running" }}
        initialExpandedBoxIds={[0]}
      />,
    );
    // 展开的框显示原文，未展开的框保持折叠。
    expect(html).toContain('data-testid="multibox-original-0"');
    expect(html).toContain("hello");
    expect(html).toContain('aria-expanded="true"');
    expect(html).toContain("lucide-chevron-down");
    expect(html).not.toContain('data-testid="multibox-original-1"');
    expect(html).not.toContain("world");
    // 原文以小字次级色样式渲染在译文上方（次级底色样式仅原文区域使用）。
    expect(html).toContain("bg-slate-100/70");
    expect(html).toContain("text-slate-500");
    const originalIndex = html.indexOf("hello");
    const translationIndex = html.indexOf("你好");
    expect(originalIndex).toBeGreaterThanOrEqual(0);
    expect(translationIndex).toBeGreaterThan(originalIndex);
  });

  it("omits the original area and toggle entirely when original_text is empty", () => {
    const html = renderToStaticMarkup(
      <MultiBoxResults
        boxes={BOXES}
        results={{
          0: result(0, "#FF6B6B", "你好", "hello"),
          1: result(1, "#4ECDC4", "世界", ""),
        }}
        statuses={{ 0: "Running", 1: "Stopped" }}
      />,
    );
    // 有原文的框渲染折叠开关（默认折叠）。
    expect(html).toContain('data-testid="multibox-original-toggle-0"');
    expect(html).not.toContain('data-testid="multibox-original-0"');
    // 空原文的框不渲染开关与原文区域，也不留任何空占位。
    expect(html).not.toContain('data-testid="multibox-original-toggle-1"');
    expect(html).not.toContain('data-testid="multibox-original-1"');
    // 布局回归：彩色边框、分隔线、状态徽章保持现状。
    expect(html).toContain("2px solid #FF6B6B");
    expect(html).toContain("2px solid #4ECDC4");
    expect((html.match(/multibox-divider/g) ?? []).length).toBe(1);
    expect(html).toContain("运行中");
    expect(html).toContain("已停止");
  });

  it("makes the stack scrollable", () => {
    const html = renderToStaticMarkup(
      <MultiBoxResults boxes={BOXES} results={{}} statuses={{}} />,
    );
    expect(html).toContain('data-testid="multibox-results"');
    // min-h-0 允许 flex 子项收缩，滚动条才会出现（弹窗 body 的 overflow:hidden
    // 会把撑高的内容裁掉）。
    expect(html).toContain("min-h-0");
    expect(html).toContain("overflow-y-auto");
    // 无结果时不渲染任何原文区域。
    expect(html).not.toContain("multibox-original");
  });

  it("marks a stopped box without a result as stopped", () => {
    const html = renderToStaticMarkup(
      <MultiBoxResults boxes={BOXES} results={{ 0: result(0, "#FF6B6B", "你好") }} statuses={{ 0: "Running", 1: "Stopped" }} />,
    );
    expect(html).toContain("已停止");
    // 框 2 无结果且已停止，正文显示「已停止」而非等待占位。
    expect(html).toContain('data-testid="multibox-translation-1"');
  });

  it("falls back to results when the box list has not hydrated yet", () => {
    const html = renderToStaticMarkup(
      <MultiBoxResults
        boxes={[]}
        results={{ 0: result(0, "#FF6B6B", "你好") }}
        statuses={{ 0: "Running" }}
      />,
    );
    expect(html).toContain("框 1");
    expect(html).toContain("你好");
    expect(html).toContain("2px solid #FF6B6B");
  });

  it("surfaces the error message for an errored box without a result", () => {
    const html = renderToStaticMarkup(
      <MultiBoxResults
        boxes={BOXES}
        results={{}}
        statuses={{ 0: { Error: "capture failed" }, 1: "Stopped" }}
      />,
    );
    expect(html).toContain("capture failed");
  });
});
