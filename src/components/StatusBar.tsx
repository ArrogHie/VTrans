import { AlertCircle, CheckCircle2, CircleDashed, LoaderCircle } from "lucide-react";
import type { PipelineStatus } from "../types";
import { isPipelineError, pipelineStatusLabel } from "../types";

interface StatusBarProps {
  status: PipelineStatus;
  error?: string | null;
}

export function StatusBar({ status, error }: StatusBarProps) {
  const failed = isPipelineError(status) || Boolean(error);
  const busy = status === "capturing" || status === "ocr_in_progress" || status === "translating";
  const Icon = failed ? AlertCircle : busy ? LoaderCircle : status === "completed" ? CheckCircle2 : CircleDashed;
  const label = error ?? pipelineStatusLabel(status);
  return (
    <div className={`flex items-center gap-2 rounded-lg px-3 py-2 text-sm ${failed ? "bg-red-50 text-red-700" : "bg-slate-100 text-slate-600"}`} role="status">
      <Icon size={16} className={busy ? "animate-spin" : undefined} aria-hidden="true" />
      <span className="truncate">{label}</span>
    </div>
  );
}
