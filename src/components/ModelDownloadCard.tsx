import { useEffect, useRef, useState } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  Download,
  Loader2,
  RefreshCw,
  Trash2,
} from "lucide-react";
import { onModelDownloadProgress } from "../services/events";
import {
  applyModelDownloadProgress,
  cancelModelDownload,
  deleteModel,
  downloadModel,
  refreshModelStatus,
} from "../services/modelActions";
import { getIpcErrorMessage } from "../services/tauri";
import { useAppStore } from "../stores/appStore";
import { findTranslationModelEntry } from "../types";
import type { ModelState } from "../types";

const STATE_LABELS: Record<ModelState, string> = {
  ready: "已安装",
  missing: "未安装",
  invalid: "校验失败",
};

/**
 * Settings card for the downloadable local translation model.
 *
 * 状态来源：挂载时 `get_model_status` 水合（经 modelActions 写入 store）；
 * 进度经 `model_download_progress` 事件实时更新；下载/取消/删除命令结算后
 * 重新 `get_model_status` 刷新终态。下载状态全部存于 Zustand store，因此
 * 设置面板关闭不中断后端下载，重新挂载即按最新状态与持续推送的进度事件水合。
 * 前端不保存任何模型内容，进度数据只进内存状态。
 */
export function ModelDownloadCard() {
  const modelStatus = useAppStore((state) => state.modelStatus);
  const downloading = useAppStore((state) => state.translationModelDownloading);
  const progress = useAppStore((state) => state.modelDownloadProgress);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  /** 用户主动取消时抑制下载 promise 的取消语义错误（不当作失败展示）。 */
  const cancelRequested = useRef(false);

  useEffect(() => {
    let disposed = false;
    let unlistenFn: (() => void) | undefined;
    void refreshModelStatus().catch((refreshError) => {
      // 状态拉取失败不阻塞卡片：保留上一次已知状态，仅记录。
      console.warn(
        `[vtrans] model status hydration failed: ${getIpcErrorMessage(refreshError)}`,
      );
    });
    void onModelDownloadProgress((payload) => {
      if (disposed) return;
      applyModelDownloadProgress(payload);
    }).then((unlisten) => {
      if (disposed) unlisten();
      else unlistenFn = unlisten;
    });
    return () => {
      disposed = true;
      unlistenFn?.();
    };
  }, []);

  const entry = modelStatus ? findTranslationModelEntry(modelStatus) : null;
  const state = entry?.state ?? null;
  const percent = Math.round(
    Math.min(Math.max(progress?.fraction ?? 0, 0), 1) * 100,
  );

  const handleDownload = () => {
    cancelRequested.current = false;
    setError(null);
    void (async () => {
      try {
        // 下载 promise 在完成/失败/取消时结算；结算后重新拉取终态。
        await downloadModel();
      } catch (downloadError) {
        if (!cancelRequested.current) setError(getIpcErrorMessage(downloadError));
        // 无论成败都以最新状态刷新终态（如 sha256 校验失败后端已回滚 .part）。
        void refreshModelStatus().catch(() => {});
      }
    })();
  };

  const handleCancel = () => {
    cancelRequested.current = true;
    setError(null);
    setBusy(true);
    void (async () => {
      try {
        await cancelModelDownload();
      } catch (cancelError) {
        setError(getIpcErrorMessage(cancelError));
        void refreshModelStatus().catch(() => {});
      } finally {
        setBusy(false);
      }
    })();
  };

  /** 删除经二次确认（confirmingDelete）后才真正执行。 */
  const handleDelete = () => {
    setError(null);
    setBusy(true);
    void (async () => {
      try {
        await deleteModel();
        setConfirmingDelete(false);
      } catch (deleteError) {
        setError(getIpcErrorMessage(deleteError));
        void refreshModelStatus().catch(() => {});
      } finally {
        setBusy(false);
      }
    })();
  };

  const handleRefresh = () => {
    setError(null);
    setBusy(true);
    void refreshModelStatus()
      .catch((refreshError) => setError(getIpcErrorMessage(refreshError)))
      .finally(() => setBusy(false));
  };

  return (
    <fieldset className="rounded-lg border border-slate-100 p-3">
      <legend className="px-1 text-xs font-semibold text-slate-400">本地翻译模型</legend>
      <div className="space-y-2">
        <div className="flex items-center justify-between gap-2">
          <p
            className="flex items-center gap-1.5 text-xs text-slate-600"
            data-testid="model-state-label"
          >
            {downloading ? (
              <Loader2 size={14} className="animate-spin text-indigo-500" aria-hidden="true" />
            ) : state === "ready" ? (
              <CheckCircle2 size={14} className="text-emerald-500" aria-hidden="true" />
            ) : state === "invalid" ? (
              <AlertTriangle size={14} className="text-amber-500" aria-hidden="true" />
            ) : null}
            {downloading ? "下载中" : state === null ? "状态未知" : STATE_LABELS[state]}
          </p>
          {downloading && (
            <p className="text-xs font-medium text-indigo-600" data-testid="model-download-percent">
              {percent}%
            </p>
          )}
        </div>

        {downloading && (
          <div
            className="h-1.5 w-full overflow-hidden rounded-full bg-slate-100"
            role="progressbar"
            aria-label="模型下载进度"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={percent}
          >
            <div
              className="h-full rounded-full bg-indigo-500 transition-all duration-300"
              style={{ width: `${percent}%` }}
            />
          </div>
        )}

        {confirmingDelete && !downloading && (
          <p className="rounded-lg bg-red-50 px-3 py-2 text-xs text-red-700">
            确定要删除本地翻译模型吗？删除后需重新下载才能使用本地翻译。
          </p>
        )}

        {error && (
          <p className="rounded-lg bg-red-50 px-3 py-2 text-xs text-red-700" role="alert">
            {error}
          </p>
        )}

        <div className="flex flex-wrap gap-2">
          {downloading ? (
            <button type="button" onClick={handleCancel} disabled={busy} className="secondary-button">
              {busy ? "取消中…" : "取消下载"}
            </button>
          ) : state === "ready" && confirmingDelete ? (
            <>
              <button
                type="button"
                onClick={handleDelete}
                disabled={busy}
                className="secondary-button"
              >
                {busy ? "删除中…" : "确认删除"}
              </button>
              <button
                type="button"
                onClick={() => setConfirmingDelete(false)}
                disabled={busy}
                className="secondary-button"
              >
                取消
              </button>
            </>
          ) : state === "ready" ? (
            <button
              type="button"
              onClick={() => setConfirmingDelete(true)}
              className="secondary-button"
            >
              <Trash2 size={15} aria-hidden="true" />
              删除
            </button>
          ) : state === "missing" || state === "invalid" ? (
            <button type="button" onClick={handleDownload} className="primary-button">
              <Download size={15} aria-hidden="true" />
              {state === "invalid" ? "重新下载" : "下载"}
            </button>
          ) : (
            <button type="button" onClick={handleRefresh} disabled={busy} className="secondary-button">
              <RefreshCw size={15} aria-hidden="true" />
              {busy ? "刷新中…" : "刷新"}
            </button>
          )}
        </div>
      </div>
    </fieldset>
  );
}
