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

## 结论

- [x] 功能已整合：5 个模块分支按依赖顺序 --no-ff 合并入 main，零冲突，workspace 与前端全部质量门禁通过
- [x] 存在遗留问题，需跟踪（见上表：1 功能缺口、2 热键语义、3 文档同步、5 手工验收）
- [ ] 功能关闭前置：遗留问题 1/2 经用户决策后，手工冒烟（遗留 5）通过方可置「已验收 / 已关闭」

## 附：合并后 main 状态

```text
dd0e5e1 merge: integrate multi-box frontend module (11)
513929f merge: integrate multi-box app module (10)
1258528 merge: integrate multi-box pipeline module (09)
7a3b97b merge: integrate multi-box text module (06)
9e8e626 merge: integrate multi-box config module (02)
1fe7380 useless docs clear（本地领先 origin/main）
```
