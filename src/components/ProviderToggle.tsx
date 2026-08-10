import { Check, Radio } from "lucide-react";
import type { ProviderId } from "../types";

/** All selectable translation providers with their UI labels. */
export const PROVIDER_OPTIONS: readonly { value: ProviderId; label: string }[] = [
  { value: "openai", label: "OpenAI" },
  { value: "deepl", label: "DeepL" },
  { value: "google", label: "Google" },
  { value: "azure", label: "Azure" },
  { value: "baidu", label: "百度" },
  { value: "local", label: "本地模型" },
];

interface ProviderToggleProps {
  value: ProviderId;
  onChange: (value: ProviderId) => void;
  /** Settings hides local-model selection because it is available on the main window. */
  includeLocal?: boolean;
}

export function ProviderToggle({ value, onChange, includeLocal = true }: ProviderToggleProps) {
  const options = includeLocal
    ? PROVIDER_OPTIONS
    : PROVIDER_OPTIONS.filter((option) => option.value !== "local");

  return (
    <div className="grid grid-cols-3 gap-2">
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          onClick={() => onChange(option.value)}
          aria-pressed={value === option.value}
          className={`flex items-center justify-between rounded-lg border px-2 py-2 text-sm transition ${value === option.value ? "border-indigo-300 bg-indigo-50 text-indigo-700" : "border-slate-200 bg-white text-slate-600 hover:border-slate-300"}`}
        >
          <span className="flex items-center gap-1.5">
            <Radio size={15} aria-hidden="true" />
            {option.label}
          </span>
          {value === option.value && <Check size={15} aria-hidden="true" />}
        </button>
      ))}
    </div>
  );
}
