import { useEffect } from "react";
import {
  listenToFrontendLiveConfig,
  listenToFrontendLivePaused,
  listenToFrontendLiveStopped,
  listenToFrontendOcrResult,
  onMultiBoxBoxAdded,
  onMultiBoxBoxRemoved,
  onMultiBoxBoxUpdated,
  onMultiBoxResult,
  onMultiBoxStatus,
  onSingleTranslationResult,
  subscribeToBackendEvents,
  type Unlisten,
} from "./services/events";
import { getIpcErrorMessage, showResultWindow } from "./services/tauri";
import { hideRegionOverlay } from "./services/regionOverlay";
import { useAppStore } from "./stores/appStore";
import { getWindowLabel } from "./utils/windowLabel";
import { FloatingBall } from "./windows/FloatingBall";
import { MainWindow } from "./windows/MainWindow";
import { OverlayWindow } from "./windows/OverlayWindow";
import { RegionSelector } from "./windows/RegionSelector";
import { ResultWindow } from "./windows/ResultWindow";

export function App() {
  const label = getWindowLabel();
  useBackendEvents();
  if (label === "selector") return <RegionSelector />;
  if (label === "result") return <ResultWindow />;
  if (label === "overlay") return <OverlayWindow />;
  if (label === "floater") return <FloatingBall />;
  return <MainWindow />;
}

function useBackendEvents() {
  const setStatus = useAppStore((state) => state.setStatus);
  const setMode = useAppStore((state) => state.setMode);
  const setOcrResult = useAppStore((state) => state.setOcrResult);
  const setLiveConfig = useAppStore((state) => state.setLiveConfig);
  const setLivePaused = useAppStore((state) => state.setLivePaused);
  const setTranslationResult = useAppStore((state) => state.setTranslationResult);
  const setModelProgress = useAppStore((state) => state.setModelProgress);
  const setSelectedRegion = useAppStore((state) => state.setSelectedRegion);
  const setError = useAppStore((state) => state.setError);
  const upsertBox = useAppStore((state) => state.upsertBox);
  const removeBox = useAppStore((state) => state.removeBox);
  const updateBoxRegion = useAppStore((state) => state.updateBoxRegion);
  const setBoxStatus = useAppStore((state) => state.setBoxStatus);
  const setMultiBoxResult = useAppStore((state) => state.setMultiBoxResult);
  const setSingleResult = useAppStore((state) => state.setSingleResult);

  useEffect(() => {
    let disposed = false;
    let cleanup: (() => void) | undefined;

    const register = async () => {
      const unlisteners = await Promise.all<Unlisten>([
        subscribeToBackendEvents({
          capture_status_changed: ({ status }) => {
            if (status === "capturing") {
              setStatus("capturing");
            }
          },
          ocr_started: () => setStatus("ocr_in_progress"),
          ocr_completed: ({ result }) => {
            setOcrResult(result);
            setStatus("completed");
            void showResultWindow();
          },
          translation_started: () => setStatus("translating"),
          translation_completed: ({ result }) => {
            setTranslationResult(result);
            setStatus("completed");
            void showResultWindow();
          },
          pipeline_error: ({ message }) => setError(message),
          live_session_stopped: () => {
            const wasPaused = useAppStore.getState().livePaused;
            setStatus("idle");
            if (!wasPaused) {
              // 会话真正结束（停止/异常终止）时移除常驻选区方框；
              // 暂停期间保留方框，恢复后无需重新定位。
              void hideRegionOverlay();
              setLiveConfig(null);
              setLivePaused(false);
              setMode("single");
            } else {
              setMode("live");
            }
          },
          model_loading_progress: ({ progress }) => setModelProgress(progress),
          region_selected: (region) => setSelectedRegion(region),
        }),
        listenToFrontendOcrResult((result) => {
          setOcrResult(result);
          setStatus("completed");
          void showResultWindow();
        }),
        listenToFrontendLiveConfig((config) => {
          setLiveConfig(config);
          setLivePaused(false);
          setSelectedRegion(config.region);
          setMode("live");
        }),
        listenToFrontendLivePaused(() => {
          setLivePaused(true);
          setMode("live");
          setStatus("idle");
        }),
        listenToFrontendLiveStopped(() => {
          setLiveConfig(null);
          setLivePaused(false);
          setMode("single");
          setStatus("idle");
        }),
        onMultiBoxBoxAdded((payload) => {
          upsertBox({ box_id: payload.box_id, color: payload.color, region: payload.region });
        }),
        onMultiBoxBoxRemoved((payload) => removeBox(payload.box_id)),
        onMultiBoxBoxUpdated((payload) => updateBoxRegion(payload.box_id, payload.region)),
        onMultiBoxStatus((payload) => setBoxStatus(payload.box_id, payload.status)),
        onMultiBoxResult((result) => setMultiBoxResult(result)),
        onSingleTranslationResult((payload) => setSingleResult(payload)),
      ]);
      if (disposed) {
        for (const unlisten of unlisteners) unlisten();
      } else {
        cleanup = () => unlisteners.forEach((unlisten) => unlisten());
      }
    };

    void register().catch((error) => {
      if (!disposed) console.warn(`[vtrans] event subscription failed: ${getIpcErrorMessage(error)}`);
    });
    return () => {
      disposed = true;
      cleanup?.();
    };
  }, [removeBox, setBoxStatus, setError, setLiveConfig, setLivePaused, setMode, setModelProgress, setMultiBoxResult, setOcrResult, setSelectedRegion, setSingleResult, setStatus, setTranslationResult, updateBoxRegion, upsertBox]);
}

export default App;
