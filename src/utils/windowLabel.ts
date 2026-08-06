import { getCurrentWindow } from "@tauri-apps/api/window";

/** Minimal document surface needed to apply the window label. */
export interface WindowLabelDocument {
  documentElement: { dataset: { window?: string } };
}

/**
 * Resolves the current Tauri webview label.
 *
 * Falls back to the `window` query parameter (used by plain-browser
 * development) and finally to `main`, mirroring the routing in `App`.
 */
export function getWindowLabel(): string {
  try {
    return getCurrentWindow().label;
  } catch {
    return new URLSearchParams(window.location.search).get("window") ?? "main";
  }
}

/**
 * Applies the window label to `document.documentElement` synchronously.
 *
 * `main.tsx` must call this before the first React render: window-scoped CSS
 * (transparent backgrounds, scrollbar resets) keys off
 * `html[data-window="..."]`, and waiting for a React effect would flash the
 * default opaque background for one frame.
 */
export function applyWindowLabel(doc: WindowLabelDocument): string {
  const label = getWindowLabel();
  doc.documentElement.dataset.window = label;
  return label;
}
