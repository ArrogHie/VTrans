import { Copy, RefreshCw } from "lucide-react";
import { useState } from "react";

interface ResultCardProps {
  title: string;
  text: string;
  actionLabel?: string;
  onAction?: () => void;
}

export function ResultCard({ title, text, actionLabel, onAction }: ResultCardProps) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    } catch {
      setCopied(false);
    }
  };
  return (
    <section className="rounded-xl border border-slate-200 bg-white p-4 shadow-sm">
      <div className="mb-2 flex items-center justify-between gap-3">
        <h2 className="text-xs font-semibold uppercase tracking-wide text-slate-400">{title}</h2>
        <div className="flex items-center gap-1">
          {actionLabel && onAction && (
            <button type="button" onClick={onAction} className="rounded-md p-1.5 text-slate-400 hover:bg-slate-100 hover:text-indigo-600" title={actionLabel}>
              <RefreshCw size={15} aria-hidden="true" />
            </button>
          )}
          <button type="button" onClick={copy} disabled={!text} className="rounded-md p-1.5 text-slate-400 hover:bg-slate-100 hover:text-indigo-600 disabled:opacity-30" title={copied ? "已复制" : "复制"}>
            <Copy size={15} aria-hidden="true" />
          </button>
        </div>
      </div>
      <p className="min-h-12 whitespace-pre-wrap break-words text-sm leading-6 text-slate-700">{text || "暂无内容"}</p>
    </section>
  );
}
