import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  ChevronDown,
  ChevronRight,
  Pause,
  Pin,
  PinOff,
  Play,
  RefreshCw,
  Settings2,
  X,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { ErrorBanner } from "../components/ErrorBanner";
import {
  applyHydratedAppearance,
  applyResultAppearance,
  clampResultFontSize,
  clampResultOpacity,
  persistResultAppearance,
} from "../services/resultAppearance";
import { getIpcErrorMessage, captureOnce, publishFrontendOcrResult } from "../services/tauri";
import { toggleLivePause } from "../services/translateActions";
import { getAppConfig } from "../services/tauri";
import { useAppStore } from "../stores/appStore";
import {
  RESULT_FONT_SIZE_MAX,
  RESULT_FONT_SIZE_MIN,
  RESULT_OPACITY_MAX,
  RESULT_OPACITY_MIN,
} from "../types";

/** Debounce delay for persisting appearance changes (ms). */
const APPEARANCE_PERSIST_MS = 350;

/**
 * Mini-bar translation popup (result window).
 *
 * The translation is the main body; the source text is collapsible and
 * hidden by default. A single toolbar row provides pin, pause/resume
 * (live), retranslate (single), appearance and close actions. Appearance
 * (font size and background alpha) is applied instantly through CSS custom
 * properties on the root node and persisted through the dedicated
 * `update_result_window_appearance` command; the text itself never fades —
 * only the background alpha changes, letting the desktop show through the
 * transparent window.
 *
 * `initialSourceOpen` is a test seam only; production always starts with the
 * source text collapsed.
 */
export function ResultWindow({
  initialSourceOpen = false,
}: { initialSourceOpen?: boolean } = {}) {
  const ocrResult = useAppStore((state) => state.ocrResult);
  const translationResult = useAppStore((state) => state.translationResult);
  const mode = useAppStore((state) => state.mode);
  const livePaused = useAppStore((state) => state.livePaused);
  const error = useAppStore((state) => state.error);
  const setError = useAppStore((state) => state.setError);
  const setOcrResult = useAppStore((state) => state.setOcrResult);
  const setTranslationResult = useAppStore((state) => state.setTranslationResult);
  const config = useAppStore((state) => state.config);

  const rootRef = useRef<HTMLElement | null>(null);
  const persistTimer = useRef<number | undefined>(undefined);
  const [alwaysOnTop, setAlwaysOnTop] = useState(config.result_window.always_on_top);
  const [sourceOpen, setSourceOpen] = useState(initialSourceOpen);
  const [appearanceOpen, setAppearanceOpen] = useState(false);
  const [opacity, setOpacityValue] = useState(() => clampResultOpacity(config.result_window.opacity));
  const [fontSizePx, setFontSizePx] = useState(() =>
    clampResultFontSize(config.result_window.font_size_px),
  );

  // 挂载首帧把当前外观写入 CSS 变量，避免闪烁。
  useEffect(() => {
    if (rootRef.current) applyResultAppearance(rootRef.current, opacity, fontSizePx);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 启动水合：每个 WebView 的 store 相互隔离，主窗口的水合不会同步到
  // 结果窗口。挂载时自行拉取持久化配置，把 opacity/font_size_px 应用到
  // 本地 state 与根节点 CSS 变量，保证重启后外观保持用户保存值。
  useEffect(() => {
    let active = true;
    void getAppConfig()
      .then((hydrated) => {
        if (!active) return;
        useAppStore.getState().setConfig(hydrated);
        const next = applyHydratedAppearance(hydrated, rootRef.current);
        setOpacityValue(next.opacity);
        setFontSizePx(next.fontSizePx);
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, []);

  // 本地外观变化（滑块或水合）时即时应用到 CSS 变量；
  // 窗口存活期间 store 配置变化（如主窗口设置面板保存）时，
  // 通过 setConfig 驱动本 effect 重新应用，保证两个 WebView 一致。
  useEffect(() => {
    if (rootRef.current) applyResultAppearance(rootRef.current, opacity, fontSizePx);
  }, [opacity, fontSizePx]);

  useEffect(() => {
    void getCurrentWindow()
      .setAlwaysOnTop(alwaysOnTop)
      .catch(() => undefined);
  }, [alwaysOnTop]);

  useEffect(() => () => window.clearTimeout(persistTimer.current), []);

  const schedulePersist = (nextOpacity: number, nextFontSizePx: number) => {
    window.clearTimeout(persistTimer.current);
    persistTimer.current = window.setTimeout(() => {
      void persistResultAppearance(nextOpacity, nextFontSizePx)
        .then(() => {
          // 后端命令已持久化，这里只把同一份值写回本地 store，
          // 让其他依赖 config 的 UI（如主窗口）保持同步。
          const current = useAppStore.getState().config;
          useAppStore.getState().setConfig({
            ...current,
            result_window: {
              ...current.result_window,
              opacity: nextOpacity,
              font_size_px: nextFontSizePx,
            },
          });
        })
        .catch((persistError) => setError(getIpcErrorMessage(persistError)));
    }, APPEARANCE_PERSIST_MS);
  };

  const changeOpacity = (value: number) => {
    const next = clampResultOpacity(value);
    setOpacityValue(next);
    if (rootRef.current) applyResultAppearance(rootRef.current, next, fontSizePx);
    schedulePersist(next, fontSizePx);
  };

  const changeFontSize = (value: number) => {
    const next = clampResultFontSize(value);
    setFontSizePx(next);
    if (rootRef.current) applyResultAppearance(rootRef.current, opacity, next);
    schedulePersist(opacity, next);
  };

  const close = () => void getCurrentWindow().hide();

  /** Re-runs a single capture on the last selected region. */
  const retranslate = async () => {
    if (mode === "live") return;
    const region = useAppStore.getState().selectedRegion;
    if (!region) return;
    // 清空上一次的译文，避免新原文与旧译文并列造成误导。
    setTranslationResult(null);
    try {
      const result = await captureOnce(region);
      setOcrResult(result);
      void publishFrontendOcrResult(result);
    } catch (captureError) {
      setError(getIpcErrorMessage(captureError));
    }
  };

  const pause = () => void toggleLivePause();

  return (
    <main
      ref={rootRef}
      data-testid="result-mini-bar"
      className="result-mini-bar flex min-h-screen flex-col gap-1.5 rounded-xl border border-slate-200 p-2 text-slate-900 shadow-lg"
    >
      {/* 无原生标题栏：整个顶栏都是拖动区域（deep），按钮等可交互元素仍可点击。 */}
      <header
        className="flex w-full select-none items-center justify-between gap-2"
        data-tauri-drag-region="deep"
      >
        <div className="flex min-w-0 items-center gap-2">
          <span className="text-[10px] font-semibold uppercase tracking-[0.2em] text-indigo-500">
            VTRANS
          </span>
          {mode === "live" && (
            <span className="rounded-full bg-indigo-50 px-1.5 py-0.5 text-[10px] font-medium text-indigo-600">
              {livePaused ? "已暂停" : "实时"}
            </span>
          )}
        </div>
        <div className="flex items-center gap-0.5">
          <button
            type="button"
            onClick={() => setAlwaysOnTop((value) => !value)}
            className="icon-button"
            title={alwaysOnTop ? "取消置顶" : "置顶"}
          >
            {alwaysOnTop ? <Pin size={14} /> : <PinOff size={14} />}
          </button>
          {mode === "live" && (
            <button
              type="button"
              onClick={pause}
              className="icon-button"
              title={livePaused ? "继续" : "暂停"}
            >
              {livePaused ? <Play size={14} /> : <Pause size={14} />}
            </button>
          )}
          {mode === "single" && (
            <button
              type="button"
              onClick={() => void retranslate()}
              className="icon-button"
              title="重新翻译"
            >
              <RefreshCw size={14} />
            </button>
          )}
          <button
            type="button"
            onClick={() => setAppearanceOpen((value) => !value)}
            className="icon-button"
            title="外观"
            aria-expanded={appearanceOpen}
          >
            <Settings2 size={14} />
          </button>
          <button type="button" onClick={close} className="result-close-button" title="关闭">
            <X size={14} />
          </button>
        </div>
      </header>

      {error && <ErrorBanner message={error} onDismiss={() => setError(null)} />}

      {appearanceOpen && (
        <div
          className="space-y-2 rounded-lg border border-slate-200 bg-white/90 p-2"
          data-testid="result-appearance"
        >
          <label className="flex flex-col gap-1 text-[11px] text-slate-500">
            背景透明度
            <input
              type="range"
              min={RESULT_OPACITY_MIN}
              max={RESULT_OPACITY_MAX}
              step={0.05}
              value={opacity}
              onChange={(event) => changeOpacity(Number(event.target.value))}
              className="accent-indigo-600"
              data-testid="result-opacity-slider"
            />
          </label>
          <label className="flex flex-col gap-1 text-[11px] text-slate-500">
            字体大小（{fontSizePx}px）
            <input
              type="range"
              min={RESULT_FONT_SIZE_MIN}
              max={RESULT_FONT_SIZE_MAX}
              step={1}
              value={fontSizePx}
              onChange={(event) => changeFontSize(Number(event.target.value))}
              className="accent-indigo-600"
              data-testid="result-font-slider"
            />
          </label>
        </div>
      )}

      <button
        type="button"
        onClick={() => setSourceOpen((value) => !value)}
        className="flex items-center gap-1 self-start rounded px-1 py-0.5 text-[11px] font-medium text-slate-400 transition hover:bg-slate-100 hover:text-slate-600"
        aria-expanded={sourceOpen}
        data-testid="result-source-toggle"
      >
        {sourceOpen ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
        原文
      </button>
      {sourceOpen && (
        <p className="result-text max-h-24 overflow-y-auto whitespace-pre-wrap break-words rounded-md bg-slate-100/70 p-1.5 leading-5 text-slate-500" data-testid="result-source-text">
          {ocrResult?.merged_text || "暂无内容"}
        </p>
      )}

      <p
        className="result-text flex-1 whitespace-pre-wrap break-words leading-6 text-slate-800"
        data-testid="result-translation-text"
        data-tauri-drag-region
      >
        {translationResult?.translated_text || "等待翻译…"}
      </p>
    </main>
  );
}
