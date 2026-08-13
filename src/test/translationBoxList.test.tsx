import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { TranslationBoxList } from "../components/TranslationBoxList";
import type { BoxStatus, TranslationBoxInfo } from "../types";

const BOX_0: TranslationBoxInfo = {
  box_id: 0,
  region: { monitor_id: "m0", x: 10, y: 20, width: 300, height: 400 },
  color: "#FF6B6B",
};
const BOX_1: TranslationBoxInfo = {
  box_id: 1,
  region: { monitor_id: "m0", x: 1, y: 2, width: 30, height: 40 },
  color: "#4ECDC4",
};

const noop = () => undefined;

function renderList(
  boxes: TranslationBoxInfo[],
  statuses: Record<number, BoxStatus>,
  warningThreshold = 4,
) {
  return renderToStaticMarkup(
    <TranslationBoxList
      boxes={boxes}
      statuses={statuses}
      warningThreshold={warningThreshold}
      onAdd={noop}
      onEdit={noop}
      onRemove={noop}
      onStart={noop}
      onStop={noop}
      onStopBox={noop}
    />,
  );
}

describe("TranslationBoxList", () => {
  it("renders each box with a color swatch, ordinal and actions", () => {
    const html = renderList([BOX_0, BOX_1], { 0: "Stopped", 1: "Stopped" });
    expect(html).toContain("#FF6B6B");
    expect(html).toContain("#4ECDC4");
    expect(html).toContain("框 1");
    expect(html).toContain("框 2");
    expect(html).toContain("新增翻译框");
    expect(html).toContain("编辑框 1 区域");
    expect(html).toContain("删除框 1");
  });

  it("shows the empty-state guidance when no boxes exist", () => {
    const html = renderList([], {});
    expect(html).toContain("尚未添加翻译框");
    expect(html).toContain('data-testid="multibox-empty"');
  });

  it("shows the persistent warning bar at or above the threshold", () => {
    const boxes = [BOX_0, BOX_1, BOX_0, BOX_1];
    const html = renderList(boxes, {}, 4);
    expect(html).toContain('data-testid="multibox-warning"');
    expect(html).toContain("翻译框过多可能导致卡顿，建议不超过 4 个");
  });

  it("hides the warning bar below the threshold", () => {
    const html = renderList([BOX_0, BOX_1], {}, 4);
    expect(html).not.toContain('data-testid="multibox-warning"');
  });

  it("shows per-box status badges", () => {
    const html = renderList([BOX_0, BOX_1], { 0: "Running", 1: { Error: "boom" } });
    expect(html).toContain("运行中");
    expect(html).toContain("错误");
  });

  it("shows a stop action only for running boxes", () => {
    const html = renderList([BOX_0, BOX_1], { 0: "Running", 1: "Stopped" });
    expect(html).toContain("停止框 1");
    expect(html).not.toContain("停止框 2");
    // The running box drives the whole session into the "stop all" control.
    expect(html).toContain("停止全部");
  });

  it("offers start when nothing is running", () => {
    const html = renderList([BOX_0], { 0: "Stopped" });
    expect(html).toContain("开始多框实时");
    expect(html).not.toContain("停止全部");
  });

  it("never renders coordinates, size or shape information", () => {
    const html = renderList([BOX_0], { 0: "Stopped" });
    expect(html).not.toContain("物理像素");
    expect(html).not.toContain("位置");
    expect(html).not.toContain("坐标");
    expect(html).not.toContain("尺寸");
    expect(html).not.toContain("形状");
    // The region values and monitor id must not leak into the list either.
    expect(html).not.toMatch(/300\s*[x×]\s*400/);
    expect(html).not.toContain("m0");
  });
});
