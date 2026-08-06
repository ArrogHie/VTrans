import { describe, expect, it, vi } from "vitest";

const { label } = vi.hoisted(() => ({ label: "floater" }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ label }),
}));

import { applyWindowLabel, getWindowLabel } from "../utils/windowLabel";

describe("window label bootstrap", () => {
  it("resolves the label from the current Tauri window", () => {
    expect(getWindowLabel()).toBe("floater");
  });

  it("applies the label to the document element synchronously", () => {
    const dataset: Record<string, string> = {};
    const doc = { documentElement: { dataset } };
    expect(applyWindowLabel(doc)).toBe("floater");
    expect(dataset.window).toBe("floater");
  });

  it("overwrites a stale label", () => {
    const dataset: Record<string, string> = { window: "main" };
    applyWindowLabel({ documentElement: { dataset } });
    expect(dataset.window).toBe("floater");
  });
});
