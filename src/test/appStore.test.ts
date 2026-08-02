import { beforeEach, describe, expect, it } from "vitest";
import { DEFAULT_CONFIG } from "../types";
import { useAppStore } from "../stores/appStore";

beforeEach(() => {
  useAppStore.setState({
    mode: "single",
    status: "idle",
    ocrResult: null,
    translationResult: null,
    selectedRegion: null,
    error: null,
    modelProgress: null,
    config: structuredClone(DEFAULT_CONFIG),
    hydrated: false,
  });
});

describe("appStore", () => {
  it("updates mode and status immutably", () => {
    useAppStore.getState().setMode("live");
    useAppStore.getState().setStatus("capturing");
    expect(useAppStore.getState().mode).toBe("live");
    expect(useAppStore.getState().status).toBe("capturing");
  });

  it("updates nested language settings without replacing other settings", () => {
    const before = useAppStore.getState().config;
    useAppStore.getState().updateLanguage("target", "ja");
    const after = useAppStore.getState().config;
    expect(after.translation.target_language).toBe("ja");
    expect(after.capture).toEqual(before.capture);
    expect(after.translation.provider).toBe(before.translation.provider);
  });

  it("represents errors as both visible error and pipeline error status", () => {
    useAppStore.getState().setError("后端不可用");
    expect(useAppStore.getState().error).toBe("后端不可用");
    expect(useAppStore.getState().status).toEqual({ error: "后端不可用" });
  });
});
