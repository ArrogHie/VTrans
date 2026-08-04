import { getCurrentWindow } from "@tauri-apps/api/window";
import { Pause, Play, Pin, PinOff, Square, X } from "lucide-react";
import { useEffect, useState } from "react";
import { ErrorBanner } from "../components/ErrorBanner";
import { ResultCard } from "../components/ResultCard";
import {
  captureOnce,
  getIpcErrorMessage,
  publishFrontendLiveConfig,
  publishFrontendLivePaused,
  publishFrontendOcrResult,
  startLiveTranslation,
  stopLiveTranslation,
} from "../services/tauri";
import { useAppStore } from "../stores/appStore";

export function ResultWindow() {
  const ocrResult = useAppStore((state) => state.ocrResult);
  const translationResult = useAppStore((state) => state.translationResult);
  const mode = useAppStore((state) => state.mode);
  const liveConfig = useAppStore((state) => state.liveConfig);
  const livePaused = useAppStore((state) => state.livePaused);
  const setLivePaused = useAppStore((state) => state.setLivePaused);
  const setOcrResult = useAppStore((state) => state.setOcrResult);
  const setTranslationResult = useAppStore((state) => state.setTranslationResult);
  const setError = useAppStore((state) => state.setError);
  const error = useAppStore((state) => state.error);
  const [alwaysOnTop, setAlwaysOnTop] = useState(true);

  useEffect(() => {
    void getCurrentWindow().setAlwaysOnTop(alwaysOnTop).catch(() => undefined);
  }, [alwaysOnTop]);

  const close = () => void getCurrentWindow().hide();
  const togglePause = async () => {
    if (livePaused) {
      if (!liveConfig) return;
      try {
        await startLiveTranslation(liveConfig);
        setLivePaused(false);
        await publishFrontendLiveConfig(liveConfig);
      } catch {
        // Keep the paused state when the backend rejects a resume request.
      }
      return;
    }
    try {
      setLivePaused(true);
      await stopLiveTranslation();
      // The shared store drives the button in every WebView.
      await publishFrontendLivePaused();
    } catch {
      setLivePaused(false);
    }
  };

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
    } catch (error) {
      setError(getIpcErrorMessage(error));
    }
  };

  return (
    <main className="min-h-screen bg-slate-50 p-3 text-slate-900">
      <header className="mb-3 flex items-center justify-between" data-tauri-drag-region>
        <div>
          <p className="text-xs font-semibold uppercase tracking-[0.2em] text-indigo-500">VTRANS</p>
          <h1 className="text-base font-semibold">翻译结果</h1>
        </div>
        <div className="flex items-center gap-1">
          <button type="button" onClick={() => setAlwaysOnTop((value) => !value)} className="icon-button" title={alwaysOnTop ? "取消置顶" : "置顶"}>
            {alwaysOnTop ? <Pin size={16} /> : <PinOff size={16} />}
          </button>
          {mode === "live" && (
            <button type="button" onClick={() => void togglePause()} className="icon-button" title={livePaused ? "继续" : "暂停"}>
              {livePaused ? <Play size={16} /> : <Pause size={16} />}
            </button>
          )}
          <button type="button" onClick={close} className="icon-button" title="关闭"><X size={16} /></button>
        </div>
      </header>
      {error && (
        <ErrorBanner message={error} onDismiss={() => setError(null)} />
      )}
      <div className="space-y-3">
        <ResultCard title="原文" text={ocrResult?.merged_text ?? ""} />
        <ResultCard
          title="译文"
          text={translationResult?.translated_text ?? ""}
          actionLabel={mode === "single" ? "重新翻译" : undefined}
          onAction={mode === "single" ? () => void retranslate() : undefined}
        />
      </div>
      {mode === "live" && (
        <div className="mt-3 flex items-center gap-2 text-xs text-slate-400"><Square size={12} />实时结果会随后台事件更新</div>
      )}
    </main>
  );
}
