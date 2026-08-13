import type { BoxStatus, BoxedTranslationResult } from "../types";
import { boxStatusLabel, isBoxError } from "../types";

/** Minimal box identity needed to render a multi-box result section. */
export interface MultiBoxEntry {
  box_id: number;
  color: string;
}

interface MultiBoxResultsProps {
  boxes: MultiBoxEntry[];
  results: Record<number, BoxedTranslationResult>;
  statuses: Record<number, BoxStatus>;
}

/**
 * Stacked multi-box translation display for the result popup.
 *
 * Boxes are laid out top-to-bottom in list order, each wrapped in a border
 * whose color matches the box's palette color (e.g. `2px solid #FF6B6B`), and
 * separated by dividers. The whole stack scrolls (`overflow-y: auto`) so an
 * arbitrary number of boxes stays usable inside the small popup. Each section
 * shows the box ordinal, its runtime status, and the latest result: the OCR
 * original text as small secondary-colored text above the translation (only
 * when non-empty, so failed/empty OCR leaves no placeholder), followed by the
 * translated text (or a stopped/error placeholder).
 */
export function MultiBoxResults({ boxes, results, statuses }: MultiBoxResultsProps) {
  // Defensive fallback: derive entries from results when the box list has not
  // been hydrated yet (e.g. the popup opened before the list command returned).
  const entries =
    boxes.length > 0
      ? boxes
      : Object.values(results)
          .sort((a, b) => a.box_id - b.box_id)
          .map((result) => ({ box_id: result.box_id, color: result.color }));

  return (
    <div className="flex-1 overflow-y-auto" data-testid="multibox-results">
      {entries.map((box, index) => {
        const result = results[box.box_id];
        const status = statuses[box.box_id] ?? "Stopped";
        const stopped = status === "Stopped";
        const body = result?.result.translated_text
          ? result.result.translated_text
          : stopped
            ? "已停止"
            : isBoxError(status)
              ? status.Error
              : "等待翻译…";
        const originalText = result?.original_text ?? "";
        return (
          <div key={box.box_id} data-testid={`multibox-result-${box.box_id}`}>
            <section
              className="rounded-lg bg-white/90 p-2"
              style={{ border: `2px solid ${box.color}` }}
            >
              <div className="mb-1 flex items-center gap-1.5">
                <span
                  className="h-2.5 w-2.5 shrink-0 rounded-full"
                  style={{ backgroundColor: box.color }}
                  aria-hidden="true"
                />
                <span className="text-[11px] font-semibold text-slate-500">
                  框 {index + 1}
                </span>
                <span
                  className={`rounded-full px-1.5 py-0.5 text-[10px] font-medium ${
                    isBoxError(status)
                      ? "bg-red-50 text-red-600"
                      : status === "Running"
                        ? "bg-emerald-50 text-emerald-600"
                        : "bg-slate-100 text-slate-500"
                  }`}
                >
                  {boxStatusLabel(status)}
                </span>
              </div>
              {originalText !== "" && (
                <p
                  className="result-text mb-1 max-h-24 overflow-y-auto whitespace-pre-wrap break-words rounded-md bg-slate-100/70 p-1.5 leading-5 text-slate-500"
                  data-testid={`multibox-original-${box.box_id}`}
                >
                  {originalText}
                </p>
              )}
              <p
                className="result-text whitespace-pre-wrap break-words leading-5 text-slate-700"
                data-testid={`multibox-translation-${box.box_id}`}
              >
                {body}
              </p>
            </section>
            {index < entries.length - 1 && (
              <div
                data-testid="multibox-divider"
                className="my-1 border-t border-slate-200"
                aria-hidden="true"
              />
            )}
          </div>
        );
      })}
    </div>
  );
}
