import { useAppStore } from "../stores/appStore";
import { showMultiBoxOverlay } from "./regionOverlay";
import {
  addTranslationBox,
  getIpcErrorMessage,
  isRegionSelectionCancelled,
  listTranslationBoxes,
  openResultWindow,
  publishFrontendMultiBoxStarted,
  publishFrontendMultiBoxStopped,
  removeTranslationBox,
  startMultiRealtime,
  startRegionSelection,
  stopBox,
  stopMultiRealtime,
  updateTranslationBox,
} from "./tauri";

/** Outcome of a multi-box action, distinguishing a cancelled selection. */
export interface MultiBoxActionResult {
  /** Whether the action completed successfully. */
  ok: boolean;
  /** Whether the region selection was cancelled by the user (Esc). */
  cancelled: boolean;
}

const succeeded: MultiBoxActionResult = { ok: true, cancelled: false };
const failed = (cancelled = false): MultiBoxActionResult => ({ ok: false, cancelled });

function reportError(error: unknown): void {
  useAppStore.getState().setStatus({ error: getIpcErrorMessage(error) });
}

/**
 * Selects a region and adds a new translation box to the multi-box session.
 *
 * The backend assigns the next id/color and also emits `multibox://box-added`;
 * the returned info is upserted into the store so the list updates even if
 * the event was missed (the store upsert is idempotent).
 */
export async function addBox(): Promise<MultiBoxActionResult> {
  try {
    const region = await startRegionSelection();
    const info = await addTranslationBox(region);
    useAppStore.getState().upsertBox(info);
    return succeeded;
  } catch (error) {
    if (isRegionSelectionCancelled(error)) return failed(true);
    reportError(error);
    return failed();
  }
}

/**
 * Re-selects a region and updates an existing translation box's capture area.
 */
export async function editBox(boxId: number): Promise<MultiBoxActionResult> {
  try {
    const region = await startRegionSelection();
    await updateTranslationBox(boxId, region);
    useAppStore.getState().updateBoxRegion(boxId, region);
    return succeeded;
  } catch (error) {
    if (isRegionSelectionCancelled(error)) return failed(true);
    reportError(error);
    return failed();
  }
}

/** Removes a translation box from the pipeline and configuration. */
export async function removeBox(boxId: number): Promise<MultiBoxActionResult> {
  try {
    await removeTranslationBox(boxId);
    useAppStore.getState().removeBox(boxId);
    return succeeded;
  } catch (error) {
    reportError(error);
    return failed();
  }
}

/**
 * Starts real-time translation for every configured box.
 *
 * After the backend confirms the start, every session box is marked
 * `Running` in the local store and the `frontend_multibox_started` event is
 * published so the other webviews (floating ball, result window) derive the
 * same running state. Failures only report the error — no fake running state
 * is written or broadcast.
 */
export async function startMultiBox(): Promise<MultiBoxActionResult> {
  try {
    // 后端 start_multi_realtime 只 show overlay 窗口、不定位（BUGFIX-2 根因）。
    // 先按第一个框所在显示器定位/缩放 overlay（只定位不 show，失败仅告警），
    // 后端随后 show 时各框才能按物理坐标/dpr 对齐绘制。
    await showMultiBoxOverlay(useAppStore.getState().translationBoxes);
    await startMultiRealtime();
    const boxIds = useAppStore.getState().translationBoxes.map((box) => box.box_id);
    useAppStore.getState().setBoxesStatus(boxIds, "Running");
    void publishFrontendMultiBoxStarted(boxIds);
    return succeeded;
  } catch (error) {
    reportError(error);
    return failed();
  }
}

/**
 * Stops all multi-box translation tasks.
 *
 * After the backend confirms the stop, every locally known session box
 * (configured boxes plus any box with a recorded status, in case the box list
 * has not hydrated yet) is marked `Stopped` and the
 * `frontend_multibox_stopped` event is published so every webview converges.
 * Failures only report the error — no fake stopped state is broadcast.
 */
export async function stopMultiBox(): Promise<MultiBoxActionResult> {
  try {
    await stopMultiRealtime();
    const state = useAppStore.getState();
    const boxIds = Array.from(
      new Set([
        ...state.translationBoxes.map((box) => box.box_id),
        ...Object.keys(state.boxStatuses).map((boxId) => Number(boxId)),
      ]),
    );
    useAppStore.getState().setBoxesStatus(boxIds, "Stopped");
    void publishFrontendMultiBoxStopped(boxIds);
    return succeeded;
  } catch (error) {
    reportError(error);
    return failed();
  }
}

/** Stops a single translation box, leaving it registered. */
export async function stopSingleBox(boxId: number): Promise<MultiBoxActionResult> {
  try {
    await stopBox(boxId);
    return succeeded;
  } catch (error) {
    reportError(error);
    return failed();
  }
}

/** Opens the translation popup, or focuses it when already visible. */
export async function openResultPopup(): Promise<MultiBoxActionResult> {
  try {
    await openResultWindow();
    return succeeded;
  } catch (error) {
    reportError(error);
    return failed();
  }
}

/**
 * Hydrates the configured box list from the backend.
 *
 * The list is read from persisted config so it survives restarts even when
 * the multi-box pipeline has not been started. Failures are logged, not
 * propagated: an empty list is a safe fallback.
 */
export async function hydrateBoxes(): Promise<void> {
  try {
    const boxes = await listTranslationBoxes();
    useAppStore.getState().setTranslationBoxes(boxes);
  } catch (error) {
    console.warn(`[vtrans] hydrate translation boxes failed: ${getIpcErrorMessage(error)}`);
  }
}
