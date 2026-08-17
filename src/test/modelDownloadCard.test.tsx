import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { ModelDownloadCard } from "../components/ModelDownloadCard";
import { useAppStore } from "../stores/appStore";
import { TRANSLATION_MODEL_ENTRY_ID } from "../types";
import type { ModelDownloadProgress, ModelStatusReport } from "../types";

/** Report fixture whose translation model entry state can be overridden. */
function reportWithTranslationState(
  state: "ready" | "missing" | "invalid",
): ModelStatusReport {
  return {
    entries: [
      { id: "ppocr-det-v6", state: "ready", optional: false },
      { id: "opus-mt-en-zh-tokenizer", state: "ready", optional: false },
      { id: TRANSLATION_MODEL_ENTRY_ID, state, optional: true },
    ],
    ocr_ready: true,
    translation_ready: state === "ready",
  };
}

interface CardStorePreset {
  modelStatus: ModelStatusReport | null;
  modelDownloadProgress: ModelDownloadProgress | null;
  translationModelDownloading: boolean;
}

/**
 * Presets the store fields the card reads.
 *
 * `renderToStaticMarkup` reads the SSR server snapshot, which zustand v4
 * serves from `getInitialState()` — so the preset mutates that object (same
 * pattern as the existing MainWindow SSR tests) instead of `setState`.
 */
function presetStore(preset: CardStorePreset): void {
  Object.assign(useAppStore.getInitialState(), preset);
}

function renderCard(preset: CardStorePreset): string {
  presetStore(preset);
  return renderToStaticMarkup(<ModelDownloadCard />);
}

/** Extracts the plain-text label of every rendered button, in order. */
function buttonLabels(html: string): string[] {
  const buttons = html.match(/<button[^>]*>[\s\S]*?<\/button>/g) ?? [];
  return buttons.map((button) => button.replace(/<[^>]+>/g, "").trim());
}

describe("ModelDownloadCard states", () => {
  it("shows 未安装 with only the download button when the model is missing", () => {
    const html = renderCard({
      modelStatus: reportWithTranslationState("missing"),
      modelDownloadProgress: null,
      translationModelDownloading: false,
    });
    expect(html).toContain("未安装");
    expect(buttonLabels(html)).toEqual(["下载"]);
  });

  it("shows 下载中 with progress and only the cancel button while downloading", () => {
    const html = renderCard({
      modelStatus: reportWithTranslationState("missing"),
      modelDownloadProgress: { bytes: 42, total: 100, fraction: 0.42 },
      translationModelDownloading: true,
    });
    expect(html).toContain("下载中");
    expect(html).toContain("42%");
    expect(html).toContain('role="progressbar"');
    expect(html).toContain('aria-valuenow="42"');
    expect(html).toContain("width:42%");
    expect(buttonLabels(html)).toEqual(["取消下载"]);
  });

  it("shows 0% while downloading before the first progress event arrives", () => {
    const html = renderCard({
      modelStatus: reportWithTranslationState("missing"),
      modelDownloadProgress: null,
      translationModelDownloading: true,
    });
    expect(html).toContain("0%");
    expect(html).toContain('aria-valuenow="0"');
  });

  it("clamps the download percentage at 100", () => {
    const html = renderCard({
      modelStatus: reportWithTranslationState("missing"),
      modelDownloadProgress: { bytes: 100, total: 100, fraction: 1.7 },
      translationModelDownloading: true,
    });
    expect(html).toContain("100%");
    expect(html).toContain('aria-valuenow="100"');
  });

  it("shows 已安装 with only the delete button when the model is ready", () => {
    const html = renderCard({
      modelStatus: reportWithTranslationState("ready"),
      modelDownloadProgress: null,
      translationModelDownloading: false,
    });
    expect(html).toContain("已安装");
    expect(buttonLabels(html)).toEqual(["删除"]);
  });

  it("shows 校验失败 with only the redownload button when verification failed", () => {
    const html = renderCard({
      modelStatus: reportWithTranslationState("invalid"),
      modelDownloadProgress: null,
      translationModelDownloading: false,
    });
    expect(html).toContain("校验失败");
    expect(buttonLabels(html)).toEqual(["重新下载"]);
  });

  it("shows 状态未知 with only the refresh button before any report arrives", () => {
    const html = renderCard({
      modelStatus: null,
      modelDownloadProgress: null,
      translationModelDownloading: false,
    });
    expect(html).toContain("状态未知");
    expect(buttonLabels(html)).toEqual(["刷新"]);
  });

  it("shows the download state while downloading even when the report is unknown", () => {
    const html = renderCard({
      modelStatus: null,
      modelDownloadProgress: { bytes: 7, total: 20, fraction: 0.35 },
      translationModelDownloading: true,
    });
    expect(html).toContain("下载中");
    expect(html).toContain("35%");
    expect(buttonLabels(html)).toEqual(["取消下载"]);
  });

  it("does not render a progress bar outside the downloading state", () => {
    const html = renderCard({
      modelStatus: reportWithTranslationState("missing"),
      modelDownloadProgress: null,
      translationModelDownloading: false,
    });
    expect(html).not.toContain('role="progressbar"');
  });
});
