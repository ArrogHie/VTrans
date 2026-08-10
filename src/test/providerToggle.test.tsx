import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { ProviderToggle, PROVIDER_OPTIONS } from "../components/ProviderToggle";

describe("ProviderToggle", () => {
  it("exposes all five cloud providers plus the local provider", () => {
    expect(PROVIDER_OPTIONS.map((option) => option.value)).toEqual([
      "openai",
      "deepl",
      "google",
      "azure",
      "baidu",
      "local",
    ]);
  });

  it("renders every provider option with its label", () => {
    const html = renderToStaticMarkup(<ProviderToggle value="openai" onChange={() => {}} />);
    expect(html).toContain("OpenAI");
    expect(html).toContain("DeepL");
    expect(html).toContain("Google");
    expect(html).toContain("Azure");
    expect(html).toContain("百度");
    expect(html).toContain("本地模型");
  });

  it("marks the selected provider as pressed", () => {
    const html = renderToStaticMarkup(<ProviderToggle value="azure" onChange={() => {}} />);
    expect(html).toContain('aria-pressed="true"');
    expect(html.match(/aria-pressed="true"/g)).toHaveLength(1);
  });
});
