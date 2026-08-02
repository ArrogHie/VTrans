import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect } from "react";
import { listenToFrontendOcrResult, subscribeToBackendEvents } from "./services/events";
import { getIpcErrorMessage, showResultWindow } from "./services/tauri";
import { useAppStore } from "./stores/appStore";
import { MainWindow } from "./windows/MainWindow";
import { RegionSelector } from "./windows/RegionSelector";
import { ResultWindow } from "./windows/ResultWindow";

export function App() {
  const label = getWindowLabel();
  useBackendEvents();
  if (label === "selector") return <RegionSelector />;
  if (label === "result") return <ResultWindow />;
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
  const setTranslationResult = useAppStore((state) => state.setTranslationResult);
  const setModelProgress = useAppStore((state) => state.setModelProgress);
  const setSelectedRegion = useAppStore((state) => state.setSelectedRegion);
  const setError = useAppStore((state) => state.setError);

  useEffect(() => {
    let disposed = false;
    let cleanup: (() => void) | undefined;
    let unlistenFrontendResult: (() => void) | undefined;
    void subscribeToBackendEvents({
      capture_status_changed: ({ status }) => {
        if (status === "capturing") {
          setMode("live");
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
        setMode("single");
        setStatus("idle");
      },
      model_loading_progress: ({ progress }) => setModelProgress(progress),
      region_selected: ({ result }) => {
        setSelectedRegion(result);
      },
    }).then((unlisten) => {
      if (disposed) unlisten();
      else cleanup = unlisten;
    }).then(() => listenToFrontendOcrResult((result) => {
      setOcrResult(result);
      setStatus("completed");
      void showResultWindow();
    })).then((unlisten) => {
      if (disposed) unlisten();
      else unlistenFrontendResult = unlisten;
    }).catch((error) => {
      if (!disposed) console.warn(`[vtrans] event subscription failed: ${getIpcErrorMessage(error)}`);
    });
    return () => {
      disposed = true;
      cleanup?.();
      unlistenFrontendResult?.();
    };
  }, [setError, setMode, setModelProgress, setOcrResult, setSelectedRegion, setStatus, setTranslationResult]);
}

export default App;
