import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { FloatingBall } from "../windows/FloatingBall";

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ label: "floater" }),
  availableMonitors: () => Promise.resolve([]),
}));

vi.mock("@tauri-apps/api/dpi", () => ({
  LogicalSize: class {
    constructor(public width: number, public height: number) {}
  },
  PhysicalPosition: class {
    constructor(public x: number, public y: number) {}
  },
}));

describe("FloatingBall", () => {
  it("renders the collapsed ball without the menu", () => {
    const html = renderToStaticMarkup(<FloatingBall />);
    expect(html).toContain('aria-label="悬浮球"');
    expect(html).toContain('data-testid="floating-ball"');
    expect(html).not.toContain("框选翻译");
    expect(html).not.toContain('data-testid="floating-ball-menu"');
  });
});
