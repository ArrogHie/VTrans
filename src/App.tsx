import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect } from "react";
import {
  listenToFrontendLiveConfig,
  listenToFrontendLivePaused,
  listenToFrontendLiveStopped,
  listenToFrontendOcrResult,
  subscribeToBackendEvents,
  type Unlisten,
} from "./services/events";
import { getIpcErrorMessage, showResultWindow } from "./services/tauri";
import { useAppStore } from "./stores/appStore";
import { MainWindow } from "./windows/MainWindow";
import { OverlayWindow } from "./windows/OverlayWindow";
import { RegionSelector } from "./windows/RegionSelector";
import { ResultWindow } from "./windows/ResultWindow";

export function App() {
  const label = getWindowLabel();
  useEffect(() => {
    // 让全局样式可以按窗口隔离（例如选区窗口需要透明背景）。
    document.documentElement.dataset.window = label;
  }, [label]);
  useBackendEvents();
  if (label === "selector") return <RegionSelector />;
  if (label === "result") return <ResultWindow />;
  if (label === "overlay") return <OverlayWindow />;
  return <MainWindow />;
}

function getWindowLabel(): string {
  try {
    return getCurrentWindow().label;
  } catch {
    return new URLSearchParams(window.location.search).get("window") ?? "main";
  }
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
  }, [setError, setLiveConfig, setLivePaused, setMode, setModelProgress, setOcrResult, setSelectedRegion, setStatus, setTranslationResult]);
}

export default App;
