import { describe, expect, it, vi } from "vitest";
import {
  createFloaterDragHandlers,
  FLOATER_DRAG_THRESHOLD_PX,
  type FloaterDragHandlers,
} from "../utils/floaterDrag";

function setup(threshold?: number): {
  handlers: FloaterDragHandlers;
  startDragging: ReturnType<typeof vi.fn>;
  onToggle: ReturnType<typeof vi.fn>;
} {
  const startDragging = vi.fn();
  const onToggle = vi.fn();
  const handlers = createFloaterDragHandlers({ threshold, startDragging, onToggle });
  return { handlers, startDragging, onToggle };
}

function press(handlers: FloaterDragHandlers, x: number, y: number, button = 0): void {
  handlers.onMouseDown({ button, clientX: x, clientY: y });
}

function move(handlers: FloaterDragHandlers, x: number, y: number): void {
  handlers.onMouseMove({ clientX: x, clientY: y });
}

function release(handlers: FloaterDragHandlers): void {
  handlers.onMouseUp();
}

function click(handlers: FloaterDragHandlers): void {
  handlers.onClick();
}

describe("createFloaterDragHandlers", () => {
  it("classifies a sub-threshold move as a click: no drag, toggle runs", () => {
    const { handlers, startDragging, onToggle } = setup();
    press(handlers, 10, 10);
    move(handlers, 12, 12); // 位移 ≈ 2.83px < 4px 阈值
    click(handlers);
    expect(startDragging).not.toHaveBeenCalled();
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("starts a drag at exactly the threshold and suppresses the toggle", () => {
    const { handlers, startDragging, onToggle } = setup();
    press(handlers, 10, 10);
    move(handlers, 14, 10); // 水平位移恰为 4px
    click(handlers);
    expect(startDragging).toHaveBeenCalledTimes(1);
    expect(onToggle).not.toHaveBeenCalled();
  });

  it("starts a drag past the threshold and suppresses the toggle", () => {
    const { handlers, startDragging, onToggle } = setup();
    press(handlers, 10, 10);
    move(handlers, 20, 20); // 位移 ≈ 14.1px ≥ 4px
    click(handlers);
    expect(startDragging).toHaveBeenCalledTimes(1);
    expect(onToggle).not.toHaveBeenCalled();
  });

  it("starts the native drag only once per gesture", () => {
    const { handlers, startDragging } = setup();
    press(handlers, 0, 0);
    move(handlers, 10, 0);
    move(handlers, 30, 30);
    move(handlers, 60, 60);
    expect(startDragging).toHaveBeenCalledTimes(1);
  });

  it("recovers to a plain click after a completed drag gesture", () => {
    const { handlers, startDragging, onToggle } = setup();
    press(handlers, 0, 0);
    move(handlers, 20, 20);
    release(handlers);
    click(handlers);
    expect(startDragging).toHaveBeenCalledTimes(1);
    expect(onToggle).not.toHaveBeenCalled();

    // 下一次按下重新进入点击分支（onMouseDown 重置拖动标记）。
    press(handlers, 5, 5);
    release(handlers);
    click(handlers);
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("treats a press without any move as a click", () => {
    const { handlers, startDragging, onToggle } = setup();
    press(handlers, 8, 8);
    release(handlers);
    click(handlers);
    expect(startDragging).not.toHaveBeenCalled();
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("ignores non-left mouse buttons", () => {
    const { handlers, startDragging, onToggle } = setup();
    press(handlers, 0, 0, 2); // 右键
    move(handlers, 50, 50);
    click(handlers);
    expect(startDragging).not.toHaveBeenCalled();
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("honours a custom threshold", () => {
    const { handlers, startDragging, onToggle } = setup(10);
    press(handlers, 0, 0);
    move(handlers, 5, 0);
    click(handlers);
    expect(startDragging).not.toHaveBeenCalled();
    expect(onToggle).toHaveBeenCalledTimes(1);

    press(handlers, 0, 0);
    move(handlers, 12, 0);
    click(handlers);
    expect(startDragging).toHaveBeenCalledTimes(1);
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("exposes the documented threshold constant", () => {
    expect(FLOATER_DRAG_THRESHOLD_PX).toBe(4);
  });
});
