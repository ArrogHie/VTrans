import { useState } from "react";
import { Save } from "lucide-react";
import { getIpcErrorMessage, saveSettings } from "../services/tauri";
import type {
  AppConfig,
  CaptureConfig,
  HotkeyConfig,
  ResultWindowConfig,
  TranslationConfig,
} from "../types";

interface SettingsPanelProps {
  config: AppConfig;
  onSaved: (config: AppConfig) => void;
  onClose: () => void;
}

/**
 * Validates the editable settings and returns a user-facing error message,
 * or `null` when the draft is valid.
 */
export function validateSettings(config: AppConfig): string | null {
  if (!Number.isInteger(config.capture.interval_ms) || config.capture.interval_ms <= 0) {
    return "捕获间隔必须是大于 0 的整数（毫秒）";
  }
  const threshold = config.capture.difference_threshold;
  if (!Number.isFinite(threshold) || threshold < 0 || threshold > 1) {
    return "差异阈值必须在 0 到 1 之间";
  }
  if (!/^https?:\/\/.+/.test(config.translation.api_endpoint)) {
    return "API 端点必须以 http:// 或 https:// 开头";
  }
  if (config.translation.api_model.trim() === "") {
    return "API 模型名不能为空";
  }
  if (!Number.isInteger(config.translation.timeout_seconds) || config.translation.timeout_seconds <= 0) {
    return "API 超时必须是大于 0 的整数（秒）";
  }
  if (!Number.isInteger(config.translation.max_retries) || config.translation.max_retries < 0) {
    return "最大重试次数必须是非负整数";
  }
  for (const value of Object.values(config.hotkeys)) {
    if (value.trim() === "") return "快捷键不能为空";
  }
  return null;
}

const inputClass =
  "w-full rounded-lg border border-slate-200 bg-white px-3 py-1.5 text-sm text-slate-800 outline-none ring-indigo-200 transition focus:ring-2";
const labelClass = "text-xs font-medium text-slate-500";

export function SettingsPanel({ config, onSaved, onClose }: SettingsPanelProps) {
  const [draft, setDraft] = useState<AppConfig>(() => structuredClone(config));
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [saving, setSaving] = useState(false);

  const markDirty = () => {
    setSaveError(null);
    setSaved(false);
  };
  const setCapture = (key: keyof CaptureConfig, value: number) => {
    markDirty();
    setDraft((current) => ({ ...current, capture: { ...current.capture, [key]: value } }));
  };
  const setTranslation = (key: keyof TranslationConfig, value: string | number) => {
    markDirty();
    setDraft((current) => ({
      ...current,
      translation: { ...current.translation, [key]: value },
    }));
  };
  const setHotkey = (key: keyof HotkeyConfig, value: string) => {
    markDirty();
    setDraft((current) => ({ ...current, hotkeys: { ...current.hotkeys, [key]: value } }));
  };
  const setResultWindow = (key: keyof ResultWindowConfig, value: boolean) => {
    markDirty();
    setDraft((current) => ({
      ...current,
      result_window: { ...current.result_window, [key]: value },
    }));
  };

  const save = async () => {
    const message = validateSettings(draft);
    if (message) {
      setSaveError(message);
      return;
    }
    setSaving(true);
    setSaveError(null);
    try {
      await saveSettings(draft);
      onSaved(draft);
      setSaved(true);
    } catch (error) {
      setSaveError(getIpcErrorMessage(error));
    } finally {
      setSaving(false);
    }
  };

  return (
    <section className="mb-4 rounded-xl border border-slate-200 bg-white p-4 shadow-sm">
      <div className="mb-3 flex items-center justify-between">
        <h2 className="text-sm font-semibold">设置</h2>
        <button type="button" onClick={onClose} className="text-xs text-slate-400 hover:text-slate-600">
          收起
        </button>
      </div>

      <div className="space-y-3">
        <fieldset className="rounded-lg border border-slate-100 p-3">
          <legend className="px-1 text-xs font-semibold text-slate-400">采集</legend>
          <div className="grid grid-cols-2 gap-3">
            <label className="flex flex-col gap-1">
              <span className={labelClass}>捕获间隔 (ms)</span>
              <input
                type="number"
                min={50}
                step={50}
                value={draft.capture.interval_ms}
                onChange={(event) => setCapture("interval_ms", event.target.valueAsNumber || 0)}
                className={inputClass}
              />
            </label>
            <label className="flex flex-col gap-1">
              <span className={labelClass}>差异阈值</span>
              <input
                type="number"
                min={0}
                max={1}
                step={0.01}
                value={draft.capture.difference_threshold}
                onChange={(event) => setCapture("difference_threshold", event.target.valueAsNumber || 0)}
                className={inputClass}
              />
            </label>
          </div>
        </fieldset>

        <fieldset className="rounded-lg border border-slate-100 p-3">
          <legend className="px-1 text-xs font-semibold text-slate-400">云端 API</legend>
          <div className="space-y-3">
            <label className="flex flex-col gap-1">
              <span className={labelClass}>API 端点</span>
              <input
                type="url"
                value={draft.translation.api_endpoint}
                onChange={(event) => setTranslation("api_endpoint", event.target.value)}
                className={inputClass}
                placeholder="https://api.openai.com/v1/chat/completions"
              />
            </label>
            <label className="flex flex-col gap-1">
              <span className={labelClass}>API 模型名</span>
              <input
                type="text"
                value={draft.translation.api_model}
                onChange={(event) => setTranslation("api_model", event.target.value)}
                className={inputClass}
                placeholder="gpt-4o-mini"
              />
            </label>
            <div className="grid grid-cols-2 gap-3">
              <label className="flex flex-col gap-1">
                <span className={labelClass}>超时 (秒)</span>
                <input
                  type="number"
                  min={1}
                  value={draft.translation.timeout_seconds}
                  onChange={(event) => setTranslation("timeout_seconds", event.target.valueAsNumber || 0)}
                  className={inputClass}
                />
              </label>
              <label className="flex flex-col gap-1">
                <span className={labelClass}>最大重试</span>
                <input
                  type="number"
                  min={0}
                  value={draft.translation.max_retries}
                  onChange={(event) => setTranslation("max_retries", event.target.valueAsNumber || 0)}
                  className={inputClass}
                />
              </label>
            </div>
          </div>
        </fieldset>

        <fieldset className="rounded-lg border border-slate-100 p-3">
          <legend className="px-1 text-xs font-semibold text-slate-400">快捷键</legend>
          <div className="grid grid-cols-3 gap-3">
            <label className="flex flex-col gap-1">
              <span className={labelClass}>选择并翻译</span>
              <input
                type="text"
                value={draft.hotkeys.select_and_translate}
                onChange={(event) => setHotkey("select_and_translate", event.target.value)}
                className={inputClass}
              />
            </label>
            <label className="flex flex-col gap-1">
              <span className={labelClass}>实时翻译</span>
              <input
                type="text"
                value={draft.hotkeys.live_translate}
                onChange={(event) => setHotkey("live_translate", event.target.value)}
                className={inputClass}
              />
            </label>
            <label className="flex flex-col gap-1">
              <span className={labelClass}>停止实时</span>
              <input
                type="text"
                value={draft.hotkeys.stop_live}
                onChange={(event) => setHotkey("stop_live", event.target.value)}
                className={inputClass}
              />
            </label>
          </div>
        </fieldset>

        <label className="flex items-center gap-2 text-sm text-slate-600">
          <input
            type="checkbox"
            checked={draft.result_window.always_on_top}
            onChange={(event) => setResultWindow("always_on_top", event.target.checked)}
            className="h-4 w-4 rounded border-slate-300 text-indigo-600 focus:ring-indigo-500"
          />
          结果窗口默认置顶
        </label>

        {saveError && (
          <p className="rounded-lg bg-red-50 px-3 py-2 text-xs text-red-700" role="alert">
            {saveError}
          </p>
        )}
        {saved && (
          <p className="rounded-lg bg-emerald-50 px-3 py-2 text-xs text-emerald-700" role="status">
            设置已保存
          </p>
        )}

        <p className="rounded-lg bg-slate-50 px-3 py-2 text-xs text-slate-400">
          API Key 存储依赖 vtrans-app 的凭据命令（Credential Manager），暂未开放；本地模型语言对限制见"语言与引擎"提示。
        </p>
      </div>

      <div className="mt-3 flex justify-end gap-2 border-t border-slate-100 pt-3">
        <button type="button" onClick={onClose} className="secondary-button">
          取消
        </button>
        <button type="button" onClick={() => void save()} disabled={saving} className="primary-button">
          <Save size={15} aria-hidden="true" />
          {saving ? "保存中…" : "保存设置"}
        </button>
      </div>
    </section>
  );
}
