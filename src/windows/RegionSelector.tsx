import { getCurrentWindow, currentMonitor } from "@tauri-apps/api/window";
import { useCallback, useEffect, useState } from "react";
import { cancelRegionSelection, toPhysicalRegion, updateLiveRegion } from "../services/tauri";
import { useAppStore } from "../stores/appStore";

export function RegionSelector() {
  const [start, setStart] = useState<{ x: number; y: number } | null>(null);
  const [end, setEnd] = useState<{ x: number; y: number } | null>(null);
  const [monitorId, setMonitorId] = useState<string | null>(null);
  const setSelectedRegion = useAppStore((state) => state.setSelectedRegion);
  const [message, setMessage] = useState("拖动鼠标框选要翻译的区域");

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
      await updateLiveRegion(region);
      setSelectedRegion(region);
      await getCurrentWindow().hide();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "选区提交失败");
    }
  }, [end, monitorId, setSelectedRegion, start]);

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
        void cancelRegionSelection().finally(() => getCurrentWindow().hide());
      }
      else if (event.key === "Enter") void confirmSelection();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      active = false;
      unlistenClose?.();
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [confirmSelection]);

  const onPointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    event.currentTarget.setPointerCapture(event.pointerId);
    setStart({ x: event.clientX, y: event.clientY });
    setEnd({ x: event.clientX, y: event.clientY });
  };
  const onPointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    if (start) setEnd({ x: event.clientX, y: event.clientY });
  };
  const onPointerUp = (event: React.PointerEvent<HTMLDivElement>) => {
    if (start) setEnd({ x: event.clientX, y: event.clientY });
  };

  const box = getSelectionBox(start, end);
  return (
    <main className="fixed inset-0 cursor-crosshair select-none bg-slate-950/25" onPointerDown={onPointerDown} onPointerMove={onPointerMove} onPointerUp={onPointerUp} aria-label="屏幕区域选择器">
      <div className="pointer-events-none absolute left-1/2 top-8 -translate-x-1/2 rounded-full bg-slate-950/75 px-4 py-2 text-xs text-white shadow-lg">{message} · Enter 确认 · Esc 取消</div>
      {box && <div className="pointer-events-none absolute border-2 border-indigo-400 bg-indigo-400/10 shadow-[0_0_0_9999px_rgba(15,23,42,0.2)]" style={{ left: box.left, top: box.top, width: box.width, height: box.height }}><span className="absolute -top-7 left-0 rounded bg-indigo-500 px-2 py-1 text-xs font-medium text-white">{Math.round(box.width)} × {Math.round(box.height)}</span></div>}
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
