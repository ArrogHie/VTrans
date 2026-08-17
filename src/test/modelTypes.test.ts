import { describe, expect, it } from "vitest";
import {
  TRANSLATION_MODEL_ENTRY_ID,
  findTranslationModelEntry,
  hasModelSetupProblems,
  localProviderBlockReason,
} from "../types";
import type { ModelStatusReport } from "../types";

/** Healthy report fixture: OCR ready, tokenizer ready, translation model ready. */
function healthyReport(overrides: Partial<ModelStatusReport> = {}): ModelStatusReport {
  return {
    entries: [
      { id: "ppocr-det-v6", state: "ready", optional: false },
      { id: "opus-mt-en-zh-tokenizer", state: "ready", optional: false },
      { id: TRANSLATION_MODEL_ENTRY_ID, state: "ready", optional: true },
    ],
    ocr_ready: true,
    translation_ready: true,
    ...overrides,
  };
}

describe("findTranslationModelEntry", () => {
  it("locates the translation model entry by its manifest id", () => {
    const report = healthyReport();
    const entry = findTranslationModelEntry(report);
    expect(entry).toEqual({ id: TRANSLATION_MODEL_ENTRY_ID, state: "ready", optional: true });
  });

  it("falls back to the optional entry when the manifest id is renamed", () => {
    const report = healthyReport({
      entries: [
        { id: "ppocr-det-v6", state: "ready", optional: false },
        { id: "some-renamed-model", state: "missing", optional: true },
      ],
    });
    expect(findTranslationModelEntry(report)?.id).toBe("some-renamed-model");
  });

  it("returns null when the report carries no translation entry", () => {
    const report = healthyReport({
      entries: [{ id: "ppocr-det-v6", state: "ready", optional: false }],
    });
    expect(findTranslationModelEntry(report)).toBeNull();
  });
});

describe("hasModelSetupProblems", () => {
  it("reports a healthy model setup as fine", () => {
    expect(hasModelSetupProblems(healthyReport())).toBe(false);
  });

  it("raises the banner when OCR is not ready", () => {
    expect(hasModelSetupProblems(healthyReport({ ocr_ready: false }))).toBe(true);
  });

  it("raises the banner when a non-optional entry is invalid", () => {
    const report = healthyReport({
      entries: [
        { id: "ppocr-det-v6", state: "ready", optional: false },
        { id: "opus-mt-en-zh-tokenizer", state: "invalid", optional: false },
        { id: TRANSLATION_MODEL_ENTRY_ID, state: "missing", optional: true },
      ],
    });
    expect(hasModelSetupProblems(report)).toBe(true);
  });

  it("does not raise the banner for an invalid optional entry alone", () => {
    // 翻译模型是 optional：损坏只进设置卡片「校验失败」，不进启动横幅。
    const report = healthyReport({
      entries: [
        { id: "ppocr-det-v6", state: "ready", optional: false },
        { id: "opus-mt-en-zh-tokenizer", state: "ready", optional: false },
        { id: TRANSLATION_MODEL_ENTRY_ID, state: "invalid", optional: true },
      ],
    });
    expect(hasModelSetupProblems(report)).toBe(false);
  });
});

describe("localProviderBlockReason", () => {
  it("never blocks before the first status report arrives", () => {
    expect(localProviderBlockReason(null, false)).toBeNull();
  });

  it("does not block a ready translation model", () => {
    expect(localProviderBlockReason(healthyReport(), false)).toBeNull();
  });

  it("does not block when the report lacks a translation entry (unknown)", () => {
    const report = healthyReport({
      entries: [{ id: "ppocr-det-v6", state: "ready", optional: false }],
    });
    expect(localProviderBlockReason(report, false)).toBeNull();
  });

  it("blocks with 'missing' when the translation model is not installed", () => {
    const report = healthyReport({
      entries: [
        { id: "ppocr-det-v6", state: "ready", optional: false },
        { id: "opus-mt-en-zh-tokenizer", state: "ready", optional: false },
        { id: TRANSLATION_MODEL_ENTRY_ID, state: "missing", optional: true },
      ],
    });
    expect(localProviderBlockReason(report, false)).toBe("missing");
  });

  it("blocks with 'invalid' when the translation model failed verification", () => {
    const report = healthyReport({
      entries: [
        { id: "ppocr-det-v6", state: "ready", optional: false },
        { id: "opus-mt-en-zh-tokenizer", state: "ready", optional: false },
        { id: TRANSLATION_MODEL_ENTRY_ID, state: "invalid", optional: true },
      ],
    });
    expect(localProviderBlockReason(report, false)).toBe("invalid");
  });

  it("blocks with 'downloading' while a download is in flight, even without a report", () => {
    expect(localProviderBlockReason(null, true)).toBe("downloading");
    expect(localProviderBlockReason(healthyReport(), true)).toBe("downloading");
  });
});
