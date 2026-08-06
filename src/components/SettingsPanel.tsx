import { useState } from "react";
import { Save } from "lucide-react";
import { publishFrontendFloaterEnabled } from "../services/events";
import { getIpcErrorMessage, saveSettings, setApiKey } from "../services/tauri";
import {
  clampFloaterOpacity,
  clampFloaterSizePx,
  persistFloaterAppearance,
} from "../services/floaterAppearance";
import type {
  AppConfig,
  CaptureConfig,
  FloatingBallConfig,
  HotkeyConfig,
  ResultWindowConfig,
  TranslationConfig,
} from "../types";
import {
  FLOATER_OPACITY_MAX,
  FLOATER_OPACITY_MIN,
  FLOATER_SIZE_MAX,
  FLOATER_SIZE_MIN,
  RESULT_FONT_SIZE_MAX,
  RESULT_FONT_SIZE_MIN,
  RESULT_OPACITY_MAX,
  RESULT_OPACITY_MIN,
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
  const opacity = config.result_window.opacity;
  if (!Number.isFinite(opacity) || opacity < RESULT_OPACITY_MIN || opacity > RESULT_OPACITY_MAX) {
    return `背景透明度必须在 ${RESULT_OPACITY_MIN} 到 ${RESULT_OPACITY_MAX} 之间`;
  }
  const fontSize = config.result_window.font_size_px;
  if (!Number.isInteger(fontSize) || fontSize < RESULT_FONT_SIZE_MIN || fontSize > RESULT_FONT_SIZE_MAX) {
    return `字体大小必须是 ${RESULT_FONT_SIZE_MIN} 到 ${RESULT_FONT_SIZE_MAX} 的整数（像素）`;
  }
  const floaterOpacity = config.floating_ball.opacity;
  if (!Number.isFinite(floaterOpacity) || floaterOpacity < FLOATER_OPACITY_MIN || floaterOpacity > FLOATER_OPACITY_MAX) {
    return `悬浮球透明度必须在 ${FLOATER_OPACITY_MIN} 到 ${FLOATER_OPACITY_MAX} 之间`;
  }
  const floaterSize = config.floating_ball.size_px;
  if (!Number.isInteger(floaterSize) || floaterSize < FLOATER_SIZE_MIN || floaterSize > FLOATER_SIZE_MAX) {
    return `悬浮球大小必须是 ${FLOATER_SIZE_MIN} 到 ${FLOATER_SIZE_MAX} 的整数（像素）`;
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
  const [apiKey, setApiKeyDraft] = useState("");
  const [savingKey, setSavingKey] = useState(false);
  const [keyMessage, setKeyMessage] = useState<{ kind: "error" | "success"; text: string } | null>(null);

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
  const setResultWindow = (key: keyof ResultWindowConfig, value: boolean | number) => {
    markDirty();
    setDraft((current) => ({
      ...current,
      result_window: { ...current.result_window, [key]: value },
    }));
  };
  const setFloatingBall = (key: keyof FloatingBallConfig, value: boolean | number) => {
    markDirty();
    setDraft((current) => ({
      ...current,
      floating_ball: { ...current.floating_ball, [key]: value },
    }));
  };
  const toggleFloatingBall = (enabled: boolean) => {
    markDirty();
    setDraft((current) => ({
      ...current,
      floating_ball: { ...current.floating_ball, enabled },
    }));
    // 切换即时生效，不等待保存；这是纯前端事件，不经过 Rust。
    void publishFrontendFloaterEnabled(enabled);
  };
  const changeFloaterOpacity = (value: number) => {
    const next = clampFloaterOpacity(value);
    setFloatingBall("opacity", next);
    // 外观即时持久化，不走整包 save_settings，实时会话期间也可保存。
    void persistFloaterAppearance(next, draft.floating_ball.size_px).catch((error) =>
      console.warn(`[vtrans] floating ball appearance persist failed: ${getIpcErrorMessage(error)}`),
    );
  };
  const changeFloaterSize = (value: number) => {
    const next = clampFloaterSizePx(value);
    setFloatingBall("size_px", next);
    void persistFloaterAppearance(draft.floating_ball.opacity, next).catch((error) =>
      console.warn(`[vtrans] floating ball appearance persist failed: ${getIpcErrorMessage(error)}`),
    );
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

  const saveKey = async () => {
    const trimmed = apiKey.trim();
    if (!trimmed) {
      setKeyMessage({ kind: "error", text: "请输入 API Key" });
      return;
    }
    setSavingKey(true);
    setKeyMessage(null);
    try {
      await setApiKey(trimmed);
      setApiKeyDraft("");
      setKeyMessage({ kind: "success", text: "API Key 已保存到系统凭据" });
    } catch (error) {
      setKeyMessage({ kind: "error", text: getIpcErrorMessage(error) });
    } finally {
      setSavingKey(false);
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
            <div className="rounded-lg border border-slate-100 bg-slate-50/60 p-2.5">
              <label className="flex flex-col gap-1">
                <span className={labelClass}>API Key（保存到 Windows 凭据，不写入配置文件）</span>
                <div className="flex gap-2">
                  <input
                    type="password"
                    value={apiKey}
                    onChange={(event) => {
                      setApiKeyDraft(event.target.value);
                      setKeyMessage(null);
                    }}
                    className={inputClass}
                    placeholder="sk-..."
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <button type="button" onClick={() => void saveKey()} disabled={savingKey} className="secondary-button shrink-0">
                    {savingKey ? "保存中…" : "保存 Key"}
                  </button>
                </div>
              </label>
              {keyMessage && (
                <p className={`mt-1.5 text-xs ${keyMessage.kind === "error" ? "text-red-600" : "text-emerald-600"}`} role={keyMessage.kind === "error" ? "alert" : "status"}>
                  {keyMessage.text}
                </p>
              )}
            </div>
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

        <label className="flex items-center gap-2 text-sm text-slate-600">
          <input
            type="checkbox"
            checked={draft.floating_ball.enabled}
            onChange={(event) => toggleFloatingBall(event.target.checked)}
            className="h-4 w-4 rounded border-slate-300 text-indigo-600 focus:ring-indigo-500"
          />
          显示悬浮球（即时生效）
        </label>

        <div className="grid grid-cols-2 gap-3 rounded-lg border border-slate-100 p-3">
          <label className="flex flex-col gap-1">
            <span className={labelClass}>悬浮球透明度</span>
            <input
              type="range"
              min={FLOATER_OPACITY_MIN}
              max={FLOATER_OPACITY_MAX}
              step={0.05}
              value={draft.floating_ball.opacity}
              onChange={(event) => changeFloaterOpacity(Number(event.target.value))}
              className="accent-indigo-600"
            />
            <span className="text-[11px] text-slate-400">{draft.floating_ball.opacity.toFixed(2)}</span>
          </label>
          <label className="flex flex-col gap-1">
            <span className={labelClass}>悬浮球大小</span>
            <input
              type="range"
              min={FLOATER_SIZE_MIN}
              max={FLOATER_SIZE_MAX}
              step={1}
              value={draft.floating_ball.size_px}
              onChange={(event) => changeFloaterSize(Number(event.target.value))}
              className="accent-indigo-600"
            />
            <span className="text-[11px] text-slate-400">{draft.floating_ball.size_px}px</span>
          </label>
        </div>

        <div className="grid grid-cols-2 gap-3 rounded-lg border border-slate-100 p-3">
          <label className="flex flex-col gap-1">
            <span className={labelClass}>弹窗背景透明度</span>
            <input
              type="range"
              min={RESULT_OPACITY_MIN}
              max={RESULT_OPACITY_MAX}
              step={0.05}
              value={draft.result_window.opacity}
              onChange={(event) => setResultWindow("opacity", Number(event.target.value))}
              className="accent-indigo-600"
            />
            <span className="text-[11px] text-slate-400">{draft.result_window.opacity.toFixed(2)}</span>
          </label>
          <label className="flex flex-col gap-1">
            <span className={labelClass}>弹窗字体大小</span>
            <input
              type="range"
              min={RESULT_FONT_SIZE_MIN}
              max={RESULT_FONT_SIZE_MAX}
              step={1}
              value={draft.result_window.font_size_px}
              onChange={(event) => setResultWindow("font_size_px", Number(event.target.value))}
              className="accent-indigo-600"
            />
            <span className="text-[11px] text-slate-400">{draft.result_window.font_size_px}px</span>
          </label>
        </div>

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
          本地模型语言对限制见"语言与引擎"提示；API Key 仅保存在系统凭据中。
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
