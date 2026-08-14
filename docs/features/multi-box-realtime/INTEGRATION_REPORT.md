# 整合报告：多框实时翻译（Multi-Box Realtime Translation）

- 整合日期：2026-08-13
- 整合人：功能统筹 Agent
- 基准分支：main（1fe7380）→ 整合后 main（dd0e5e1）

## 合并记录

| 模块 | 分支 | 合并顺序 | 合并提交 | 结果 |
|------|------|----------|----------|------|
| 02-config | feat/multibox-config | 1 | 9e8e626 | ✅ 无冲突 |
| 06-text | feat/multibox-text | 2 | 7a3b97b | ✅ 无冲突 |
| 09-pipeline | feat/multibox-pipeline | 3 | 1258528 | ✅ 无冲突 |
| 10-app | feat/multibox-app | 4 | 513929f | ✅ 无冲突 |
| 11-frontend | feat/multibox-frontend | 5 | dd0e5e1 | ✅ 无冲突 |

全部按依赖层级顺序合并（层级 1 → 3 → 4），每次合并后 `cargo check --workspace` 通过。

## 集成验证

### 质量门禁（整合前 feature 分支 tip 与整合后 main 各跑一轮，结果一致）

| 检查项 | 结果 |
|--------|------|
| cargo fmt --all -- --check | PASS |
| cargo clippy --workspace --all-targets | PASS（0 错误；1 个既有警告位于 vtrans-translation，非本功能引入） |
| cargo test --workspace | PASS（全部 crate 测试绿，含 pipeline_multibox 909 行并发测试） |
| pnpm test | PASS（38 个测试文件，264 个测试） |
| pnpm exec tsc --noEmit | PASS |
| cargo check --workspace（含 src-tauri 二进制） | PASS |

### 契约核对

- 8 个新增 Command：`add_translation_box` / `remove_translation_box` / `update_translation_box` / `list_translation_boxes` / `start_multi_realtime` / `stop_multi_realtime` / `stop_box` / `open_result_window` — Rust（commands.rs invoke_handler）与前端（services/tauri.ts）两端签名一致，camelCase 参数映射正确。
- 7 个新增 Event：`multibox://result` / `box-added` / `box-removed` / `box-updated` / `status` / `warning`、`translation://single-result` — Rust（events.rs 常量）与前端（services/events.ts 常量）名称一致，payload serde 形状与 TypeScript 类型一一对应（contracts.rs 与 multiboxTypes.test.ts 双向断言）。
- 冻结契约：vtrans-core 未修改；TranslationBox/BoxedTranslationResult/BoxStatus 定义于 vtrans-pipeline，BoxId 为 u32。
- 配置：AppConfig 新增 `translation_boxes`/`max_boxes`/`warning_threshold`，v5→v6 迁移与校验齐备；前端 DEFAULT_CONFIG 与后端 `CURRENT_CONFIG_VERSION = 6` 一致。

### 验收标准对照（16 条，代码层面对照）

| # | 验收条目 | 结果 |
|---|----------|------|
| 1 | 主页面翻译框列表增删改 | ✅ TranslationBoxList + multiBoxActions |
| 2 | 新增框走选区窗口、自动配色 | ✅ 后端 next_box_id / next_box_color |
| 3 | 每框不同颜色、overlay 彩色方框 | ✅ OverlayWindow 按 color 渲染 |
| 4 | 启动后所有框同时采集翻译 | ✅ MultiBoxPipeline.start_all（每框独立 tokio task） |
| 5 | 弹窗多框结果由上到下+分隔线 | ✅ MultiBoxResults 堆叠布局 |
| 6 | 每框内容用对应颜色边框包含 | ✅ border: 2px solid color |
| 7 | 超阈值卡顿提示 | ✅ multibox://warning + toast + 持久警告条 |
| 8 | 单次翻译单框、结果走弹窗 | ✅ translation://single-result；捕获完成自动弹窗 |
| 9 | 单独停止某框 / 一键停止全部 | ✅ stop_box / stop_multi_realtime |
| 10 | 修改区域实时生效 | ✅ pipeline.update_box 停旧 task 以新区域重启 |
| 11 | 删除框后结果从弹窗移除 | ✅ store.removeBox 同删 status/results |
| 12 | 主页面不显示结果、显示弹窗按钮 | ✅ MainWindow 无结果渲染 + 按钮 |
| 13 | 弹窗已存在仅置顶不重复 | ✅ open_result_window show+set_focus（窗口预声明、关闭即隐藏） |
| 14 | 主页面不显示坐标/大小/形状 | ✅ 列表仅编号+颜色+状态 |
| 15 | 单次与实时均使用弹窗展示 | ✅ ResultWindow 双布局 |
| 16 | （重复项，同 8/15） | ✅ |

> ⚠️ 弹窗「每框显示原文+译文」为 PLAN 弹窗设计节的细化要求，当前多框区域仅显示译文（见遗留问题 1）。

### 回归范围

- 单次翻译链路（capture_once / 选区窗口 / ocr_completed / translation_completed 事件）保留；新增 single-result 事件为补充通道。
- 单框实时链路（start/stop_live_translation、pause）保留；热键 Alt+Shift+A/R/S 行为未变（见遗留问题 2）。
- 设置保存、provider 切换、模型校验、托盘、悬浮球、外观命令未改动。
- 已知限制沿用：本地模型仅 en→zh-CN；图像不跨 IPC；快捷键修改需重启。

## 遗留问题

| # | 问题 | 性质 | 负责人 | 状态 |
|---|------|------|--------|------|
| 1 | 多框结果不含原文：BoxedTranslationResult 仅携带 TranslationResult（译文），弹窗多框区域无法显示原文。需 pipeline 层将 OCR 文本与翻译结果配对后增加字段（影响 pipeline/frontend；app 仅 README 同步）。vtrans-app README 已注明为已知限制 | 功能缺口（PLAN 弹窗设计细化要求） | 模块开发 Agent（后续迭代） | **已确认纳入后续迭代**（2026-08-13 用户决策）；任务单见 TASK-FOLLOWUP-09-pipeline.md / TASK-FOLLOWUP-11-frontend.md |
| 2 | 热键 Alt+Shift+R/S 仍启动/停止单框实时会话，未接入多框（hotkeys.rs 未改动）。与「复用现有热键」确认决策的解读偏差 | 需用户确认语义 | 用户 | **已确认保持现状**（2026-08-13 用户决策）：R/S 控制单框实时，多框仅 UI 按钮。记录为设计决策，关闭 |
| 3 | docs/modules/01-10*.md 与 docs/ARCHITECTURE.md 未同步多框契约（新命令/事件、MultiBoxPipeline、TranslationBoxConfig、v5 迁移）。crate README 与 src/README.md 已更新 | 文档同步缺口 | 各模块开发 Agent | **已确认派单补齐**（2026-08-13 用户决策）；任务单见 TASK-DOCSYNC.md |
| 4 | 本地 main 领先 origin/main 一个提交 1fe7380（2026-08-11 旧 feature-plans 文档清理）。沙箱无网络，fetch/push 需用户执行；推送时该提交一并带出 | 推送提醒 | 用户 | 待推送 |
| 5 | GUI 端到端冒烟（cargo tauri dev 下框选、多框实时、弹窗布局、警告、热键回归）无法在沙箱执行，需有显示环境手工验证 | 手工验证项 | 用户/验收 | 待验收 |
| 6 | clippy 既有警告 1 处（vtrans-translation「items after a test module」），非本功能引入 | 非阻塞 | — | 观察 |

## 后续迭代整合（2026-08-13 派发子代理执行）

| 任务 | 模块 | 分支 | 合并提交 | Review 结果 |
|------|------|------|----------|-------------|
| F1 原文字段 | 09-pipeline | feat/multibox-original-text-pipeline | 0fca96d | ✅ 范围仅 pipeline；fmt/clippy 0 警告；90 测试全绿；契约：`original_text` serde 名正确、三参 `new` 保持兼容（`with_original_text` builder） |
| F2 原文展示 | 11-frontend | feat/multibox-original-text-frontend | 223f600 | ✅ 范围仅 src/；tsc 通过；266 测试全绿；每框译文上方次级色常显原文，空串不占位 |
| D1 文档同步 | 02/06/09/10 + ARCHITECTURE | docs/multibox-contract-sync | 9a00507 | ✅ 仅文档；关键事实抽检（8 色调色板、max_boxes 1..=32、broadcast max_boxes*2、26 命令）与代码一致 |

遗留问题状态更新：

| # | 问题 | 状态 |
|---|------|------|
| 1 | 多框结果不含原文 | **已解决**（F1 + F2 已整合，弹窗每框显示原文+译文） |
| 2 | 热键语义 | **保持现状**（用户决策，记录为设计决策，关闭） |
| 3 | docs/modules 与 ARCHITECTURE 未同步 | **已解决**（D1 已整合，含 vtrans-app README 已知限制段刷新） |
| 4 | 推送提醒（本地 main 领先 origin，含 1fe7380 文档清理提交） | 待推送 |
| 5 | GUI 端到端冒烟 | 待手工验收 |
| 6 | clippy 既有警告（vtrans-translation） | 观察 |
| 7 | **flaky 测试**：vtrans-translation tests/api_provider.rs 的 mock HTTP 用例（如 retry_after_header_is_honored、http_401_returns_unauthorized、retries_until_success）在 2026-08-13 多框整合门禁中出现随机失败（HTTP 502/断言失败，复跑通过）。取证：`git diff 1fe7380..main -- crates/vtrans-translation` 仅 Cargo.lock 差异，源码未变 → 与多框无关的既有缺陷（cloud provider 时代引入的 mock 时序竞态） | 既有缺陷 | 07-translation 模块开发 Agent（建议派 fix/07-api-provider-flaky-tests） | 待派单 |

## UI 缺陷修复整合（2026-08-13，用户 bug 报告）

| Bug | 描述 | 修复 | 合并提交 |
|-----|------|------|----------|
| B1 | 主页面「翻译区域」与「翻译框」两栏在两种模式下同时存在 | single 模式仅「翻译区域」（含选择并翻译）；live 模式仅「翻译框」+ 单框实时控制行；runLive 无选区自动先框选再启动 | 07bcc3f（分支 fix/11-multibox-ui-bugs） |
| B2 | 弹窗多框原文应逐框折叠而非 F2 的常显 | 每框默认折叠 + chevron 开关，展开状态按 box_id 记忆，空原文无开关 | 同上 |
| B3 | 弹窗无滚动条，文本过多只能扩大窗口 | 根容器 min-h-screen→h-screen；多框滚动容器与单框译文区补 min-h-0+overflow-y-auto（flexbox min-height:auto 陷阱）；滚动条不隐藏 | 同上 |

Review：范围仅 src/（6 文件）；pnpm 273 测试（39 文件）+ tsc 通过；无 console.log 残留。任务单：TASK-BUGFIX-11-ui.md。

## UI 交互缺陷修复整合（2026-08-13，用户第二轮 bug 报告）

| Bug | 描述 | 修复 | 合并提交 |
|-----|------|------|----------|
| B2-1 | 单次模式出现「选择屏幕区域」与「选择并翻译」两个同功能按钮 | 仅保留「选择并翻译」 | e781c40（分支 fix/11-multibox-interaction） |
| B2-2 | live 模式「开始多框实时」与「开始实时」并存 | 列表删除会话级启停按钮；底部「开始实时/停止」统一控制多框；删除「暂停/继续实时」（单框实时由热键/悬浮球/弹窗控制，既定设计决策） | 同上 |
| B2-3 | overlay 实时框与实际框位置不符 | 根因：多框启动时后端只 show overlay 不定位；修复：启动前前端把 overlay 定位/铺满第一个框所在显示器（物理坐标），失败仅告警；已知限制：单窗口只覆盖一个显示器 | 同上 |
| B2-4 | 选区窗口每次残留上一次的框，需先点「重新选择」 | 确认成功/取消/系统关闭三条退出路径均重置选区状态，每次打开从空白开始 | 同上 |

Review：范围仅 src/（10 文件）；pnpm 282 测试（39 文件）+ tsc 通过；无 console.log 残留；IPC 契约未变更。任务单：TASK-BUGFIX-11-interaction.md。
> 门禁说明：合并后 main 全量门禁中 cargo test --workspace 出现 vtrans-translation api_provider 的随机失败（遗留 7，与本功能无关、该 crate 源码未变）；fmt/clippy/pnpm/tsc 与其余全部 workspace 测试均 PASS，多框相关 crate（config/text/pipeline/app）测试全绿。

## 结论

- [x] 功能已整合：5 个模块分支 + 后续迭代 F1/F2/D1 + 两轮 UI 缺陷修复（B1-B3、B2-1..4）全部合并入 main（当前 e781c40），零冲突
- [x] 原文缺口、文档同步、两轮共 7 处 UI/交互缺陷均已闭环
- [x] 功能关闭前置之 GUI 手工冒烟：已于 2026-08-14 第三轮缺陷修复冒烟中通过（A1-A3 / B1-B4 / C1-C3 / D1-D2 全部符合预期，用户确认「正常」）；遗留 4（推送 main）由用户授权执行中

## 第三轮缺陷修复整合（2026-08-14，用户 bug 报告：窗口隐藏 / 状态同步 / 自捕获排除）

| Bug | 描述 | 修复分支（合并提交） | 门禁与验证 |
|-----|------|---------------------|-----------|
| Bug-004 | 悬浮球与主页面「实时翻译」状态不同步 | fix/11-floater-live-sync（9425647）+ fix/10-multibox-status-snapshot（2fa4226） | 各分支 fmt/clippy/test 全绿；GUI 冒烟 B1-B4 通过 |
| Bug-005 | 框选期间未隐藏 VTrans 自身窗口 | fix/10-hide-windows-on-selection（5b87f03） | 门禁全绿；GUI 冒烟 A1-A3 通过 |
| Bug-006 | 翻译框捕获 VTrans 自身窗口 | fix/10-window-capture-exclusion（5749f64）+ fix/11-overlay-border-outside（58432d6）+ fix/04-wgc-wda-verification（4698999） | 门禁全绿；WDA 实机验证 + GUI 冒烟 C1-C3 通过 |

要点：

- Bug-005：框选开始隐藏 main/result/floater（快照「框选前可见集合」）；取消/超时立即恢复；确认成功延迟到
  `capture_once` / `start_live_translation` / `add_translation_box` / `update_translation_box` 完成后恢复（成败都恢复）；
  floater 恢复受 `floating_ball.enabled` 约束；连续框选不覆盖首次快照。
- Bug-004：前端新增纯前端事件 `frontend_multibox_started/stopped` 广播多框会话状态；悬浮球运行态 = 单框 live ∪
  任一框 Running（共享推导函数，无复制逻辑），按钮与主页面统一驱动多框；后端 `start/stop_multi_realtime` 同步
  `current_mode`（停止时并发单框 live 保护，`mode_after_multi_stop` 纯函数）。
- Bug-006：`WDA_EXCLUDEFROMCAPTURE` 实机验证对 WGC 显示器捕获**生效**（wda_probe：红色窗口 398‰→0‰、露出背景）；
  main/result/floater 设置 WDA 排除捕获；overlay 描边外移出捕获区域（区域贴屏边时该侧描边内缩，已知限制 18）。
  副作用：VTrans 窗口在一切第三方捕获（含用户截图工具）中不可见——用户已接受该权衡。
- 数量：前端测试 312（+30）、vtrans-app 108+14、vtrans-capture 55；六分支合并零冲突；未触碰 vtrans-core（冻结契约）。

## 附：合并后 main 状态

```text
dd0e5e1 merge: integrate multi-box frontend module (11)
513929f merge: integrate multi-box app module (10)
1258528 merge: integrate multi-box pipeline module (09)
7a3b97b merge: integrate multi-box text module (06)
9e8e626 merge: integrate multi-box config module (02)
1fe7380 useless docs clear（本地领先 origin/main）
```
