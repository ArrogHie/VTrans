interface ModelSetupBannerProps {
  retrying: boolean;
  onRetry: () => void;
}

/**
 * Persistent R6 banner: model files are not ready for use.
 *
 * Shown by the main window while `get_model_status` reports `ocr_ready ===
 * false` or a non-optional entry is invalid. It never blocks the rest of the
 * main window; the retry button re-runs `retry_model_setup`, and the banner
 * disappears automatically once the refreshed status is healthy.
 */
export function ModelSetupBanner({ retrying, onRetry }: ModelSetupBannerProps) {
  return (
    <div
      className="mb-3 flex items-start justify-between gap-2 rounded-lg bg-red-50 px-3 py-2 text-xs text-red-700"
      role="alert"
      data-testid="model-setup-banner"
    >
      <span className="min-w-0 break-words">OCR 模型未就位，翻译功能不可用</span>
      <button
        type="button"
        onClick={onRetry}
        disabled={retrying}
        className="shrink-0 rounded-md bg-red-600 px-2 py-1 font-medium text-white transition hover:bg-red-700 disabled:cursor-not-allowed disabled:opacity-60"
      >
        {retrying ? "重试中…" : "重试"}
      </button>
    </div>
  );
}
