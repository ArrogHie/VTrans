import { useEffect, useMemo, useState } from "react";
import { FolderCheck, MousePointer2, Pause, Play, RefreshCw, Settings2, Square } from "lucide-react";
import { DebugPanel } from "../components/DebugPanel";
import { LanguageSelector } from "../components/LanguageSelector";
import { ModeToggle } from "../components/ModeToggle";
import { ProviderSelect } from "../components/ProviderSelect";
import { ResultCard } from "../components/ResultCard";
import { SettingsPanel } from "../components/SettingsPanel";
import { StatusBar } from "../components/StatusBar";
import { useDebugFrame } from "../hooks/useDebugFrame";
import { showRegionOverlay } from "../services/regionOverlay";
import {
  getAppConfig,
  getAppStatus,
  getIpcErrorMessage,
  loadLocalModels,
  setOcrLanguage,
  setTargetLanguage,
  setTranslationProvider,
} from "../services/tauri";
import {
  selectAndTranslateOnce,
  selectRegionForLive,
  startLive,
  stopLive as stopLiveSession,
  toggleLivePause,
} from "../services/translateActions";
import { useAppStore } from "../stores/appStore";
import { regionPreviewBox } from "../utils/regionPreview";
import { shouldRestoreOverlay } from "../utils/overlayVisibility";
import { isLocalPairSupported } from "../types";
import type { LanguageCode, Mode, ProviderId } from "../types";

const OCR_LANGUAGES = [
  { value: "auto", label: "自动检测" },
  { value: "ja", label: "日语" },
  { value: "en", label: "英语" },
  { value: "zh-CN", label: "简体中文" },
] as const;
const TARGET_LANGUAGES = [
  { value: "zh-CN", label: "中文" },
  { value: "ja", label: "日语" },
  { value: "en", label: "英语" },
] as const;

export function MainWindow() {
  const {
    mode, status, error, selectedRegion, config, modelProgress, liveConfig, livePaused,
    providerSwitching,
    setMode, setStatus, setSelectedRegion, setProvider, setConfig, updateLanguage,
    setProviderSwitching, setModelProgress,
  } = useAppStore();
  const ocrResult = useAppStore((state) => state.ocrResult);
  const translationResult = useAppStore((state) => state.translationResult);
  const [busy, setBusy] = useState(false);
  const [modelMessage, setModelMessage] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [debugMode, setDebugMode] = useState(false);
  const debugFrame = useDebugFrame(debugMode);

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const [config, snapshot] = await Promise.all([getAppConfig(), getAppStatus()]);
        if (!active) return;
        // 先水合真实配置再应用状态快照，保证整包 save_settings
        // 不会用前端默认值覆盖后端字段（OCR 语言、日志级别等）。
        useAppStore.getState().setConfig(config);
        useAppStore.getState().applyStatus(snapshot);
        setDebugMode(snapshot.debug_mode);
        if (snapshot.selected_region) {
          setSelectedRegion(snapshot.selected_region);
          // 仅实时会话模式恢复常驻选区方框；单次模式不恢复。
          if (shouldRestoreOverlay(snapshot)) {
            void showRegionOverlay(snapshot.selected_region);
          }
        }
      } catch {
        // 水合失败时保留默认配置与初始状态，用户仍可手动操作。
      }
    })();
    return () => {
      active = false;
    };
  }, [setSelectedRegion]);

  const regionLabel = useMemo(() => {
    if (!selectedRegion) return "尚未选择区域";
    return `${selectedRegion.width} × ${selectedRegion.height} · ${selectedRegion.monitor_id}`;
  }, [selectedRegion]);

  const selectRegion = async () => {
    setBusy(true);
    try {
      // 单次模式：框选并翻译；实时模式：框选后启动/重启实时会话。
      // 状态机与错误处理统一由 translateActions 服务承担，与悬浮球共用。
      await (mode === "single" ? selectAndTranslateOnce() : selectRegionForLive());
    } finally {
      setBusy(false);
    }
  };

  const runLive = async () => {
    setBusy(true);
    try {
      await startLive();
    } finally {
      setBusy(false);
    }
  };

  const togglePause = async () => {
    setBusy(true);
    try {
      await toggleLivePause();
    } finally {
      setBusy(false);
    }
  };

  const stopLive = async () => {
    setBusy(true);
    try {
      await stopLiveSession();
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
  const changeTargetLanguage = async (value: string) => {
    const language = value as Exclude<LanguageCode, "auto">;
    try {
      await setTargetLanguage(language);
      updateLanguage("target", language);
    } catch (ipcError) {
      setStatus({ error: getIpcErrorMessage(ipcError) });
    }
  };
  const changeProvider = async (provider: ProviderId) => {
    // Ignore re-entry while a switch is already in flight, and skip no-op
    // selections of the currently active provider.
    if (providerSwitching || provider === config.translation.provider) return;
    setProviderSwitching(true);
    // Clear a stale progress value from a previous switch so a cached hit
    // does not flash an old percentage before the new progress arrives.
    setModelProgress(null);
    try {
      await setTranslationProvider(provider);
      setProvider(provider);
    } catch (ipcError) {
      setStatus({ error: getIpcErrorMessage(ipcError) });
    } finally {
      setProviderSwitching(false);
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
        <SettingsPanel
          config={config}
          onSaved={(next) => {
            setConfig(next);
            setStatus("idle");
          }}
          onClose={() => setSettingsOpen(false)}
        />
      )}

      <div className="space-y-4">
        <ModeToggle
          value={mode}
          onChange={(next) => void switchMode(next)}
          disabled={busy || (mode === "live" && Boolean(liveConfig) && !livePaused)}
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
          {selectedRegion && (
            <div className="mt-3">
              <div className="relative mx-auto h-24 w-40 overflow-hidden rounded-md border border-slate-200 bg-slate-50">
                <div
                  className="absolute rounded border-2 border-indigo-400 bg-indigo-400/20"
                  style={regionPreviewBox(selectedRegion, 160, 96)}
                  data-testid="region-preview"
                />
              </div>
              <p className="mt-1 text-center text-[11px] text-slate-400">
                位置 ({selectedRegion.x}, {selectedRegion.y}) · {selectedRegion.width} × {selectedRegion.height}（物理像素）
              </p>
            </div>
          )}
        </section>

        <section className="rounded-xl border border-slate-200 bg-white p-4 shadow-sm">
          <div className="mb-3 flex items-center justify-between">
            <h2 className="text-sm font-semibold">翻译结果</h2>
            {mode === "live" && (
              <span className="rounded-full bg-indigo-50 px-2 py-0.5 text-[11px] font-medium text-indigo-600">
                {livePaused ? "已暂停" : "实时更新中"}
              </span>
            )}
          </div>
          <div className="space-y-3">
            <ResultCard title="原文" text={ocrResult?.merged_text ?? ""} />
            <ResultCard title="译文" text={translationResult?.translated_text ?? ""} />
          </div>
        </section>

        {debugMode && <DebugPanel frame={debugFrame} ocrText={ocrResult?.merged_text ?? null} />}

        <section className="rounded-xl border border-slate-200 bg-white p-4 shadow-sm">
          <h2 className="mb-3 text-sm font-semibold">语言与引擎</h2>
          <div className="flex gap-2">
            {/* OCR 识别语言与翻译源语言经后端联动恒相等，合并为单一选择器。 */}
            <LanguageSelector label="识别语言" value={config.ocr.language} options={OCR_LANGUAGES} onChange={(value) => void changeOcrLanguage(value)} />
            <LanguageSelector label="目标语言" value={config.translation.target_language} options={TARGET_LANGUAGES} onChange={(value) => void changeTargetLanguage(value)} />
          </div>
          <ProviderSelect
            value={config.translation.provider}
            onChange={(provider) => void changeProvider(provider)}
            disabled={providerSwitching || busy}
            switching={providerSwitching}
            progress={modelProgress}
          />
          {config.translation.provider === "local" && !isLocalPairSupported(config) && (
            <p className="mt-2 rounded-lg bg-amber-50 px-3 py-2 text-xs text-amber-700">
              本地模型目前仅支持 en → zh-CN，且不能自动判断源语言；其它源语言请切换到云端 Provider。
            </p>
          )}
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
        <div className="flex items-center justify-center gap-2 px-2 text-center text-xs text-slate-400"><RefreshCw size={14} />识别语言与翻译引擎即时保存；API 参数与凭据在设置面板保存（凭据仅存入系统凭据）。</div>
      </div>
    </main>
  );
}
