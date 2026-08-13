# 模块开发说明：11-frontend — 多框 UI 三处缺陷修复（BUGFIX）

## AGENT_DEV_PROMPT 参数
- MODULE_NUMBER: 11
- MODULE_NAME: frontend
- MODULE_SLUG: frontend
- CRATE_PATH: src
- SCOPE: frontend
- BRANCH_NAME: fix/11-multibox-ui-bugs（从 main 拉分支；main 已含多框功能与 F1/F2/D1 全部整合）

## Bug 来源（用户报告，2026-08-13）
1. 主页面「翻译框」与「翻译区域」两栏在单次/实时两种模式下同时存在，应分开：「翻译框」属于实时翻译，「翻译区域」属于单次翻译，互不干扰。
2. 翻译弹窗多框原文应使用逐框折叠，而非 F2 实现的常显。
3. 翻译弹窗无滚动条，文本过多时只能靠扩大窗口阅读。

## 任务要求

### Bug 1：主页面按模式分离两个区块（src/windows/MainWindow.tsx）
- `mode === "single"`：渲染「翻译区域」区块（选择屏幕区域 + 底部「选择并翻译」），**不渲染** TranslationBoxList。
- `mode === "live"`：渲染 TranslationBoxList（多框列表 + 开始/停止多框实时），**不渲染**「翻译区域」区块。
- 底部单框实时控制行（开始实时/暂停/停止）保留在 live 模式：为避免隐藏「翻译区域」后单框实时失去 UI 入口，`runLive` 在无 `selectedRegion` 时回退为 `selectRegionForLive()`（先框选再启动，与悬浮球 `toggleLiveFromFloater` 行为一致）。实现时对照 src/services/translateActions.ts 的 startLive/selectRegionForLive 语义。
- 「打开翻译弹窗」按钮两种模式都保留（单次与实时都走弹窗）。

### Bug 2：多框弹窗原文逐框折叠（src/components/MultiBoxResults.tsx）
- 每框原文默认**折叠**；每框头部增加折叠开关（chevron 图标 + 可点击区域，风格与 ResultWindow 单框布局的「原文」折叠开关一致）。
- 展开后显示原文（保留 F2 的次级色样式：result-text + text-slate-500 + bg-slate-100/70）。
- `original_text` 为空字符串时不渲染开关与占位。
- 框编号、颜色色块、状态徽章、彩色边框、分隔线保持现状。
- 折叠状态按 box_id 记忆于组件内（多框结果高频更新时不因重渲染丢失展开状态）。

### Bug 3：翻译弹窗滚动（src/windows/ResultWindow.tsx + src/components/MultiBoxResults.tsx）
- 根容器 `min-h-screen` 改为 `h-screen`（固定为视口高度，窗口 resize 时自适应）。
- MultiBoxResults 滚动容器保持 `flex-1 overflow-y-auto`，**增加 `min-h-0`**（flex 子项默认 min-height:auto 会阻止收缩，是滚动条不出现的根因；result 窗口 body 的 overflow:hidden 会把撑高的内容裁掉）。
- 单框布局译文区（`data-testid="result-translation-text"` 的 p）同样加 `min-h-0` 与 `overflow-y-auto`，防止 h-screen 后长译文被裁切（防回归）。
- 验证：多框多文本时滚动条出现在弹窗内部；窗口拖拽缩放时滚动区自适应。滚动条样式用 WebView2 默认即可，不额外隐藏。

## 测试要求（src/test/）
- Bug 1：MainWindow 相关测试补充/更新——single 模式不渲染多框列表、live 模式不渲染「翻译区域」。
- Bug 2：multiBoxResults.test.tsx——默认折叠（原文不可见）、点击开关后展开、空原文不渲染开关；既有边框/分隔线/状态徽章断言保持通过。
- Bug 3：类名/结构断言更新（如现有测试断言根容器类名则同步）；无 DOM 级滚动断言的测试不必强行新增 CSS 行为断言，保证既有测试全绿即可。
- 全部既有测试（当前 266 个）必须保持通过。

## 横切标准提醒
- 质量门禁：pnpm test；pnpm exec tsc --noEmit；cargo check --workspace（只读验证 Rust 侧无破坏）。
- 无 console.log 残留；事件监听清理与错误处理遵循现有惯例。

## 提交规范（按 bug 拆分，可多提交）
- fix(frontend): split region and translation box sections by mode
- fix(frontend): collapse original text per multi-box section
- fix(frontend): make multi-box popup content scrollable

## 完成定义（DoD）
- [ ] 三个 bug 的行为修复与上述描述一致
- [ ] pnpm test / tsc / cargo check --workspace 全部通过
- [ ] 范围仅限 src/；未修改任何 Rust 文件
- [ ] 未 merge 到 main、未 push
- [ ] PR 描述含三处修复说明与测试覆盖

## 环境提示
pnpm/cargo 不在默认 PATH，运行命令前先执行：
export PATH="/c/Users/ArrogHie/.cargo/bin:/d/NodeJs:/c/Users/ArrogHie/AppData/Roaming/npm:$PATH"
