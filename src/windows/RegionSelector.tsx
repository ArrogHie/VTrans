import { getCurrentWindow, currentMonitor } from "@tauri-apps/api/window";
import { useCallback, useEffect, useState } from "react";
import { Check, RotateCcw, X } from "lucide-react";
import { hideRegionOverlay } from "../services/regionOverlay";
import {
  cancelRegionSelection,
  getAppStatus,
  getIpcErrorMessage,
  toPhysicalRegion,
  updateLiveRegion,
} from "../services/tauri";
import { useAppStore } from "../stores/appStore";

type Phase = "selecting" | "confirmed";

export function RegionSelector() {
  const [start, setStart] = useState<{ x: number; y: number } | null>(null);
  const [end, setEnd] = useState<{ x: number; y: number } | null>(null);
  const [phase, setPhase] = useState<Phase>("selecting");
  const [monitorId, setMonitorId] = useState<string | null>(null);
  const setSelectedRegion = useAppStore((state) => state.setSelectedRegion);
  // 选区窗口与主窗口共享会话模式（经跨窗口事件同步）；后端按此模式
  // 决定常驻方框的显隐，单次模式确认不会显示方框。
  const mode = useAppStore((state) => state.mode);
  const [message, setMessage] = useState("拖动鼠标框选要翻译的区域，松开后确认");

  // 选区窗口是独立 WebView：store 的 mode 依赖跨窗口事件同步，打开时
  // 可能滞后。打开时主动拉取一次后端状态，让确认按真实会话模式提交
  // （实时重选区显示方框、单次确认不显示）。拉取失败时保持现状，
  // 默认 single 是安全降级（不显示方框）。
  useEffect(() => {
    let active = true;
    void getAppStatus()
      .then((snapshot) => {
        if (active) useAppStore.getState().applyStatus(snapshot);
      })
      .catch((error) => {
        // 同步失败非致命：保持 store 现状，默认 single 是安全降级
        // （不显示方框）；只记录脱敏信息便于排查。
        console.warn(`[vtrans] selector mode sync failed: ${getIpcErrorMessage(error)}`);
      });
    return () => {
      active = false;
    };
  }, []);

  const resetSelection = useCallback(() => {
    void hideRegionOverlay();
    setStart(null);
    setEnd(null);
    setPhase("selecting");
    setMessage("拖动鼠标框选要翻译的区域，松开后确认");
  }, []);

  const cancelSelection = useCallback(() => {
    void (async () => {
      await hideRegionOverlay();
      try {
        await cancelRegionSelection();
      } catch {
        // 后端可能已经清理了待处理选区，忽略即可。
      } finally {
        await getCurrentWindow().hide();
      }
    })();
  }, []);

  const confirmSelection = useCallback(async () => {
    if (!start || !end) return;
    if (!monitorId) {
      setMessage("无法识别当前显示器，请关闭后重试");
      return;
    }
    const region = toPhysicalRegion(monitorId, start, end);
    if (!region) {
      setMessage("选区太小，请重新拖动");
      return;
    }
    try {
      await updateLiveRegion(region, mode);
      setSelectedRegion(region);
      await getCurrentWindow().hide();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "选区提交失败");
    }
  }, [end, mode, monitorId, setSelectedRegion, start]);

  useEffect(() => {
    let active = true;
    let unlistenClose: (() => void) | undefined;
    const currentWindow = getCurrentWindow();
    void currentWindow.onCloseRequested(async () => {
      await cancelRegionSelection();
    }).then((unlisten) => {
      if (active) unlistenClose = unlisten;
      else unlisten();
    });
    void currentMonitor().then((monitor) => {
      if (active && monitor?.name) {
        setMonitorId(monitor.name);
      } else if (active) {
        setMessage("无法识别当前显示器，请关闭后重试");
      }
    });
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        cancelSelection();
      }
      else if (event.key === "Enter") void confirmSelection();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      active = false;
      unlistenClose?.();
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [cancelSelection, confirmSelection]);

  const onPointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (phase !== "selecting") return;
    event.currentTarget.setPointerCapture(event.pointerId);
    setStart({ x: event.clientX, y: event.clientY });
    setEnd({ x: event.clientX, y: event.clientY });
    setMessage("拖动鼠标框选要翻译的区域，松开后确认");
  };
  const onPointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    if (phase === "selecting" && start) setEnd({ x: event.clientX, y: event.clientY });
  };
  const onPointerUp = (event: React.PointerEvent<HTMLDivElement>) => {
    if (phase !== "selecting" || !start) return;
    setEnd({ x: event.clientX, y: event.clientY });
    const box = getSelectionBox(start, { x: event.clientX, y: event.clientY });
    if (!box || box.width < 4 || box.height < 4) {
      setMessage("选区太小，请重新拖动");
      return;
    }
    setPhase("confirmed");
    setMessage("确认选区，或重新选择");
  };

  const box = getSelectionBox(start, end);
  const confirmed = phase === "confirmed" && box !== null;
  return (
    <main className="fixed inset-0 cursor-crosshair select-none bg-slate-950/25" onPointerDown={onPointerDown} onPointerMove={onPointerMove} onPointerUp={onPointerUp} aria-label="屏幕区域选择器">
      <div className="pointer-events-none absolute left-1/2 top-8 -translate-x-1/2 rounded-full bg-slate-950/75 px-4 py-2 text-xs text-white shadow-lg">{message} · Enter 确认 · Esc 取消</div>
      {box && <div className="pointer-events-none absolute border-2 border-indigo-400 bg-indigo-400/10 shadow-[0_0_0_9999px_rgba(15,23,42,0.2)]" style={{ left: box.left, top: box.top, width: box.width, height: box.height }}><span className="absolute -top-7 left-0 rounded bg-indigo-500 px-2 py-1 text-xs font-medium text-white">{Math.round(box.width)} × {Math.round(box.height)}</span></div>}
      {confirmed && (
        <div className="absolute bottom-10 left-1/2 flex -translate-x-1/2 items-center gap-2 rounded-xl bg-slate-950/80 px-3 py-2 shadow-lg">
          <button type="button" onClick={() => void confirmSelection()} className="inline-flex items-center gap-1.5 rounded-lg bg-indigo-500 px-4 py-2 text-sm font-semibold text-white hover:bg-indigo-400">
            <Check size={15} aria-hidden="true" />确认
          </button>
          <button type="button" onClick={resetSelection} className="inline-flex items-center gap-1.5 rounded-lg bg-slate-700 px-3 py-2 text-sm font-medium text-slate-100 hover:bg-slate-600">
            <RotateCcw size={14} aria-hidden="true" />重新选择
          </button>
          <button type="button" onClick={cancelSelection} className="inline-flex items-center gap-1.5 rounded-lg bg-slate-700 px-3 py-2 text-sm font-medium text-slate-100 hover:bg-slate-600">
            <X size={14} aria-hidden="true" />取消
          </button>
        </div>
      )}
    </main>
  );
}

function getSelectionBox(start: { x: number; y: number } | null, end: { x: number; y: number } | null): { left: number; top: number; width: number; height: number } | null {
  if (!start || !end) return null;
  return { left: Math.min(start.x, end.x), top: Math.min(start.y, end.y), width: Math.abs(end.x - start.x), height: Math.abs(end.y - start.y) };
}

export function selectionBoxForTest(start: { x: number; y: number } | null, end: { x: number; y: number } | null) {
  return getSelectionBox(start, end);
}
