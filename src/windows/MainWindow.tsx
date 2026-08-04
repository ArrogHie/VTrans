import { useEffect, useMemo, useState } from "react";
import { FolderCheck, MousePointer2, Pause, Play, RefreshCw, Settings2, Square } from "lucide-react";
import { LanguageSelector } from "../components/LanguageSelector";
import { ModeToggle } from "../components/ModeToggle";
import { ProviderToggle } from "../components/ProviderToggle";
import { StatusBar } from "../components/StatusBar";
import {
  captureOnce,
  getAppStatus,
  getIpcErrorMessage,
  isRegionSelectionCancelled,
  loadLocalModels,
  publishFrontendLiveConfig,
  publishFrontendLivePaused,
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
import type { LanguageCode, Mode } from "../types";

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
    mode, status, error, selectedRegion, config, modelProgress, liveConfig, livePaused,
    setMode, setStatus, setSelectedRegion, setOcrResult, setProvider, setLiveConfig, setLivePaused, updateLanguage,
  } = useAppStore();
  const [busy, setBusy] = useState(false);
  const [modelMessage, setModelMessage] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);

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
    // 选区期间暂停实时任务，避免旧区域在框选过程中持续触发识别。
    const liveWasRunning = mode === "live" && !livePaused && Boolean(liveConfig);
    if (liveWasRunning) {
      setLivePaused(true);
      try {
        await stopLiveTranslation();
        void publishFrontendLivePaused();
      } catch (ipcError) {
        setLivePaused(false);
        setStatus({ error: getIpcErrorMessage(ipcError) });
        setBusy(false);
        return;
      }
    }
    setStatus(liveWasRunning ? "idle" : "capturing");
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
        if (!liveConfig) {
          setStatus("idle");
          return;
        }
        const updatedLiveConfig = { ...liveConfig, region };
        setLiveConfig(updatedLiveConfig);
        if (liveWasRunning) {
          await startLiveTranslation(updatedLiveConfig);
          setLivePaused(false);
          setStatus("capturing");
        } else {
          setLivePaused(false);
          setStatus("idle");
        }
        void publishFrontendLiveConfig(updatedLiveConfig);
      }
    } catch (ipcError) {
      if (isRegionSelectionCancelled(ipcError)) {
        // Esc 取消选区是正常操作；实时会话保持暂停，等待用户恢复。
        setStatus("idle");
      } else {
        setStatus({ error: getIpcErrorMessage(ipcError) });
      }
    } finally {
      setBusy(false);
    }
  };

  const runLive = async () => {
    if (!selectedRegion) {
      setStatus({ error: "请先选择翻译区域" });
      return;
    }
    if (liveConfig && !livePaused) return;
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

  const togglePause = async () => {
    setBusy(true);
    try {
      if (livePaused) {
        if (!liveConfig) return;
        await startLiveTranslation(liveConfig);
        setLivePaused(false);
        setStatus("capturing");
        void publishFrontendLiveConfig(liveConfig);
      } else {
        setLivePaused(true);
        await stopLiveTranslation();
        setStatus("idle");
        void publishFrontendLivePaused();
      }
    } catch (ipcError) {
      // 无论暂停还是恢复失败，都回滚到操作前的暂停状态。
      setLivePaused(livePaused);
      setStatus({ error: getIpcErrorMessage(ipcError) });
    } finally {
      setBusy(false);
    }
  };

  const stopLive = async () => {
    setBusy(true);
    try {
      if (liveConfig && !livePaused) {
        await stopLiveTranslation();
      }
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

  /** Switches the translation mode, stopping a live session first. */
  const switchMode = async (next: Mode) => {
    if (next === mode) return;
    if (mode === "live" && liveConfig) {
      await stopLive();
    }
    setMode(next);
  };

  const changeOcrLanguage = async (value: string) => {
    const language = value as LanguageCode;
    try {
      await setOcrLanguage(language);
      updateLanguage("ocr", language);
    } catch (ipcError) {
      setStatus({ error: getIpcErrorMessage(ipcError) });
    }
  };
  const changeProvider = async (provider: "api" | "local") => {
    try {
      await setTranslationProvider(provider);
      setProvider(provider);
    } catch (ipcError) {
      setStatus({ error: getIpcErrorMessage(ipcError) });
    }
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
        <button
          type="button"
          onClick={() => setSettingsOpen((open) => !open)}
          className="icon-button mt-1"
          title="设置"
          aria-expanded={settingsOpen}
        >
          <Settings2 size={20} aria-hidden="true" />
        </button>
      </header>

      {settingsOpen && (
        <section className="mb-4 rounded-xl border border-slate-200 bg-white p-4 shadow-sm">
          <div className="mb-2 flex items-center justify-between">
            <h2 className="text-sm font-semibold">设置</h2>
            <span className="text-xs text-slate-400">只读</span>
          </div>
          <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-sm">
            <div>
              <dt className="text-xs text-slate-400">捕获间隔</dt>
              <dd className="mt-0.5 text-slate-700">{config.capture.interval_ms} ms</dd>
            </div>
            <div>
              <dt className="text-xs text-slate-400">差异阈值</dt>
              <dd className="mt-0.5 text-slate-700">{config.capture.difference_threshold}</dd>
            </div>
            <div>
              <dt className="text-xs text-slate-400">API 超时</dt>
              <dd className="mt-0.5 text-slate-700">{config.translation.timeout_seconds} s</dd>
            </div>
            <div>
              <dt className="text-xs text-slate-400">最大重试</dt>
              <dd className="mt-0.5 text-slate-700">{config.translation.max_retries}</dd>
            </div>
            <div>
              <dt className="text-xs text-slate-400">选择并翻译</dt>
              <dd className="mt-0.5 text-slate-700">{config.hotkeys.select_and_translate}</dd>
            </div>
            <div>
              <dt className="text-xs text-slate-400">实时翻译</dt>
              <dd className="mt-0.5 text-slate-700">{config.hotkeys.live_translate}</dd>
            </div>
            <div>
              <dt className="text-xs text-slate-400">停止实时</dt>
              <dd className="mt-0.5 text-slate-700">{config.hotkeys.stop_live}</dd>
            </div>
            <div>
              <dt className="text-xs text-slate-400">结果窗口置顶</dt>
              <dd className="mt-0.5 text-slate-700">{config.result_window.always_on_top ? "是" : "否"}</dd>
            </div>
          </dl>
          <p className="mt-3 border-t border-slate-100 pt-2 text-xs text-slate-400">
            OCR 语言与翻译引擎可即时修改并保存；完整编辑将在设置 IPC 开放后提供。
          </p>
        </section>
      )}

      <div className="space-y-4">
        <ModeToggle
          value={mode}
          onChange={(next) => void switchMode(next)}
          disabled={mode === "live" && Boolean(liveConfig) && !livePaused}
        />
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
              {livePaused ? (
                <button type="button" onClick={() => void runLive()} disabled={busy} className="primary-button"><Play size={16} />继续实时</button>
              ) : (
                <button type="button" onClick={() => void togglePause()} disabled={busy || !liveConfig} className="secondary-button"><Pause size={16} />暂停</button>
              )}
              <button type="button" onClick={() => void runLive()} disabled={busy || Boolean(liveConfig && !livePaused)} className="secondary-button"><Play size={16} />开始实时</button>
              <button type="button" onClick={() => void stopLive()} disabled={busy} className="secondary-button col-span-2"><Square size={16} />停止</button>
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
        <div className="flex items-center justify-center gap-2 px-2 text-center text-xs text-slate-400"><RefreshCw size={14} />OCR 语言和翻译引擎会立即保存；完整编辑将在设置 IPC 开放后提供。</div>
      </div>
    </main>
  );
}
