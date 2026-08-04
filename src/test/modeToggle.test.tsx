import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { ModeToggle } from "../components/ModeToggle";

describe("ModeToggle", () => {
  it("renders both translation modes", () => {
    const html = renderToStaticMarkup(<ModeToggle value="single" onChange={() => {}} />);
    expect(html).toContain("单次翻译");
    expect(html).toContain("实时翻译");
  });

  it("marks the active mode as pressed", () => {
    const html = renderToStaticMarkup(<ModeToggle value="live" onChange={() => {}} />);
    expect(html).toContain('aria-pressed="true"');
  });

  it("disables every button while a live session is running", () => {
    const html = renderToStaticMarkup(<ModeToggle value="live" onChange={() => {}} disabled />);
    const disabledCount = (html.match(/disabled/g) ?? []).length;
    expect(disabledCount).toBe(2);
  });
});
