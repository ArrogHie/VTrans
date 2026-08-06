import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const styles = readFileSync(new URL("../styles.css", import.meta.url), "utf8");

describe("floater/result window scrollbar guards", () => {
  it("resets the global body minimum size for the floater window", () => {
    expect(styles).toContain('html[data-window="floater"] body');
  });

  it("resets the global body minimum size for the result window", () => {
    expect(styles).toContain('html[data-window="result"] body');
  });

  it("hides document overflow for both windows", () => {
    const block = styles.match(
      /html\[data-window="floater"\] body,\s*html\[data-window="result"\] body\s*\{([^}]*)\}/,
    );
    expect(block).not.toBeNull();
    const declarations = block?.[1] ?? "";
    expect(declarations).toMatch(/min-width:\s*0;/);
    expect(declarations).toMatch(/min-height:\s*0;/);
    expect(declarations).toMatch(/overflow:\s*hidden;/);
  });

  it("keeps html/body/#root transparent for both windows", () => {
    expect(styles).toContain('html[data-window="floater"] #root');
    expect(styles).toContain('html[data-window="result"] #root');
    expect(styles).toContain('html[data-window="floater"] body');
    expect(styles).toContain('html[data-window="result"] body');
  });

  it("defines the transparent padding variable on the floater root", () => {
    expect(styles).toContain('html[data-window="floater"] { --floater-padding: 16px; }');
  });

  it("keeps the main window 320px minimum untouched", () => {
    expect(styles).toMatch(/body\s*\{\s*margin:\s*0;\s*min-width:\s*320px;/);
  });
});
