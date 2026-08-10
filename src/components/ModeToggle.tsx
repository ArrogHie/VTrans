import type { Mode } from "../types";

interface ModeToggleProps {
  value: Mode;
  onChange: (mode: Mode) => void;
  /** Disable switching while a live session is running. */
  disabled?: boolean;
}

export function ModeToggle({ value, onChange, disabled = false }: ModeToggleProps) {
  return (
    <div className="grid grid-cols-2 gap-2 rounded-xl bg-slate-100 p-1" aria-label="翻译模式">
      {(["single", "live"] as const).map((mode) => (
        <button
          key={mode}
          type="button"
          disabled={disabled}
          className={`rounded-lg px-3 py-2 text-sm font-medium transition ${
            disabled
              ? "cursor-not-allowed opacity-40"
              : value === mode
                ? "bg-white text-indigo-700 shadow-sm"
                : "text-slate-500 hover:text-slate-800"
          }`}
          onClick={() => onChange(mode)}
          aria-pressed={value === mode}
        >
          {mode === "single" ? "单次翻译" : "实时翻译"}
        </button>
      ))}
    </div>
  );
}

interface LanguageSelectorProps {
  label: string;
  value: string;
  options: readonly { value: string; label: string }[];
  onChange: (value: string) => void;
}

export function LanguageSelector({ label, value, options, onChange }: LanguageSelectorProps) {
  return (
    <label className="flex flex-1 flex-col gap-1.5 text-xs font-medium text-slate-500">
      {label}
      <select
        value={value}
        onChange={(event) => onChange(event.target.value)}
        className="rounded-lg border border-slate-200 bg-white px-3 py-2 text-sm text-slate-800 outline-none ring-indigo-200 transition focus:ring-2"
      >
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
    </label>
  );
}
