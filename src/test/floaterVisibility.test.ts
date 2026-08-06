import { describe, expect, it, vi } from "vitest";
import { applyFloaterVisibility } from "../utils/floaterVisibility";

describe("applyFloaterVisibility", () => {
  it("shows the window when enabled", () => {
    const show = vi.fn().mockResolvedValue(undefined);
    const hide = vi.fn();
    applyFloaterVisibility({ show, hide }, true);
    expect(show).toHaveBeenCalledOnce();
    expect(hide).not.toHaveBeenCalled();
  });

  it("hides the window when disabled", () => {
    const show = vi.fn();
    const hide = vi.fn().mockResolvedValue(undefined);
    applyFloaterVisibility({ show, hide }, false);
    expect(hide).toHaveBeenCalledOnce();
    expect(show).not.toHaveBeenCalled();
  });

  it("swallows window failures so the UI keeps working", () => {
    const show = vi.fn().mockRejectedValue(new Error("permission denied"));
    expect(() => applyFloaterVisibility({ show, hide: vi.fn() }, true)).not.toThrow();
  });
});
