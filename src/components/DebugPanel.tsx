import type { DebugFramePayload } from "../types";

/** Display cap for the optional OCR cross-check line. */
const MAX_OCR_PREVIEW_LENGTH = 120;

/** Props for the Debug-mode capture-frame panel. */
export interface DebugPanelProps {
  /** Latest debug frame received from the backend, or `null` before the first event. */
  frame: DebugFramePayload | null;
  /** Most recent OCR text to cross-check against the frame (optional). */
  ocrText?: string | null;
}

/**
 * Formats a millisecond Unix timestamp as a local wall-clock time.
 *
 * Kept as a separate helper so the panel and its tests share one definition.
 */
export function formatDebugTimestamp(timestampMs: number): string {
  return new Date(timestampMs).toLocaleTimeString("zh-CN", { hour12: false });
}

/**
 * Truncates a long text for on-screen preview.
 *
 * The debug panel shows only a bounded preview of the latest OCR text; the
 * full text stays in the regular result view and is never stored here.
 */
export function truncateForDisplay(text: string, maxLength = MAX_OCR_PREVIEW_LENGTH): string {
  const trimmed = text.trim();
  if (trimmed.length <= maxLength) return trimmed;
  return `${trimmed.slice(0, maxLength)}…`;
}

/**
 * Debug-mode capture-frame panel.
 *
 * Display-only: it renders the latest pre-OCR thumbnail with its region,
 * frame index and timestamp, plus an optional OCR cross-check line. Nothing
 * is written to storage, exported, or forwarded to other windows.
 */
export function DebugPanel({ frame, ocrText }: DebugPanelProps) {
  return (
    <section
      className="rounded-xl border border-emerald-200 bg-emerald-50/40 p-4 shadow-sm"
      aria-label="调试面板"
      data-testid="debug-panel"
    >
      <div className="mb-2 flex items-center justify-between">
        <h2 className="text-sm font-semibold text-emerald-800">Debug 模式 · 仅显示不保存</h2>
        <span className="rounded-full bg-emerald-100 px-2 py-0.5 text-[11px] font-medium text-emerald-700">
          Debug
        </span>
      </div>
      {frame ? (
        <>
          <img
            src={`data:image/jpeg;base64,${frame.image}`}
            alt="OCR 前的捕获帧缩略图"
            className="max-h-56 w-full rounded-md border border-slate-200 bg-slate-50 object-contain"
            data-testid="debug-frame-image"
          />
          <p
            className="mt-2 text-[11px] leading-relaxed text-slate-500"
            data-testid="debug-frame-meta"
          >
            帧 #{frame.frame_index} · 位置 ({frame.region.x}, {frame.region.y}) · 尺寸{" "}
            {frame.region.width} × {frame.region.height} · {frame.region.monitor_id} · 时间{" "}
            {formatDebugTimestamp(frame.timestamp_ms)}
          </p>
          {ocrText && (
            <p
              className="mt-1 border-t border-emerald-100 pt-2 text-[11px] text-slate-500"
              data-testid="debug-ocr-text"
            >
              最近识别：{truncateForDisplay(ocrText)}
            </p>
          )}
        </>
      ) : (
        <p className="py-4 text-center text-xs text-slate-400">等待捕获帧…</p>
      )}
    </section>
  );
}
