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

  it("disables only the local option and shows the hint when local is selected but the model is missing", () => {
    const html = renderToStaticMarkup(
      <ProviderSelect value="local" onChange={() => {}} disabled switching={false} progress={null} localBlocked="missing" />,
    );
    const select = renderEngineSelect(html);
    expect(select).toContain('value="local" disabled=""');
    expect(select).not.toContain('value="openai" disabled=""');
    expect(html).toContain("请先在设置中下载本地翻译模型");
  });

  it("shows the redownload hint when the local model failed verification", () => {
    const html = renderToStaticMarkup(
      <ProviderSelect value="local" onChange={() => {}} disabled switching={false} progress={null} localBlocked="invalid" />,
    );
    expect(renderEngineSelect(html)).toContain('value="local" disabled=""');
    expect(html).toContain("本地翻译模型校验失败，请在设置中重新下载");
  });

  it("disables the local option and explains the block while a download is in flight", () => {
    // 下载中即使用户当前选的是云端引擎，也要提示本地选项为何不可选。
    const html = renderToStaticMarkup(
      <ProviderSelect value="openai" onChange={() => {}} disabled switching={false} progress={null} localBlocked="downloading" />,
    );
    expect(renderEngineSelect(html)).toContain('value="local" disabled=""');
    expect(html).toContain("本地翻译模型下载中，完成后可切换本地引擎");
  });

  it("does not show the missing-model hint while a cloud provider is selected", () => {
    const html = renderToStaticMarkup(
      <ProviderSelect value="openai" onChange={() => {}} disabled switching={false} progress={null} localBlocked="missing" />,
    );
    expect(html).not.toContain("请先在设置中下载本地翻译模型");
  });

  it("keeps the local option enabled when the model is available", () => {
    const html = renderToStaticMarkup(
      <ProviderSelect value="local" onChange={() => {}} disabled switching={false} progress={null} localBlocked={null} />,
    );
    expect(renderEngineSelect(html)).not.toContain('value="local" disabled=""');
    expect(html).not.toContain("本地翻译模型下载中");
  });
});
