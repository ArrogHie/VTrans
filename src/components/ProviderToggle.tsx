import { Check, Radio } from "lucide-react";

interface ProviderToggleProps {
  value: "api" | "local";
  onChange: (value: "api" | "local") => void;
}

export function ProviderToggle({ value, onChange }: ProviderToggleProps) {
  return (
    <div className="grid grid-cols-2 gap-2">
      {(["api", "local"] as const).map((provider) => (
        <button key={provider} type="button" onClick={() => onChange(provider)} className={`flex items-center justify-between rounded-lg border px-3 py-2 text-sm transition ${value === provider ? "border-indigo-300 bg-indigo-50 text-indigo-700" : "border-slate-200 bg-white text-slate-600 hover:border-slate-300"}`}>
          <span className="flex items-center gap-2"><Radio size={15} aria-hidden="true" />{provider === "api" ? "云端 API" : "本地模型"}</span>
          {value === provider && <Check size={15} aria-hidden="true" />}
        </button>
      ))}
    </div>
  );
}
