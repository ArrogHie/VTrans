import {
  cancelTranslationModelDownload,
  deleteTranslationModel,
  downloadTranslationModel,
  getModelStatus,
  loadLocalModels,
  retryModelSetup as retryModelSetupCommand,
} from "./tauri";
import { useAppStore } from "../stores/appStore";
import { findTranslationModelEntry, verifyReportMessage } from "../types";
import type { ModelDownloadProgress, ModelStatusReport } from "../types";

/**
 * Model download / setup actions shared by the settings card and the main
 * window. All terminal state lives in the Zustand store, so closing the
 * settings panel never interrupts a backend download: the flow functions keep
 * running and reconcile the store when the backend promise settles.
 */

/**
 * Fetches the model status snapshot and mirrors it into the app store.
 *
 * When the translation model entry is `ready` the download must have settled,
 * so the in-flight marker and the last progress payload are cleared — this
 * prevents a late progress event from pulling the UI back into a stale
 * "downloading" state.
 */
export async function refreshModelStatus(): Promise<ModelStatusReport> {
  const report = await getModelStatus();
  const state = useAppStore.getState();
  state.setModelStatus(report);
  if (findTranslationModelEntry(report)?.state === "ready") {
    state.setTranslationModelDownloading(false);
    state.setModelDownloadProgress(null);
  }
  return report;
}

/**
 * Mirrors one `model_download_progress` payload into the app store.
 *
 * Progress events are only emitted while a download runs, so they mark the
 * download as in flight — unless the store already knows the model is ready
 * (a late event racing the terminal refresh), in which case only the progress
 * value is recorded.
 */
export function applyModelDownloadProgress(progress: ModelDownloadProgress): void {
  const state = useAppStore.getState();
  state.setModelDownloadProgress(progress);
  const entry = state.modelStatus ? findTranslationModelEntry(state.modelStatus) : null;
  if (entry?.state !== "ready") state.setTranslationModelDownloading(true);
}

/**
 * Starts the translation model download and returns the terminal status
 * snapshot after the backend promise settles.
 *
 * Progress arrives via `model_download_progress` events (mirrored by
 * `applyModelDownloadProgress`); once the download completes, fails, or is
 * cancelled the backend promise settles and the status is refreshed.
 */
export async function downloadModel(): Promise<ModelStatusReport> {
  const state = useAppStore.getState();
  state.setTranslationModelDownloading(true);
  try {
    await downloadTranslationModel();
    return await refreshModelStatus();
  } finally {
    useAppStore.getState().setTranslationModelDownloading(false);
  }
}

/**
 * Cancels the running download and refreshes the terminal status.
 *
 * The in-flight marker is cleared even when the cancel command fails (for
 * example nothing is downloading), so a stale marker can always be recovered.
 */
export async function cancelModelDownload(): Promise<ModelStatusReport> {
  try {
    await cancelTranslationModelDownload();
  } finally {
    useAppStore.getState().setTranslationModelDownloading(false);
  }
  return refreshModelStatus();
}

/**
 * Deletes the local translation model and refreshes the terminal status.
 *
 * The backend cancels a running download first; the in-flight marker is
 * cleared defensively on the frontend as well.
 */
export async function deleteModel(): Promise<ModelStatusReport> {
  try {
    await deleteTranslationModel();
  } finally {
    useAppStore.getState().setTranslationModelDownloading(false);
  }
  return refreshModelStatus();
}

/**
 * Re-runs the first-run model setup (`retry_model_setup`) and mirrors the
 * fresh report into the store. The R6 banner derives its visibility from the
 * store, so it disappears automatically once the report is healthy.
 */
export async function retryModelSetup(): Promise<ModelStatusReport> {
  const report = await retryModelSetupCommand();
  useAppStore.getState().setModelStatus(report);
  return report;
}

/**
 * Runs `load_local_models` and maps the report to the user-facing verdict.
 *
 * Optional entries missing on disk are reported as `skipped` (not `failed`),
 * so a report with no failures can still mean the translation model is not
 * installed; the verdict text must reflect that instead of claiming success.
 */
export async function verifyLocalModels(): Promise<string> {
  return verifyReportMessage(await loadLocalModels());
}
