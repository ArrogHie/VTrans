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
      <select value={value} onChange={(event) => onChange(event.target.value)} className="rounded-lg border border-slate-200 bg-white px-3 py-2 text-sm text-slate-800 outline-none ring-indigo-200 transition focus:ring-2">
        {options.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
      </select>
    </label>
  );
}
