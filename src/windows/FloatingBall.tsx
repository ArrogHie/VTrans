import { availableMonitors, getCurrentWindow, type Window } from "@tauri-apps/api/window";
import { LogicalSize, PhysicalPosition } from "@tauri-apps/api/dpi";
import { Languages, MousePointer2, Pause, Play, Radio, Square } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { listenToFrontendFloaterEnabled, type Unlisten } from "../services/events";
import {
  applyFloaterAppearance,
  clampFloaterOpacity,
  clampFloaterSizePx,
  persistFloaterAppearance,
} from "../services/floaterAppearance";
import {
  getAppConfig,
  getAppStatus,
  getIpcErrorMessage,
  showMainWindow,
} from "../services/tauri";
import {
  selectAndTranslateOnce,
  toggleLiveFromFloater,
  toggleLivePause,
  type TranslateActionResult,
} from "../services/translateActions";
import { useAppStore } from "../stores/appStore";
import {
  FLOATER_OPACITY_MAX,
  FLOATER_OPACITY_MIN,
  FLOATER_SIZE_MAX,
  FLOATER_SIZE_MIN,
} from "../types";
import { createFloaterDragHandlers } from "../utils/floaterDrag";
import { clampFloaterPosition, loadFloaterPosition, saveFloaterPosition } from "../utils/floaterPosition";
import { applyFloaterVisibility } from "../utils/floaterVisibility";

/** Width of the expanded floating ball window while the menu is open. */
const MENU_WIDTH = 220;
/** Height of the expanded menu panel below the ball (excluding the ball). */
const MENU_HEIGHT = 300;
/** Debounce delay for persisting floating ball appearance changes (ms). */
const APPEARANCE_PERSIST_MS = 350;

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

/** Props for the compact appearance controls inside the ball menu. */
export interface FloatingBallAppearanceControlsProps {
  opacity: number;
  sizePx: number;
  onOpacityChange: (value: number) => void;
  onSizeChange: (value: number) => void;
}

/**
 * Compact appearance controls (transparency and diameter) in the ball menu.
 *
 * The sliders feed clamped values back to the parent, which applies them
 * immediately through CSS custom properties and persists them through
 * `update_floating_ball_appearance`.
 */
export function FloatingBallAppearanceControls({
  opacity,
  sizePx,
  onOpacityChange,
  onSizeChange,
}: FloatingBallAppearanceControlsProps) {
  return (
    <div
      className="space-y-2 border-t border-slate-100 px-2.5 py-2"
      data-testid="floater-appearance"
    >
      <label className="flex flex-col gap-1 text-[11px] text-slate-500">
        透明度
        <input
          type="range"
          min={FLOATER_OPACITY_MIN}
          max={FLOATER_OPACITY_MAX}
          step={0.05}
          value={opacity}
          onChange={(event) => onOpacityChange(Number(event.target.value))}
          className="accent-indigo-600"
          data-testid="floater-opacity-slider"
        />
      </label>
      <label className="flex flex-col gap-1 text-[11px] text-slate-500">
        大小（{sizePx}px）
        <input
          type="range"
          min={FLOATER_SIZE_MIN}
          max={FLOATER_SIZE_MAX}
          step={1}
          value={sizePx}
          onChange={(event) => onSizeChange(Number(event.target.value))}
          className="accent-indigo-600"
          data-testid="floater-size-slider"
        />
      </label>
    </div>
  );
}

/**
 * Floating ball window (label `floater`).
 *
 * A small draggable ball that expands into a compact action menu. Visibility
 * follows `floating_ball.enabled` from the persisted configuration and the
 * frontend-only `frontend_floater_enabled` event; position is remembered in
 * localStorage and clamped to the available monitors on startup. The ball
 * diameter and background alpha come from `floating_ball.size_px` /
 * `floating_ball.opacity` and are applied as CSS custom properties; the
 * collapsed window size and the expanded menu size follow the diameter.
 *
 * Dragging and clicking share the button: a press is classified as a drag
 * once the pointer moves past the 4 px threshold (`FLOATER_DRAG_THRESHOLD_PX`,
 * native `startDragging`), and as a click otherwise (menu toggle). Tauri's
 * `data-tauri-drag-region` attribute is deliberately not used here because
 * it would swallow clicks.
 *
 * `initialOpen` is a test seam only; production always starts collapsed.
 */
export function FloatingBall({ initialOpen = false }: { initialOpen?: boolean } = {}) {
  const [open, setOpen] = useState(initialOpen);
  const [busy, setBusy] = useState(false);
  const [opacity, setOpacity] = useState(1);
  const [sizePx, setSizePx] = useState(48);
  const [hydrated, setHydrated] = useState(false);
  const rootRef = useRef<HTMLElement | null>(null);
  const persistTimer = useRef<number | undefined>(undefined);
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
      // 启动水合：仅当配置开启时显示悬浮球，并把持久化的透明度/大小
      // 应用到 CSS 变量与本地 state。
      getAppConfig()
        .then((config) => {
          if (disposed) return;
          const nextOpacity = clampFloaterOpacity(config.floating_ball.opacity);
          const nextSizePx = clampFloaterSizePx(config.floating_ball.size_px);
          setOpacity(nextOpacity);
          setSizePx(nextSizePx);
          if (rootRef.current) {
            applyFloaterAppearance(rootRef.current, nextOpacity, nextSizePx);
          }
          if (config.floating_ball.enabled) applyVisibility(true);
        })
        .catch(() => undefined),
      // 主窗口设置面板切换开关时即时显隐。
      listenToFrontendFloaterEnabled(({ enabled }) => {
        if (!disposed) applyVisibility(enabled);
      }),
    ]).then(([, cleanup]) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
      setHydrated(true);
    });
    return () => {
      disposed = true;
      unlisten?.();
      window.clearTimeout(persistTimer.current);
    };
  }, []);

  // 水合完成后恢复上次位置并夹到可见显示器内；按水合后的直径计算，
  // 避免保存位置是按旧尺寸 clamp 的。
  useEffect(() => {
    if (!hydrated) return;
    let disposed = false;
    let unlistenMoved: (() => void) | undefined;
    void (async () => {
      const tauriWindow = getFloaterWindow();
      if (!tauriWindow) return;
      const monitors = await availableMonitors().catch(() => []);
      const saved = loadFloaterPosition(window.localStorage);
      if (saved && monitors.length > 0) {
        const clamped = clampFloaterPosition(saved, monitors, sizePx);
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
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 位置恢复只在水合后执行一次。
  }, [hydrated]);

  // 悬浮球直径（与展开/收起状态）驱动窗口尺寸；水合后的大小变化同样生效。
  useEffect(() => {
    const tauriWindow = getFloaterWindow();
    if (!tauriWindow) return;
    const logical = open
      ? new LogicalSize(MENU_WIDTH, sizePx + MENU_HEIGHT)
      : new LogicalSize(sizePx, sizePx);
    const resize = tauriWindow.setSize?.(logical);
    if (resize && typeof resize.catch === "function") {
      resize.catch(() => undefined);
    }
  }, [open, sizePx]);

  const schedulePersist = (nextOpacity: number, nextSizePx: number) => {
    window.clearTimeout(persistTimer.current);
    persistTimer.current = window.setTimeout(() => {
      void persistFloaterAppearance(nextOpacity, nextSizePx).catch((error) => {
        // 持久化失败只记脱敏日志，不影响已生效的即时外观。
        console.warn(
          `[vtrans] floating ball appearance persist failed: ${getIpcErrorMessage(error)}`,
        );
      });
    }, APPEARANCE_PERSIST_MS);
  };

  const changeOpacity = (value: number) => {
    const next = clampFloaterOpacity(value);
    setOpacity(next);
    if (rootRef.current) applyFloaterAppearance(rootRef.current, next, sizePx);
    schedulePersist(next, sizePx);
  };

  const changeSize = (value: number) => {
    const next = clampFloaterSizePx(value);
    setSizePx(next);
    if (rootRef.current) applyFloaterAppearance(rootRef.current, opacity, next);
    schedulePersist(opacity, next);
  };

  const collapseMenu = async () => {
    setOpen(false);
    const tauriWindow = getFloaterWindow();
    if (tauriWindow) {
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

  const dragHandlers = createFloaterDragHandlers({
    // 位移超过阈值才启动原生窗口拖动；失败静默（拖动是便利功能）。
    startDragging: () => {
      const tauriWindow = getFloaterWindow();
      if (!tauriWindow) return;
      void tauriWindow.startDragging().catch(() => undefined);
    },
    // 仅未拖动（点击）时展开/收起菜单。
    onToggle: () => void (open ? collapseMenu() : expandMenu()),
  });

  return (
    <main
      ref={rootRef}
      className="fixed inset-0 select-none overflow-hidden"
      aria-label="悬浮球"
    >
      <button
        type="button"
        onClick={dragHandlers.onClick}
        onMouseDown={dragHandlers.onMouseDown}
        onMouseMove={dragHandlers.onMouseMove}
        onMouseUp={dragHandlers.onMouseUp}
        className="floater-ball absolute left-0 top-0 flex items-center justify-center rounded-full text-white shadow-lg ring-2 ring-white/70 transition hover:brightness-110"
        title="VTrans 悬浮球"
        aria-expanded={open}
        data-testid="floating-ball"
      >
        <Languages size={Math.max(18, Math.round(sizePx * 0.45))} aria-hidden="true" />
      </button>
      {open && (
        <nav
          className="floater-menu-panel absolute left-0 w-[220px] space-y-1 rounded-xl border border-slate-200 bg-white p-2 shadow-xl"
          data-testid="floating-ball-menu"
        >
          <button
            type="button"
            onClick={() => void run(selectAndTranslateOnce)}
            disabled={busy}
            className="floater-menu-item"
          >
            <MousePointer2 size={15} aria-hidden="true" />
            框选翻译
          </button>
          <button
            type="button"
            onClick={() => void run(toggleLiveFromFloater)}
            disabled={busy}
            className="floater-menu-item"
          >
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
          <button
            type="button"
            onClick={() => void showMainWindow().then(() => collapseMenu())}
            className="floater-menu-item"
          >
            <MousePointer2 size={15} aria-hidden="true" />
            打开主窗口
          </button>
          <FloatingBallAppearanceControls
            opacity={opacity}
            sizePx={sizePx}
            onOpacityChange={changeOpacity}
            onSizeChange={changeSize}
          />
        </nav>
      )}
    </main>
  );
}
