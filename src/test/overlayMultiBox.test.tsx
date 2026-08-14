import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import {
  OVERLAY_BORDER_PX,
  OverlayFrame,
  overlayBorderBox,
  overlayBox,
  upsertOverlayBox,
  type OverlayMultiBox,
} from "../windows/OverlayWindow";

const REGION = { monitor_id: "\\\\.\\DISPLAY1", x: 0, y: 0, width: 100, height: 100 };

describe("upsertOverlayBox", () => {
  it("appends a new box", () => {
    const box: OverlayMultiBox = { box_id: 0, color: "#FF6B6B", region: REGION };
    expect(upsertOverlayBox([], box)).toEqual([box]);
  });

  it("replaces an existing box with the same id (idempotent hydration + event)", () => {
    const first: OverlayMultiBox = { box_id: 0, color: "#FF6B6B", region: REGION };
    const updated: OverlayMultiBox = {
      box_id: 0,
      color: "#FF6B6B",
      region: { ...REGION, width: 200 },
    };
    const list = upsertOverlayBox([first], updated);
    expect(list).toHaveLength(1);
    expect(list[0].region.width).toBe(200);
  });

  it("keeps distinct boxes in insertion order", () => {
    const a: OverlayMultiBox = { box_id: 0, color: "#FF6B6B", region: REGION };
    const b: OverlayMultiBox = { box_id: 1, color: "#4ECDC4", region: REGION };
    const list = upsertOverlayBox(upsertOverlayBox([], a), b);
    expect(list.map((entry) => entry.box_id)).toEqual([0, 1]);
  });
});

describe("multi-box overlay frame", () => {
  it("renders each box with the same outset border geometry as single live mode", () => {
    const region = { ...REGION, x: 40, y: 60, width: 300, height: 200 };
    const geometry = overlayBorderBox(overlayBox(region, 1), {
      width: 1920,
      height: 1080,
    });
    // 与单框同一渲染路径与几何规则：四边外移 2px，描边带全部位于区域之外。
    expect(geometry.insetLeft).toBe(OVERLAY_BORDER_PX);
    expect(geometry.left + OVERLAY_BORDER_PX).toBe(40);
    const html = renderToStaticMarkup(
      <OverlayFrame
        geometry={geometry}
        color="#FF6B6B"
        labelColor="#FF6B6B"
        label="框 1"
        testId="overlay-multibox-0"
      />,
    );
    expect(html).toContain('data-testid="overlay-multibox-0"');
    expect(html).toContain("border-2");
    expect(html).toContain("border-color:#FF6B6B");
    expect(html).toContain("background-color:#FF6B6B");
    expect(html).toContain("left:38px");
    expect(html).toContain("top:58px");
    expect(html).toContain("width:304px");
    expect(html).toContain("height:204px");
    expect(html).toContain("框 1");
  });
});
