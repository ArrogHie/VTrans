import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { ModelSetupBanner } from "../components/ModelSetupBanner";

describe("ModelSetupBanner", () => {
  it("renders the persistent message with an alert role and a retry button", () => {
    const html = renderToStaticMarkup(<ModelSetupBanner retrying={false} onRetry={() => {}} />);
    expect(html).toContain("OCR 模型未就位，翻译功能不可用");
    expect(html).toContain('role="alert"');
    expect(html).toContain("重试");
    expect(html).not.toContain("重试中");
  });

  it("disables the retry button while a retry is in flight", () => {
    const html = renderToStaticMarkup(<ModelSetupBanner retrying onRetry={() => {}} />);
    expect(html).toContain("重试中…");
    expect(html).toContain("disabled");
  });
});
