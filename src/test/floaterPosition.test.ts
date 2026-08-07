import { describe, expect, it } from "vitest";
import {
  clampFloaterPosition,
  FLOATER_POSITION_KEY,
  loadFloaterPosition,
  saveFloaterPosition,
} from "../utils/floaterPosition";

const MONITORS = [
  { position: { x: 0, y: 0 }, size: { width: 1920, height: 1080 } },
  { position: { x: 1920, y: 0 }, size: { width: 2560, height: 1440 } },
];

describe("clampFloaterPosition", () => {
  it("clamps the ball into the monitor containing its centre", () => {
    expect(clampFloaterPosition({ x: 1900, y: 1060 }, [MONITORS[0]])).toEqual({
      x: 1872,
      y: 1032,
    });
  });

  it("clamps using the full window size, not the ball diameter", () => {
    // 窗口 = 球径 48 + 2×16px 透明边距 = 80：夹取必须保证整个窗口
    // （含透明边距）都在显示器内，否则球虽在屏内、阴影可能被裁。
    expect(clampFloaterPosition({ x: 1900, y: 1060 }, [MONITORS[0]], 80)).toEqual({
      x: 1840,
      y: 1000,
    });
  });

  it("falls back to the first monitor for stale positions", () => {
    expect(clampFloaterPosition({ x: 5000, y: 5000 }, MONITORS)).toEqual({
      x: 1872,
      y: 1032,
    });
  });

  it("keeps in-bounds positions unchanged", () => {
    expect(clampFloaterPosition({ x: 200, y: 300 }, MONITORS)).toEqual({ x: 200, y: 300 });
  });

  it("returns the position unchanged when no monitor is available", () => {
    expect(clampFloaterPosition({ x: 100, y: 100 }, [])).toEqual({ x: 100, y: 100 });
  });
});

describe("floater position persistence", () => {
  it("round-trips a saved position through localStorage", () => {
    const store = new Map<string, string>();
    const storage = {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => void store.set(key, value),
    };
    saveFloaterPosition(storage, { x: 320, y: 240 });
    expect(loadFloaterPosition(storage)).toEqual({ x: 320, y: 240 });
    expect(store.has(FLOATER_POSITION_KEY)).toBe(true);
  });

  it("returns null for missing or malformed values", () => {
    const storage = {
      getItem: (key: string) => (key === FLOATER_POSITION_KEY ? '{"x":"bad"}' : null),
      setItem: () => undefined,
    };
    expect(loadFloaterPosition(storage)).toBeNull();
  });
});
