import { availableMonitors, getCurrentWindow, type Window } from "@tauri-apps/api/window";
import { LogicalSize, PhysicalPosition } from "@tauri-apps/api/dpi";
import { Languages, MousePointer2, Pause, Play, Radio, Square } from "lucide-react";
import { useEffect, useState } from "react";
import { listenToFrontendFloaterEnabled, type Unlisten } from "../services/events";
import { getAppConfig, getAppStatus, showMainWindow } from "../services/tauri";
import {
  selectAndTranslateOnce,
  toggleLiveFromFloater,
  toggleLivePause,
  type TranslateActionResult,
} from "../services/translateActions";
import { useAppStore } from "../stores/appStore";
import {
  clampFloaterPosition,
  FLOATER_BALL_SIZE,
  loadFloaterPosition,
  saveFloaterPosition,
} from "../utils/floaterPosition";
import { applyFloaterVisibility } from "../utils/floaterVisibility";

/** Expanded size of the floating ball window while the menu is open. */
const MENU_SIZE = { width: 220, height: 264 };

let cachedWindow: Window | null = null;

/** Lazily resolves the floating ball window; `null` outside a Tauri runtime. */
function getFloaterWindow(): Window | null {
  if (cachedWindow === null) {
    try {
      cachedWindow = getCurrentWindow();
    } catch {
      cachedWindow = null;
    }
  }
  return cachedWindow;
}

/**
 * Floating ball window (label `floater`).
 *
 * A small draggable ball that expands into a compact action menu. Visibility
 * follows `floating_ball.enabled` from the persisted configuration and the
 * frontend-only `frontend_floater_enabled` event; position is remembered in
 * localStorage and clamped to the available monitors on startup.
 */
export function FloatingBall() {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const mode = useAppStore((state) => state.mode);
  const livePaused = useAppStore((state) => state.livePaused);
  const liveConfig = useAppStore((state) => state.liveConfig);
  const liveRunning = mode === "live" && Boolean(liveConfig);

  useEffect(() => {
    let disposed = false;
    let unlisten: Unlisten | undefined;
    const applyVisibility = (enabled: boolean) => {
      const tauriWindow = getFloaterWindow();
      if (!tauriWindow) return;
      applyFloaterVisibility(tauriWindow, enabled);
    };
    void Promise.all([
      // 启动水合：仅当配置开启时显示悬浮球。
      getAppConfig()
        .then((config) => {
          if (!disposed && config.floating_ball.enabled) applyVisibility(true);
        })
        .catch(() => undefined),
      // 主窗口设置面板切换开关时即时显隐。
      listenToFrontendFloaterEnabled(({ enabled }) => {
        if (!disposed) applyVisibility(enabled);
      }),
    ]).then(([, cleanup]) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlistenMoved: (() => void) | undefined;
    void (async () => {
      const tauriWindow = getFloaterWindow();
      if (!tauriWindow) return;
      // 恢复上次位置并夹到可见显示器内。
      const monitors = await availableMonitors().catch(() => []);
      const saved = loadFloaterPosition(window.localStorage);
      if (saved && monitors.length > 0) {
        const clamped = clampFloaterPosition(saved, monitors);
        await tauriWindow
          .setPosition(new PhysicalPosition(clamped.x, clamped.y))
          .catch(() => undefined);
      }
      // 拖动结束后记住新位置。
      unlistenMoved = await tauriWindow.onMoved(() => {
        void tauriWindow
          .innerPosition()
          .then((position) =>
            saveFloaterPosition(window.localStorage, { x: position.x, y: position.y }),
          )
          .catch(() => undefined);
      });
    })();
    return () => {
      disposed = true;
      unlistenMoved?.();
    };
  }, []);

  const collapseMenu = async () => {
    setOpen(false);
    const tauriWindow = getFloaterWindow();
    if (tauriWindow) {
      await tauriWindow
        .setSize(new LogicalSize(FLOATER_BALL_SIZE, FLOATER_BALL_SIZE))
        .catch(() => undefined);
      // 拖动可能发生在菜单打开期间，收起时保存一次当前位置。
      void tauriWindow
        .innerPosition()
        .then((position) =>
          saveFloaterPosition(window.localStorage, { x: position.x, y: position.y }),
        )
        .catch(() => undefined);
    }
  };

  const expandMenu = async () => {
    setOpen(true);
    const tauriWindow = getFloaterWindow();
    if (tauriWindow) {
      await tauriWindow
        .setSize(new LogicalSize(MENU_SIZE.width, MENU_SIZE.height))
        .catch(() => undefined);
    }
    // 打开时主动同步一次会话状态，保证菜单按钮反映真实状态。
    void getAppStatus()
      .then((snapshot) => useAppStore.getState().applyStatus(snapshot))
      .catch(() => undefined);
  };

  const run = async (action: () => Promise<TranslateActionResult>) => {
    if (busy) return;
    setBusy(true);
    await action();
    setBusy(false);
    await collapseMenu();
  };

  return (
    <main className="fixed inset-0 select-none" aria-label="悬浮球">
      <button
        type="button"
        data-tauri-drag-region
        onClick={() => void (open ? collapseMenu() : expandMenu())}
        className="absolute left-0 top-0 flex h-12 w-12 items-center justify-center rounded-full bg-indigo-600 text-white shadow-lg ring-2 ring-white/70 transition hover:bg-indigo-500"
        title="VTrans 悬浮球"
        aria-expanded={open}
        data-testid="floating-ball"
      >
        <Languages size={22} aria-hidden="true" />
      </button>
      {open && (
        <nav className="absolute left-0 top-12 w-[220px] space-y-1 rounded-xl border border-slate-200 bg-white p-2 shadow-xl" data-testid="floating-ball-menu">
          <button type="button" onClick={() => void run(selectAndTranslateOnce)} disabled={busy} className="floater-menu-item">
            <MousePointer2 size={15} aria-hidden="true" />框选翻译
          </button>
          <button type="button" onClick={() => void run(toggleLiveFromFloater)} disabled={busy} className="floater-menu-item">
            {liveRunning ? <Square size={15} aria-hidden="true" /> : <Radio size={15} aria-hidden="true" />}
            {liveRunning ? "停止实时翻译" : "实时翻译"}
          </button>
          <button
            type="button"
            onClick={() => void run(toggleLivePause)}
            disabled={busy || !liveRunning}
            className="floater-menu-item"
          >
            {livePaused ? <Play size={15} aria-hidden="true" /> : <Pause size={15} aria-hidden="true" />}
            {livePaused ? "继续" : "暂停·继续"}
          </button>
          <button type="button" onClick={() => void showMainWindow().then(() => collapseMenu())} className="floater-menu-item">
            <MousePointer2 size={15} aria-hidden="true" />打开主窗口
          </button>
        </nav>
      )}
    </main>
  );
}
