import { getCurrentWindow } from "@tauri-apps/api/window";
import { Pause, Play, Pin, PinOff, Square, X } from "lucide-react";
import { useEffect, useState } from "react";
import { ResultCard } from "../components/ResultCard";
import { startLiveTranslation, stopLiveTranslation } from "../services/tauri";
import { useAppStore } from "../stores/appStore";

export function ResultWindow() {
  const ocrResult = useAppStore((state) => state.ocrResult);
  const translationResult = useAppStore((state) => state.translationResult);
  const mode = useAppStore((state) => state.mode);
  const selectedRegion = useAppStore((state) => state.selectedRegion);
  const config = useAppStore((state) => state.config);
  const [alwaysOnTop, setAlwaysOnTop] = useState(true);
  const [paused, setPaused] = useState(false);

  useEffect(() => {
    void getCurrentWindow().setAlwaysOnTop(alwaysOnTop).catch(() => undefined);
  }, [alwaysOnTop]);

  const close = () => void getCurrentWindow().hide();
  const togglePause = async () => {
    if (paused) {
      if (!selectedRegion) return;
      try {
        await startLiveTranslation({
          region: selectedRegion,
          capture_interval_ms: config.capture.interval_ms,
          difference_threshold: config.capture.difference_threshold,
        });
        setPaused(false);
      } catch {
        // Keep the paused state when the backend rejects a resume request.
      }
      return;
    }
    try {
      await stopLiveTranslation();
      setPaused(true);
    } catch {
      // The main window remains the source of truth when no live task exists.
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
            <button type="button" onClick={() => void togglePause()} className="icon-button" title={paused ? "继续" : "暂停"}>
              {paused ? <Play size={16} /> : <Pause size={16} />}
            </button>
          )}
          <button type="button" onClick={close} className="icon-button" title="关闭"><X size={16} /></button>
        </div>
      </header>
      <div className="space-y-3">
        <ResultCard title="原文" text={ocrResult?.merged_text ?? ""} />
        <ResultCard title="译文" text={translationResult?.translated_text ?? ""} />
      </div>
      {mode === "live" && (
        <div className="mt-3 flex items-center gap-2 text-xs text-slate-400"><Square size={12} />实时结果会随后台事件更新</div>
      )}
    </main>
  );
}
