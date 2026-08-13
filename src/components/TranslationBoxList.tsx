import { LayoutGrid, Pencil, Plus, Square, Trash2 } from "lucide-react";
import type { BoxStatus, TranslationBoxInfo } from "../types";
import { boxCountWarningText, boxStatusLabel, isBoxError, shouldWarnBoxCount } from "../types";

interface TranslationBoxListProps {
  boxes: TranslationBoxInfo[];
  statuses: Record<number, BoxStatus>;
  /** Active-box count at which the persistent warning bar appears. */
  warningThreshold: number;
  busy?: boolean;
  onAdd: () => void;
  onEdit: (boxId: number) => void;
  onRemove: (boxId: number) => void;
  onStopBox: (boxId: number) => void;
}

/**
 * Main-window multi-box management list.
 *
 * Lists each configured translation box as a color swatch, its ordinal number
 * and runtime status, with per-box edit/remove (and stop-while-running)
 * actions. Deliberately omits coordinates/size/shape: the list is a compact
 * index, not a geometry inspector. The add button lives here; the
 * whole-session start/stop controls are owned by the main window's live-mode
 * bottom row, so the multi-box workflow keeps a single source of truth for
 * session control.
 */
export function TranslationBoxList({
  boxes,
  statuses,
  warningThreshold,
  busy = false,
  onAdd,
  onEdit,
  onRemove,
  onStopBox,
}: TranslationBoxListProps) {
  const showWarning = shouldWarnBoxCount(boxes.length, warningThreshold);

  return (
    <section className="rounded-xl border border-slate-200 bg-white p-4 shadow-sm">
      <div className="mb-3 flex items-center justify-between">
        <div>
          <h2 className="text-sm font-semibold">翻译框</h2>
          <p className="mt-1 text-xs text-slate-400">共 {boxes.length} 个翻译框</p>
        </div>
        <LayoutGrid size={18} className="text-indigo-500" aria-hidden="true" />
      </div>

      {showWarning && (
        <div
          role="alert"
          data-testid="multibox-warning"
          className="mb-3 rounded-lg bg-amber-50 px-3 py-2 text-xs font-medium text-amber-700"
        >
          {boxCountWarningText(warningThreshold)}
        </div>
      )}

      {boxes.length === 0 ? (
        <p
          data-testid="multibox-empty"
          className="rounded-lg bg-slate-50 px-3 py-5 text-center text-xs text-slate-400"
        >
          尚未添加翻译框。点击「新增翻译框」框选要翻译的区域。
        </p>
      ) : (
        <ul className="space-y-2" data-testid="multibox-list">
          {boxes.map((box, index) => {
            const status = statuses[box.box_id] ?? "Stopped";
            return (
              <li
                key={box.box_id}
                data-testid={`multibox-item-${box.box_id}`}
                className="flex items-center gap-2 rounded-lg border border-slate-200 px-2 py-1.5"
              >
                <span
                  className="h-4 w-4 shrink-0 rounded"
                  style={{ backgroundColor: box.color }}
                  aria-hidden="true"
                />
                <span className="min-w-0 flex-1 truncate text-sm font-medium text-slate-700">
                  框 {index + 1}
                </span>
                <BoxStatusBadge status={status} />
                {status === "Running" && (
                  <button
                    type="button"
                    onClick={() => onStopBox(box.box_id)}
                    className="icon-button"
                    title="停止此框"
                    aria-label={`停止框 ${index + 1}`}
                  >
                    <Square size={14} aria-hidden="true" />
                  </button>
                )}
                <button
                  type="button"
                  onClick={() => onEdit(box.box_id)}
                  className="icon-button"
                  title="编辑区域"
                  aria-label={`编辑框 ${index + 1} 区域`}
                >
                  <Pencil size={14} aria-hidden="true" />
                </button>
                <button
                  type="button"
                  onClick={() => onRemove(box.box_id)}
                  className="icon-button"
                  title="删除"
                  aria-label={`删除框 ${index + 1}`}
                >
                  <Trash2 size={14} aria-hidden="true" />
                </button>
              </li>
            );
          })}
        </ul>
      )}

      <div className="mt-3">
        <button
          type="button"
          onClick={onAdd}
          disabled={busy}
          className="secondary-button w-full"
        >
          <Plus size={16} aria-hidden="true" />
          新增翻译框
        </button>
      </div>
    </section>
  );
}

/** Compact runtime status badge for a single translation box. */
export function BoxStatusBadge({ status }: { status: BoxStatus }) {
  const errored = isBoxError(status);
  const running = status === "Running";
  return (
    <span
      className={`shrink-0 rounded-full px-2 py-0.5 text-[11px] font-medium ${
        errored ? "bg-red-50 text-red-600" : running ? "bg-emerald-50 text-emerald-600" : "bg-slate-100 text-slate-500"
      }`}
    >
      {boxStatusLabel(status)}
    </span>
  );
}
