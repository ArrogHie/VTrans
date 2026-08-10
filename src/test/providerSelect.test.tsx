import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { ProviderSelect } from "../components/ProviderSelect";

function renderEngineSelect(html: string): string {
  return html.match(/<select[^>]*aria-label="翻译引擎"[\s\S]*?<\/select>/)?.[0] ?? "";
}

describe("ProviderSelect", () => {
  it("disables the engine select while a provider switch is in flight", () => {
    const html = renderToStaticMarkup(
      <ProviderSelect value="openai" onChange={() => {}} disabled switching progress={null} />,
    );
    expect(renderEngineSelect(html)).toContain("disabled=\"\"");
  });

  it("shows a generic switching message before any progress event arrives", () => {
    const html = renderToStaticMarkup(
      <ProviderSelect value="openai" onChange={() => {}} disabled switching progress={null} />,
    );
    expect(html).toContain("正在切换翻译引擎…");
  });

  it("shows the loading percentage driven by backend progress events", () => {
    const html = renderToStaticMarkup(
      <ProviderSelect value="local" onChange={() => {}} disabled switching progress={0.5} />,
    );
    expect(html).toContain("模型加载中 50%");
  });

  it("shows 100% when a cached provider completes near-instantly", () => {
    const html = renderToStaticMarkup(
      <ProviderSelect value="local" onChange={() => {}} disabled switching progress={1} />,
    );
    expect(html).toContain("模型加载中 100%");
  });

  it("keeps the select enabled and hides progress when no switch is in flight", () => {
    const html = renderToStaticMarkup(
      <ProviderSelect value="openai" onChange={() => {}} disabled={false} switching={false} progress={null} />,
    );
    expect(renderEngineSelect(html)).not.toContain("disabled");
    expect(html).not.toContain("正在切换翻译引擎");
    expect(html).not.toContain("模型加载中");
  });
});
