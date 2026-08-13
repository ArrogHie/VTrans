import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { MultiBoxResults } from "../components/MultiBoxResults";
import type { BoxedTranslationResult } from "../types";

const result = (boxId: number, color: string, text: string): BoxedTranslationResult => ({
  box_id: boxId,
  color,
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

  it("makes the stack scrollable", () => {
    const html = renderToStaticMarkup(
      <MultiBoxResults boxes={BOXES} results={{}} statuses={{}} />,
    );
    expect(html).toContain('data-testid="multibox-results"');
    expect(html).toContain("overflow-y-auto");
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
