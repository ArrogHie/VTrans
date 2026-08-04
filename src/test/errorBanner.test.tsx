import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { ErrorBanner } from "../components/ErrorBanner";

describe("ErrorBanner", () => {
  it("renders the error message with an alert role", () => {
    const html = renderToStaticMarkup(
      <ErrorBanner message="翻译请求超时" onDismiss={() => {}} />,
    );
    expect(html).toContain("翻译请求超时");
    expect(html).toContain('role="alert"');
  });

  it("renders a dismiss button", () => {
    const html = renderToStaticMarkup(
      <ErrorBanner message="模型缺失" onDismiss={() => {}} />,
    );
    expect(html).toContain('title="关闭"');
    expect(html).toContain("×");
  });
});
