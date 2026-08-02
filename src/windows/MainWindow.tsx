import { useEffect, useMemo, useState } from "react";
import { FolderCheck, MousePointer2, Play, RefreshCw, Settings2, Square } from "lucide-react";
import { LanguageSelector } from "../components/LanguageSelector";
import { ModeToggle } from "../components/ModeToggle";
import { ProviderToggle } from "../components/ProviderToggle";
import { StatusBar } from "../components/StatusBar";
import {
  captureOnce,
  getAppStatus,
  getIpcErrorMessage,
  loadLocalModels,
  publishFrontendLiveConfig,
  publishFrontendLiveStopped,
  publishFrontendOcrResult,
  setOcrLanguage,
  setTranslationProvider,
  showResultWindow,
  startLiveTranslation,
  startRegionSelection,
  stopLiveTranslation,
} from "../services/tauri";
import { useAppStore } from "../stores/appStore";
import type { LanguageCode } from "../types";

const OCR_LANGUAGES = [
  { value: "auto", label: "自动检测" },
  { value: "ja", label: "日语" },
  { value: "en", label: "英语" },
  { value: "zh-CN", label: "简体中文" },
] as const;
const SOURCE_LANGUAGES = OCR_LANGUAGES;
const TARGET_LANGUAGES = [
  { value: "zh-CN", label: "中文" },
  { value: "ja", label: "日语" },
  { value: "en", label: "英语" },
] as const;

export function MainWindow() {
  const {
    mode, status, error, selectedRegion, config, modelProgress,
    setMode, setStatus, setSelectedRegion, setOcrResult, setProvider, setLiveConfig, setLivePaused, updateLanguage,
  } = useAppStore();
  const [busy, setBusy] = useState(false);
  const [modelMessage, setModelMessage] = useState<string | null>(null);

  useEffect(() => {
    void getAppStatus().then((snapshot) => {
      useAppStore.getState().applyStatus(snapshot);
      if (snapshot.selected_region) setSelectedRegion(snapshot.selected_region);
    }).catch(() => undefined);
  }, [setSelectedRegion]);

  const regionLabel = useMemo(() => {
    if (!selectedRegion) return "尚未选择区域";
    return `${selectedRegion.width} × ${selectedRegion.height} · ${selectedRegion.monitor_id}`;
  }, [selectedRegion]);

  const selectRegion = async () => {
    setBusy(true);
    setStatus("capturing");
    try {
      const region = await startRegionSelection();
      setSelectedRegion(region);
      if (mode === "single") {
        const result = await captureOnce(region);
        setOcrResult(result);
        void publishFrontendOcrResult(result);
        setStatus("completed");
        void showResultWindow();
      } else {
        setStatus("idle");
      }
    } catch (ipcError) {
      setStatus({ error: getIpcErrorMessage(ipcError) });
    } finally {
      setBusy(false);
    }
  };

  const runLive = async () => {
    if (!selectedRegion) {
      setStatus({ error: "请先选择翻译区域" });
      return;
    }
    setBusy(true);
    try {
      const liveConfig = {
        region: selectedRegion,
        capture_interval_ms: config.capture.interval_ms,
        difference_threshold: config.capture.difference_threshold,
      };
      await startLiveTranslation(liveConfig);
      setLiveConfig(liveConfig);
      setLivePaused(false);
      void publishFrontendLiveConfig(liveConfig);
      setMode("live");
      setStatus("capturing");
    } catch (ipcError) {
      setStatus({ error: getIpcErrorMessage(ipcError) });
    } finally {
      setBusy(false);
    }
  };

  const stopLive = async () => {
    setBusy(true);
    try {
      await stopLiveTranslation();
      void publishFrontendLiveStopped();
      setLiveConfig(null);
      setLivePaused(false);
      setStatus("idle");
      setMode("single");
    } catch (ipcError) {
      setStatus({ error: getIpcErrorMessage(ipcError) });
    } finally {
      setBusy(false);
    }
  };

  const changeOcrLanguage = async (value: string) => {
    const language = value as LanguageCode;
    updateLanguage("ocr", language);
    try { await setOcrLanguage(language); } catch (ipcError) { setStatus({ error: getIpcErrorMessage(ipcError) }); }
  };
  const changeProvider = async (provider: "api" | "local") => {
    setProvider(provider);
    try { await setTranslationProvider(provider); } catch (ipcError) { setStatus({ error: getIpcErrorMessage(ipcError) }); }
  };
  const loadModels = async () => {
    setModelMessage(null);
    try {
      const report = await loadLocalModels();
      setModelMessage(report.failed.length === 0 ? "本地模型校验通过" : "本地模型需要检查");
    } catch (ipcError) {
      setModelMessage(getIpcErrorMessage(ipcError));
    }
  };

  const disabled = busy || status === "ocr_in_progress" || status === "translating";

  return (
    <main className="min-h-screen bg-slate-50 px-5 py-6 text-slate-900">
      <header className="mb-6 flex items-start justify-between" data-tauri-drag-region>
        <div>
          <p className="text-xs font-semibold uppercase tracking-[0.25em] text-indigo-500">VTRANS</p>
          <h1 className="mt-1 text-2xl font-bold tracking-tight">屏幕翻译</h1>
          <p className="mt-1 text-sm text-slate-500">选择区域，开始翻译。</p>
        </div>
        <Settings2 className="mt-1 text-slate-300" size={20} aria-hidden="true" />
      </header>

      <div className="space-y-4">
        <ModeToggle value={mode} onChange={setMode} />
        <StatusBar status={status} error={error} />

        <section className="rounded-xl border border-slate-200 bg-white p-4 shadow-sm">
          <div className="mb-3 flex items-center justify-between">
            <div>
              <h2 className="text-sm font-semibold">翻译区域</h2>
              <p className="mt-1 text-xs text-slate-400">{regionLabel}</p>
            </div>
            <MousePointer2 size={18} className="text-indigo-500" aria-hidden="true" />
          </div>
          <button type="button" onClick={() => void selectRegion()} disabled={disabled} className="primary-button w-full">
            <MousePointer2 size={16} />选择屏幕区域
          </button>
        </section>

        <section className="rounded-xl border border-slate-200 bg-white p-4 shadow-sm">
          <h2 className="mb-3 text-sm font-semibold">语言与引擎</h2>
          <div className="flex gap-2">
            <LanguageSelector label="OCR 语言" value={config.ocr.language} options={OCR_LANGUAGES} onChange={(value) => void changeOcrLanguage(value)} />
            <LanguageSelector label="源语言（待 app IPC）" value={config.translation.source_language} options={SOURCE_LANGUAGES} disabled onChange={() => undefined} />
            <LanguageSelector label="目标语言（待 app IPC）" value={config.translation.target_language} options={TARGET_LANGUAGES} disabled onChange={() => undefined} />
          </div>
          <div className="mt-4"><ProviderToggle value={config.translation.provider} onChange={(value) => void changeProvider(value)} /></div>
        </section>

        <section className="grid grid-cols-2 gap-2">
          {mode === "single" ? (
            <button type="button" onClick={() => void selectRegion()} disabled={disabled} className="primary-button col-span-2"><Play size={16} />选择并翻译</button>
          ) : (
            <>
              <button type="button" onClick={() => void runLive()} disabled={disabled || Boolean(useAppStore.getState().status === "capturing")} className="primary-button"><Play size={16} />开始实时</button>
              <button type="button" onClick={() => void stopLive()} disabled={busy} className="secondary-button"><Square size={16} />停止</button>
            </>
          )}
        </section>

        <section className="rounded-xl border border-slate-200 bg-white p-4 shadow-sm">
          <div className="flex items-center justify-between">
            <div><h2 className="text-sm font-semibold">本地模型</h2><p className="mt-1 text-xs text-slate-400">{modelProgress === null ? "未校验" : `${Math.round(modelProgress * 100)}%`}</p></div>
            <button type="button" onClick={() => void loadModels()} className="icon-button" title="校验模型"><FolderCheck size={17} /></button>
          </div>
          {modelMessage && <p className="mt-2 text-xs text-slate-500">{modelMessage}</p>}
        </section>
        <div className="flex items-center justify-center gap-2 px-2 text-center text-xs text-slate-400"><RefreshCw size={14} />OCR 语言和翻译引擎会立即保存；其余设置将在配置界面开放后保存。</div>
      </div>
    </main>
  );
}
