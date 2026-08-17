import { Loader2 } from "lucide-react";
import { PROVIDER_OPTIONS } from "./ProviderToggle";
import type { LocalProviderBlockReason, ProviderId } from "../types";

interface ProviderSelectProps {
  value: ProviderId;
  onChange: (provider: ProviderId) => void;
  /** Whether the select is disabled (busy pipeline or switch in flight). */
  disabled: boolean;
  /** Whether a provider switch is currently in flight. */
  switching: boolean;
  /** Backend model-loading progress (0..1), `null` before any event. */
  progress: number | null;
  /**
   * Why the local engine option is unavailable, or `null` when selectable.
   *
   * Derived from `get_model_status` plus the download-in-flight marker: a
   * missing/invalid translation model or a running download disables the
   * local option only (cloud options stay usable). This is double insurance
   * with the backend rejection of switching to local.
   */
  localBlocked?: LocalProviderBlockReason | null;
}

const LOCAL_BLOCK_HINTS: Record<LocalProviderBlockReason, string> = {
  missing: "请先在设置中下载本地翻译模型",
  invalid: "本地翻译模型校验失败，请在设置中重新下载",
  downloading: "本地翻译模型下载中，完成后可切换本地引擎",
};

/**
 * Engine picker used on the main window.
 *
 * While `switching` is true the select is disabled and a status row shows a
 * spinner plus the model-loading percentage driven by `model_loading_progress`
 * events; `progress === null` falls back to a generic switching message. The
 * parent owns the switch lifecycle (busy guard, progress reset, restore).
 *
 * The local option is disabled while the translation model is unavailable or
 * downloading; a hint explains why (always visible while downloading, and when
 * the unavailable local engine is the current selection).
 */
export function ProviderSelect({
  value,
  onChange,
  disabled,
  switching,
  progress,
  localBlocked = null,
}: ProviderSelectProps) {
  const localUnavailable = localBlocked !== null;
  return (
    <label className="mt-4 flex flex-col gap-1.5">
      <span className="text-xs font-medium text-slate-500">翻译引擎</span>
      <select
        value={value}
        onChange={(event) => onChange(event.target.value as ProviderId)}
        disabled={disabled}
        className="w-full rounded-lg border border-slate-200 bg-white px-3 py-2 text-sm text-slate-800 outline-none ring-indigo-200 transition focus:ring-2"
        aria-label="翻译引擎"
      >
        {PROVIDER_OPTIONS.map((option) => (
          <option
            key={option.value}
            value={option.value}
            disabled={option.value === "local" && localUnavailable}
          >
            {option.label}
          </option>
        ))}
      </select>
      {localUnavailable && (value === "local" || localBlocked === "downloading") && (
        <p
          className="mt-2 rounded-lg bg-amber-50 px-3 py-2 text-xs text-amber-700"
          role="status"
          data-testid="local-model-block-hint"
        >
          {LOCAL_BLOCK_HINTS[localBlocked]}
        </p>
      )}
      {switching && (
        <p
          className="mt-2 flex items-center gap-2 text-xs text-slate-500"
          role="status"
          data-testid="provider-switch-progress"
        >
          <Loader2 size={14} className="animate-spin" aria-hidden="true" />
          {progress === null ? "正在切换翻译引擎…" : `模型加载中 ${Math.round(progress * 100)}%`}
        </p>
      )}
    </label>
  );
}
