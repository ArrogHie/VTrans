import { useState } from "react";
import { Save } from "lucide-react";
import { ModelDownloadCard } from "./ModelDownloadCard";
import { ProviderToggle } from "./ProviderToggle";
import { publishFrontendFloaterEnabled } from "../services/events";
import { getIpcErrorMessage, saveSettings, setProviderCredentials } from "../services/tauri";
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
  ProviderId,
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
  // 本地 ONNX provider 忽略云端参数；远程 provider 需要合法的端点。
  if (
    config.translation.provider !== "local" &&
    !/^https?:\/\/.+/.test(config.translation.api_endpoint)
  ) {
    return "API 端点必须以 http:// 或 https:// 开头";
  }
  // 只有 OpenAI 强制要求模型名；DeepL/Google 视为可选，Azure/百度忽略。
  if (config.translation.provider === "openai" && config.translation.api_model.trim() === "") {
    return "API 模型名不能为空";
  }
  // Azure 区域可选，但一旦填写必须非空（与 vtrans-config 校验一致）。
  if (config.translation.provider === "azure" && config.translation.region?.trim() === "") {
    return "Azure 区域不能为空";
  }
  // 百度 provider 必须配置 APP ID（与 vtrans-config 校验一致）。
  if (config.translation.provider === "baidu" && !config.translation.app_id?.trim()) {
    return "百度 APP ID 不能为空";
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

/** Canonical endpoint applied when a cloud provider is selected. */
const PROVIDER_DEFAULT_ENDPOINTS: Record<Exclude<ProviderId, "local">, string> = {
  openai: "https://api.openai.com/v1/chat/completions",
  deepl: "https://api-free.deepl.com/v2/translate",
  google: "https://translation.googleapis.com/language/translate/v2",
  azure: "https://api.cognitive.microsofttranslator.com/translate",
  baidu: "https://fanyi-api.baidu.com/api/trans/vip/translate",
};

const DEEPL_FREE_ENDPOINT = "https://api-free.deepl.com/v2/translate";
const DEEPL_PRO_ENDPOINT = "https://api.deepl.com/v2/translate";

type DeepLMode = "free" | "pro" | "custom";

export function SettingsPanel({ config, onSaved, onClose }: SettingsPanelProps) {
  const [draft, setDraft] = useState<AppConfig>(() => structuredClone(config));
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [saving, setSaving] = useState(false);
  const [apiKey, setApiKeyDraft] = useState("");
  const [secretDraft, setSecretDraft] = useState("");
  const [savingCredentials, setSavingCredentials] = useState(false);
  const [credentialMessage, setCredentialMessage] = useState<{
    kind: "error" | "success";
    text: string;
  } | null>(null);

  const markDirty = () => {
    setSaveError(null);
    setSaved(false);
  };
  const setCapture = (key: keyof CaptureConfig, value: number) => {
    markDirty();
    setDraft((current) => ({ ...current, capture: { ...current.capture, [key]: value } }));
  };
  const setTranslation = (key: keyof TranslationConfig, value: string | number | null) => {
    markDirty();
    setDraft((current) => ({
      ...current,
      translation: { ...current.translation, [key]: value },
    }));
  };
  /** Switches the draft provider and applies its canonical endpoint. */
  const changeProvider = (provider: ProviderId) => {
    markDirty();
    setDraft((current) => {
      const translation = { ...current.translation, provider };
      if (provider !== "local") {
        translation.api_endpoint = PROVIDER_DEFAULT_ENDPOINTS[provider];
      }
      return { ...current, translation };
    });
  };
  const changeDeepLMode = (mode: DeepLMode) => {
    if (mode === "free") setTranslation("api_endpoint", DEEPL_FREE_ENDPOINT);
    else if (mode === "pro") setTranslation("api_endpoint", DEEPL_PRO_ENDPOINT);
    else markDirty();
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

  /** Saves credentials for the draft provider into the OS credential vault. */
  const saveCredentials = async () => {
    const provider = draft.translation.provider;
    if (provider === "baidu") {
      const appId = draft.translation.app_id?.trim() ?? "";
      const secret = secretDraft.trim();
      if (!appId) {
        setCredentialMessage({ kind: "error", text: "请输入百度 APP ID" });
        return;
      }
      if (!secret) {
        setCredentialMessage({ kind: "error", text: "请输入百度 Secret" });
        return;
      }
      setSavingCredentials(true);
      setCredentialMessage(null);
      try {
        await setProviderCredentials("baidu", { appId, secret });
        setSecretDraft("");
        setCredentialMessage({ kind: "success", text: "百度凭据已保存到系统凭据" });
      } catch (error) {
        setCredentialMessage({ kind: "error", text: getIpcErrorMessage(error) });
      } finally {
        setSavingCredentials(false);
      }
      return;
    }
    const trimmed = apiKey.trim();
    if (!trimmed) {
      setCredentialMessage({ kind: "error", text: "请输入 API Key" });
      return;
    }
    setSavingCredentials(true);
    setCredentialMessage(null);
    try {
      // 显式传入草稿 provider id，避免后端 set_api_key 写入当前已保存
      // provider（草稿与已保存配置不一致时会把 Key 写到错误目标）。
      await setProviderCredentials(provider, { apiKey: trimmed });
      setApiKeyDraft("");
      setCredentialMessage({ kind: "success", text: "API Key 已保存到系统凭据" });
    } catch (error) {
      setCredentialMessage({ kind: "error", text: getIpcErrorMessage(error) });
    } finally {
      setSavingCredentials(false);
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
          <legend className="px-1 text-xs font-semibold text-slate-400">翻译引擎</legend>
          <div className="space-y-3">
            <ProviderToggle
              value={draft.translation.provider}
              onChange={changeProvider}
              includeLocal={false}
            />
            {draft.translation.provider === "local" ? (
              <p className="rounded-lg bg-slate-50 px-3 py-2 text-xs text-slate-400">
                本地 ONNX 模型不使用云端 API 参数；当前为本地模型，切换请在主界面的下拉列表完成。
              </p>
            ) : (
              <div className="space-y-3">
                {(draft.translation.provider === "openai" ||
                  draft.translation.provider === "google") && (
                  <label className="flex flex-col gap-1">
                    <span className={labelClass}>API 端点</span>
                    <input
                      type="url"
                      value={draft.translation.api_endpoint}
                      onChange={(event) => setTranslation("api_endpoint", event.target.value)}
                      className={inputClass}
                      placeholder={PROVIDER_DEFAULT_ENDPOINTS[draft.translation.provider]}
                    />
                  </label>
                )}
                {draft.translation.provider === "deepl" && (
                  <>
                    <label className="flex flex-col gap-1">
                      <span className={labelClass}>DeepL 套餐</span>
                      <select
                        value={
                          draft.translation.api_endpoint === DEEPL_FREE_ENDPOINT
                            ? "free"
                            : draft.translation.api_endpoint === DEEPL_PRO_ENDPOINT
                              ? "pro"
                              : "custom"
                        }
                        onChange={(event) => changeDeepLMode(event.target.value as DeepLMode)}
                        className={inputClass}
                      >
                        <option value="free">DeepL Free</option>
                        <option value="pro">DeepL Pro</option>
                        <option value="custom">自定义端点</option>
                      </select>
                    </label>
                    <label className="flex flex-col gap-1">
                      <span className={labelClass}>API 端点</span>
                      <input
                        type="url"
                        value={draft.translation.api_endpoint}
                        onChange={(event) => setTranslation("api_endpoint", event.target.value)}
                        className={inputClass}
                        placeholder="https://api-free.deepl.com/v2/translate"
                      />
                    </label>
                  </>
                )}
                {draft.translation.provider === "azure" && (
                  <>
                    <label className="flex flex-col gap-1">
                      <span className={labelClass}>API 端点</span>
                      <input
                        type="url"
                        value={draft.translation.api_endpoint}
                        onChange={(event) => setTranslation("api_endpoint", event.target.value)}
                        className={inputClass}
                        placeholder="https://api.cognitive.microsofttranslator.com/translate"
                      />
                    </label>
                    <label className="flex flex-col gap-1">
                      <span className={labelClass}>区域（如 eastasia）</span>
                      <input
                        type="text"
                        value={draft.translation.region ?? ""}
                        onChange={(event) => setTranslation("region", event.target.value || null)}
                        className={inputClass}
                        placeholder="eastasia"
                      />
                    </label>
                  </>
                )}
                {(draft.translation.provider === "openai" ||
                  draft.translation.provider === "google") && (
                  <label className="flex flex-col gap-1">
                    <span className={labelClass}>
                      {draft.translation.provider === "openai" ? "API 模型名" : "API 模型名（可选）"}
                    </span>
                    <input
                      type="text"
                      value={draft.translation.api_model}
                      onChange={(event) => setTranslation("api_model", event.target.value)}
                      className={inputClass}
                      placeholder="gpt-4o-mini"
                    />
                  </label>
                )}
                {draft.translation.provider === "baidu" ? (
                  <div className="rounded-lg border border-slate-100 bg-slate-50/60 p-2.5">
                    <div className="space-y-2.5">
                      <label className="flex flex-col gap-1">
                        <span className={labelClass}>百度 APP ID（保存到配置，随设置保存）</span>
                        <input
                          type="text"
                          value={draft.translation.app_id ?? ""}
                          onChange={(event) => setTranslation("app_id", event.target.value || null)}
                          className={inputClass}
                          placeholder="2026081000000000"
                          autoComplete="off"
                          spellCheck={false}
                        />
                      </label>
                      <label className="flex flex-col gap-1">
                        <span className={labelClass}>百度 Secret（保存到 Windows 凭据，不写入配置文件）</span>
                        <div className="flex gap-2">
                          <input
                            type="password"
                            value={secretDraft}
                            onChange={(event) => {
                              setSecretDraft(event.target.value);
                              setCredentialMessage(null);
                            }}
                            className={inputClass}
                            placeholder="百度 Secret"
                            autoComplete="off"
                            spellCheck={false}
                          />
                          <button
                            type="button"
                            onClick={() => void saveCredentials()}
                            disabled={savingCredentials}
                            className="secondary-button shrink-0"
                          >
                            {savingCredentials ? "保存中…" : "保存凭据"}
                          </button>
                        </div>
                      </label>
                    </div>
                    {credentialMessage && (
                      <p className={`mt-1.5 text-xs ${credentialMessage.kind === "error" ? "text-red-600" : "text-emerald-600"}`} role={credentialMessage.kind === "error" ? "alert" : "status"}>
                        {credentialMessage.text}
                      </p>
                    )}
                  </div>
                ) : (
                  <div className="rounded-lg border border-slate-100 bg-slate-50/60 p-2.5">
                    <label className="flex flex-col gap-1">
                      <span className={labelClass}>API Key（保存到 Windows 凭据，不写入配置文件）</span>
                      <div className="flex gap-2">
                        <input
                          type="password"
                          value={apiKey}
                          onChange={(event) => {
                            setApiKeyDraft(event.target.value);
                            setCredentialMessage(null);
                          }}
                          className={inputClass}
                          placeholder="sk-..."
                          autoComplete="off"
                          spellCheck={false}
                        />
                        <button
                          type="button"
                          onClick={() => void saveCredentials()}
                          disabled={savingCredentials}
                          className="secondary-button shrink-0"
                        >
                          {savingCredentials ? "保存中…" : "保存 Key"}
                        </button>
                      </div>
                    </label>
                    {credentialMessage && (
                      <p className={`mt-1.5 text-xs ${credentialMessage.kind === "error" ? "text-red-600" : "text-emerald-600"}`} role={credentialMessage.kind === "error" ? "alert" : "status"}>
                        {credentialMessage.text}
                      </p>
                    )}
                  </div>
                )}
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
            )}
          </div>
        </fieldset>

        <ModelDownloadCard />

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
