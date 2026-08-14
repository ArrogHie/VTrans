import { useAppStore } from "../stores/appStore";
import { startMultiBox, stopMultiBox } from "./multiBoxActions";
import { hideRegionOverlay } from "./regionOverlay";
import {
  captureOnce,
  getIpcErrorMessage,
  isRegionSelectionCancelled,
  publishFrontendLiveConfig,
  publishFrontendLivePaused,
  publishFrontendLiveStopped,
  publishFrontendOcrResult,
  showResultWindow,
  startLiveTranslation,
  startRegionSelection,
  stopLiveTranslation,
} from "./tauri";
import { isAnyBoxRunning, isSingleLiveRunning, type PipelineConfig } from "../types";

/** Outcome of a translate action, letting callers distinguish a cancelled selection. */
export interface TranslateActionResult {
  /** Whether the action completed successfully. */
  ok: boolean;
  /** Whether the region selection was cancelled by the user (Esc). */
  cancelled: boolean;
}

const failed = (cancelled = false): TranslateActionResult => ({ ok: false, cancelled });
const succeeded: TranslateActionResult = { ok: true, cancelled: false };

function reportError(error: unknown): void {
  useAppStore.getState().setStatus({ error: getIpcErrorMessage(error) });
}

function buildLiveConfig(region: PipelineConfig["region"]): PipelineConfig {
  const { config } = useAppStore.getState();
  return {
    region,
    capture_interval_ms: config.capture.interval_ms,
    difference_threshold: config.capture.difference_threshold,
  };
}

/**
 * Selects a region and runs one capture/OCR/translation pass.
 *
 * Shared by the main window (single mode) and the floating ball
 * ("框选翻译"). A running live session is paused first so the single pass
 * does not race with live captures; the overlay marker is hidden when the
 * pass finishes.
 */
export async function selectAndTranslateOnce(): Promise<TranslateActionResult> {
  const state = useAppStore.getState();
  void hideRegionOverlay();
  const liveWasRunning = state.mode === "live" && !state.livePaused && Boolean(state.liveConfig);
  if (liveWasRunning) {
    try {
      await stopLiveTranslation();
      useAppStore.getState().setLivePaused(true);
      void publishFrontendLivePaused();
    } catch (error) {
      reportError(error);
      return failed();
    }
  }
  useAppStore.getState().setStatus(liveWasRunning ? "idle" : "capturing");
  try {
    const region = await startRegionSelection();
    useAppStore.getState().setSelectedRegion(region);
    const result = await captureOnce(region);
    useAppStore.getState().setOcrResult(result);
    void publishFrontendOcrResult(result);
    useAppStore.getState().setStatus("completed");
    // 单次翻译完成后常驻选区方框必须隐藏。后端在单次捕获结束也会隐藏，
    // 这里显式执行一次，保证任何路径下都不残留。
    void hideRegionOverlay();
    void showResultWindow();
    return succeeded;
  } catch (error) {
    if (isRegionSelectionCancelled(error)) {
      // Esc 取消选区是正常操作；实时会话保持暂停，等待用户恢复。
      useAppStore.getState().setStatus("idle");
      return failed(true);
    }
    reportError(error);
    return failed();
  }
}

/**
 * Selects a region and starts (or restarts) a live session on it.
 *
 * Used by the main window live mode and the floating ball
 * ("实时翻译启停" without an existing region). A running live session is
 * paused while the selector is open; the new region is then applied and the
 * session is resumed, or started for the first time when no config exists.
 */
export async function selectRegionForLive(): Promise<TranslateActionResult> {
  const state = useAppStore.getState();
  void hideRegionOverlay();
  const liveWasRunning = state.mode === "live" && !state.livePaused && Boolean(state.liveConfig);
  if (liveWasRunning) {
    try {
      await stopLiveTranslation();
      useAppStore.getState().setLivePaused(true);
      void publishFrontendLivePaused();
    } catch (error) {
      reportError(error);
      return failed();
    }
  }
  useAppStore.getState().setStatus(liveWasRunning ? "idle" : "capturing");
  try {
    const region = await startRegionSelection();
    useAppStore.getState().setSelectedRegion(region);
    const previous = useAppStore.getState().liveConfig;
    const liveConfig = previous ? { ...previous, region } : buildLiveConfig(region);
    await startLiveTranslation(liveConfig);
    useAppStore.getState().setLiveConfig(liveConfig);
    useAppStore.getState().setLivePaused(false);
    useAppStore.getState().setMode("live");
    useAppStore.getState().setStatus("capturing");
    void publishFrontendLiveConfig(liveConfig);
    return succeeded;
  } catch (error) {
    if (isRegionSelectionCancelled(error)) {
      // Esc 取消选区是正常操作；实时会话保持暂停，等待用户恢复。
      useAppStore.getState().setStatus("idle");
      return failed(true);
    }
    reportError(error);
    return failed();
  }
}

/**
 * Starts the live session on the currently selected region.
 *
 * Returns immediately when a session is already running; errors are written
 * to the shared store so every window sees them.
 */
export async function startLive(): Promise<TranslateActionResult> {
  const state = useAppStore.getState();
  if (!state.selectedRegion) {
    useAppStore.getState().setStatus({ error: "请先选择翻译区域" });
    return failed();
  }
  if (state.liveConfig && !state.livePaused) return succeeded;
  try {
    const liveConfig = state.liveConfig ?? buildLiveConfig(state.selectedRegion);
    await startLiveTranslation(liveConfig);
    useAppStore.getState().setLiveConfig(liveConfig);
    useAppStore.getState().setLivePaused(false);
    useAppStore.getState().setMode("live");
    useAppStore.getState().setStatus("capturing");
    void publishFrontendLiveConfig(liveConfig);
    return succeeded;
  } catch (error) {
    reportError(error);
    return failed();
  }
}

/**
 * Pauses or resumes the live session depending on its current state.
 *
 * Shared by the main window, the result mini-bar and the floating ball.
 */
export async function toggleLivePause(): Promise<TranslateActionResult> {
  const state = useAppStore.getState();
  try {
    if (state.livePaused) {
      if (!state.liveConfig) return failed();
      await startLiveTranslation(state.liveConfig);
      useAppStore.getState().setLivePaused(false);
      useAppStore.getState().setStatus("capturing");
      void publishFrontendLiveConfig(state.liveConfig);
    } else {
      useAppStore.getState().setLivePaused(true);
      await stopLiveTranslation();
      useAppStore.getState().setStatus("idle");
      void publishFrontendLivePaused();
    }
    return succeeded;
  } catch (error) {
    // 无论暂停还是恢复失败，都回滚到操作前的暂停状态。
    useAppStore.getState().setLivePaused(state.livePaused);
    reportError(error);
    return failed();
  }
}

/**
 * Stops the live session and resets the UI to single mode.
 */
export async function stopLive(): Promise<TranslateActionResult> {
  try {
    const state = useAppStore.getState();
    void hideRegionOverlay();
    if (state.liveConfig && !state.livePaused) {
      await stopLiveTranslation();
    }
    void publishFrontendLiveStopped();
    useAppStore.getState().setLiveConfig(null);
    useAppStore.getState().setLivePaused(false);
    useAppStore.getState().setStatus("idle");
    useAppStore.getState().setMode("single");
    return succeeded;
  } catch (error) {
    reportError(error);
    return failed();
  }
}

/**
 * Toggles the live session from the floating ball.
 *
 * The ball shows and controls the same session as the main window:
 *
 * - Running multi-box session (any box `Running`) → stop the multi-box
 *   session. The floating ball and the main window share one multi-box
 *   session, so stopping from the ball affects the main window controls too.
 * - Running single-live session → stop the single live session
 *   (Alt+Shift+R/S remain the only hotkey controls for single-live).
 * - Nothing running with configured boxes → start the multi-box session.
 * - Nothing running without boxes → single-live path: resume on the selected
 *   region when available, otherwise select a region first. This mirrors the
 *   main window, where "开始实时" also has no session to start without boxes.
 */
export async function toggleLiveFromFloater(): Promise<TranslateActionResult> {
  const state = useAppStore.getState();
  if (isAnyBoxRunning(state.boxStatuses)) return stopMultiBox();
  if (isSingleLiveRunning(state.mode, state.liveConfig)) return stopLive();
  if (state.translationBoxes.length > 0) return startMultiBox();
  if (state.selectedRegion) return startLive();
  return selectRegionForLive();
}
