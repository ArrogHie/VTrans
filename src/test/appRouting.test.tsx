import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ label: "floater" }),
  availableMonitors: () => Promise.resolve([]),
}));

vi.mock("@tauri-apps/api/event", () => ({
  emit: () => Promise.resolve(),
  listen: () => Promise.resolve(() => undefined),
}));

vi.mock("@tauri-apps/api/dpi", () => ({
  LogicalSize: class {
    constructor(public width: number, public height: number) {}
  },
  PhysicalPosition: class {
    constructor(public x: number, public y: number) {}
  },
}));

const { default: App } = await import("../App");

describe("window routing", () => {
  it("routes the floater label to the floating ball", () => {
    const html = renderToStaticMarkup(<App />);
    expect(html).toContain('aria-label="悬浮球"');
  });
});
