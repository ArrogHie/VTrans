interface ErrorBannerProps {
  message: string;
  onDismiss: () => void;
}

/** Inline error banner shown by the result window. */
export function ErrorBanner({ message, onDismiss }: ErrorBannerProps) {
  return (
    <div
      className="mb-3 flex items-start justify-between gap-2 rounded-lg bg-red-50 px-3 py-2 text-xs text-red-700"
      role="alert"
    >
      <span className="min-w-0 break-words">{message}</span>
      <button
        type="button"
        onClick={onDismiss}
        className="shrink-0 rounded p-0.5 hover:bg-red-100"
        title="关闭"
      >
        ×
      </button>
    </div>
  );
}
