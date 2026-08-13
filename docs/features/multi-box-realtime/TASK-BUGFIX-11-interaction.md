# 模块开发说明：11-frontend — 多框交互四缺陷修复（BUGFIX-2）

## AGENT_DEV_PROMPT 参数
- MODULE_NUMBER: 11
- MODULE_NAME: frontend
- MODULE_SLUG: frontend
- CRATE_PATH: src
- SCOPE: frontend
- BRANCH_NAME: fix/11-multibox-interaction（从 main 拉分支；main 已含 BUGFIX-1 修复）

## Bug 来源（用户报告，2026-08-13，均 11-frontend，统筹已定位根因）

1. 单次翻译时出现「选择屏幕区域」与「选择并翻译」两个按钮，实际是同一功能，只应保留一个。
2. 实时翻译时翻译框内出现「开始多框实时」，下方又有「开始实时」；翻译框内的「开始多框实时」不需要。
3. 实时翻译时屏幕上显示的实时框与实际框位置不符。
4. 屏幕区域选择时每次都显示上一次的框，需点「重新选择」才能本次修改；应每次打开选区窗口直接开始新选择。

## 任务要求

### Bug 1：单次模式只保留一个按钮（src/windows/MainWindow.tsx）
- single 模式「翻译区域」区块内只保留**一个**按钮：「选择并翻译」（行为不变：框选 → capture_once → 弹窗展示）。删除「选择屏幕区域」按钮。
- live 模式不受影响（该区块在 live 下不渲染）。

### Bug 2：实时模式统一由底部按钮控制多框会话
- src/components/TranslationBoxList.tsx：删除列表底部的「开始多框实时」/「停止全部」按钮组（保留：新增翻译框、每框的停止/编辑/删除按钮、警告条、空态引导）。
- src/windows/MainWindow.tsx live 模式底部控制行改为控制**多框会话**：
  - 「开始实时」→ handleStartMulti（startMultiBox）；disabled 条件：multiBusy 或 translationBoxes 为空 或 已有框 Running（anyRunning 由 boxStatuses 派生）。
  - 「停止」→ handleStopMulti（stopMultiBox）；disabled 条件：multiBusy 或 无框 Running。
  - 删除「暂停/继续实时」按钮（多框无暂停概念；单框实时会话由 Alt+Shift+R/S 热键、悬浮球与结果弹窗控制，这是已确认的设计决策）。
- livePaused 相关状态仍保留给弹窗/悬浮球使用，主窗口 live 模式不再消费。

### Bug 3：多框启动时把 overlay 窗口定位到翻译框所在显示器（src/services/regionOverlay.ts + multiBoxActions.ts）
- 根因：`start_multi_realtime` 后端只 show overlay 窗口不定位；overlay 默认尺寸与框所在显示器不匹配，框按 物理坐标/dpr 画在错误窗口里。
- 新增 `showMultiBoxOverlay(boxes: { region: ScreenRegion }[])`（或等价命名）：
  - 取第一个框的 `region.monitor_id`，经 `availableMonitors()` 解析显示器（找不到则回退 monitors[0]）；
  - `setPosition(PhysicalPosition(monitor.position))` + `setSize(PhysicalSize(monitor.size))` + `setIgnoreCursorEvents(true)`；**只定位不 show**（后端 start_multi_realtime 负责 show，避免闪烁）；
  - 复用/参照现有 `showRegionOverlay` 的实现与错误处理风格（失败仅 console.warn，不阻断翻译）。
- src/services/multiBoxActions.ts 的 `startMultiBox()`：invoke `start_multi_realtime` **之前**先调 `showMultiBoxOverlay(useAppStore.getState().translationBoxes)`。
- 已知限制（记录到 src/README.md 多框节）：单个 overlay 窗口只能覆盖一个显示器；多框分布在不同显示器时，仅目标显示器上的框位置准确（多显示器支持为后续迭代，不在本 bug 范围）。

### Bug 4：每次打开选区窗口都是全新选择（src/windows/RegionSelector.tsx）
- 根因：start/end/phase 为组件本地状态，窗口 hide 后不销毁，再次打开残留上次「已确认」状态。
- 复用现有 `resetSelection`：在**所有退出路径**重置状态（start/end 置 null、phase 回 selecting、message 回默认）：
  - `confirmSelection` 成功提交后（updateLiveRegion 成功、hide 之前）；提交失败不重置（保留选区供重试）；
  - `cancelSelection`（Esc / 取消按钮）hide 之前；
  - `onCloseRequested` 处理器内（系统关闭路径）。
- 每次打开选区窗口即从空白开始拖框，无需先点「重新选择」。

## 测试要求（src/test/）
- mainWindowModeSections.test.tsx：同步 single 单按钮（无「选择屏幕区域」）、live 底部按钮为多框控制（可断言文案与 disabled 逻辑，如无选区相关断言则按现状调整）。
- translationBoxList.test.tsx：删除「开始多框实时/停止全部」相关断言（如有）。
- regionOverlay 测试：新增 showMultiBoxOverlay 的定位调用断言（mock availableMonitors/WebviewWindow，参照 regionOverlay.test.ts 既有模式）。
- RegionSelector 测试（如有）：新增重置断言；否则至少保证既有测试全绿。
- 全部既有测试（当前 273 个）必须保持通过。

## 横切标准提醒
- 质量门禁：pnpm test；pnpm exec tsc --noEmit；cargo check --workspace（只读）。
- 无 console.log 残留（console.warn 用于 IPC/overlay 失败日志属既有惯例）。

## 提交规范（按 bug 拆分，可多提交）
- fix(frontend): keep single select-and-translate button in single mode
- fix(frontend): drive multi-box session from live-mode start/stop buttons
- fix(frontend): position overlay to box monitor on multi-box start
- fix(frontend): reset region selector on every exit path

## 完成定义（DoD）
- [ ] 四个 bug 行为与上述描述一致
- [ ] pnpm test / tsc / cargo check --workspace 全部通过
- [ ] 范围仅限 src/；未修改任何 Rust 文件
- [ ] 未 merge 到 main、未 push
- [ ] PR 描述含四处修复说明、测试覆盖与已知限制（多显示器 overlay）

## 环境提示
pnpm/cargo 不在默认 PATH，运行命令前先执行：
export PATH="/c/Users/ArrogHie/.cargo/bin:/d/NodeJs:/c/Users/ArrogHie/AppData/Roaming/npm:$PATH"
