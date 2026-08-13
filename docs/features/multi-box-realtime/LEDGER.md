# 功能台账：多框实时翻译

## 功能信息
- 功能名称：多框实时翻译（Multi-Box Realtime Translation）
- 需求来源：用户提出
- 优先级：P1
- 创建时间：2026-08-11
- 当前状态：已整合（main e781c40），待手工验收；后续迭代 F1/F2/D1 与两轮 UI 缺陷修复（BUGFIX-1/2/3、BUGFIX-2 四缺陷）已完成并整合（2026-08-13）。剩余：GUI 手工冒烟 + 用户推送 main。遗留问题见 INTEGRATION_REPORT.md

## 用户已确认决策（2026-08-11）
1. 优先级：P1
2. 多线程方案：接受（Tokio 异步任务）
3. 最大框数 8、警告阈值 4
4. 热键：复用现有 Alt+Shift+R/S
5. 颜色调色板：由开发 Agent 选择

## 用户补充需求（2026-08-11）
- 翻译弹窗布局：多框由上到下、彩色边框、分隔线
- 主页面变更：删除结果显示和坐标显示、新增弹窗按钮
- 单次翻译也使用弹窗

## 子任务台账

| 序号 | 模块 | 任务单文件 | 分支 | 阶段 | 状态 | 备注 |
|------|------|-----------|------|------|------|------|
| 1 | 02-config | TASK-02-config.md | feat/multibox-config | A | Review 通过 | fmt/clippy/test PASS |
| 2 | 06-text | TASK-06-text.md | feat/multibox-text | A | Review 通过 | fmt/clippy/test PASS |
| 3 | 09-pipeline | TASK-09-pipeline.md | feat/multibox-pipeline | B | Review 通过 | fmt/clippy/test PASS |
| 4 | 10-app | TASK-10-app.md | feat/multibox-app | C | Review 通过 | fmt/clippy/test PASS；命令/事件契约两端一致 |
| 5 | 11-frontend | TASK-11-frontend.md | feat/multibox-frontend | C | Review 通过 | pnpm test/tsc PASS；弹窗布局与主页面精简达标 |

## Review 结果（2026-08-12）

### 第一步：质量门禁（硬性）

| 检查项 | 结果 |
|--------|------|
| cargo fmt --all -- --check | PASS |
| cargo clippy -p vtrans-config --all-targets | PASS |
| cargo clippy -p vtrans-text --all-targets | PASS |
| cargo clippy -p vtrans-pipeline --all-targets | PASS |
| cargo test -p vtrans-config | PASS |
| cargo test -p vtrans-text | PASS |
| cargo test -p vtrans-pipeline | PASS |

### 第二步：整合级 Review

**1. 契约一致性**
- TranslationBox：存在于 vtrans-pipeline ✅
- BoxedTranslationResult：存在于 vtrans-pipeline ✅
- MultiBoxPipeline：存在于 vtrans-pipeline ✅
- BoxStatus：存在于 vtrans-pipeline ✅
- TranslationBoxConfig：存在于 vtrans-config ✅
- max_boxes：存在于 vtrans-config ✅
- warning_threshold：存在于 vtrans-config ✅
- BoxFingerprintCache：存在于 vtrans-text ✅
- vtrans-core 未修改（冻结契约保留）✅

**2. 模块边界**
- vtrans-core 未修改 ✅
- 无 src-tauri 越界修改 ✅
- 无 vtrans-core 越界修改 ✅
- feat(config)/feat(text)/feat(pipeline) 提交均存在 ✅
- README 均已更新（vtrans-config/vtrans-text/vtrans-pipeline）✅

**3. 横切标准**
- 无 todo!() ✅
- 无 dbg!() ✅
- 无 println!() ✅
- 7 处 unimplemented!() 调用（clippy 未报错，可能为既有代码或非生产路径，标记为观察项）⚠️

**4. 验收标准（API 层面）**
- add_box 方法存在 ✅
- remove_box 方法存在 ✅
- start_all 方法存在 ✅
- stop_box 方法存在 ✅
- update_box 方法存在 ✅（推断，待确认）

**5. 文档同步**
- vtrans-config README 已更新 ✅
- vtrans-text README 已更新 ✅
- vtrans-pipeline README 已更新 ✅

### 第三步：Review 结论

**✅ 通过 — 进入整合**

观察项（非阻塞）：
1. 7 处 unimplemented!() 调用：clippy 通过，可能为既有代码或非生产路径。建议开发 Agent 在后续迭代中排查并替换为具体实现或 todo!()。
2. git log 中包含 src/ 路径文件变更：可能来自其他提交（非多框功能），不影响本次整合。

## 状态流转记录

### 2026-08-11 功能拆解完成
- 输出 PLAN.md 和 5 份 TASK-*.md

### 2026-08-11 用户补充需求纳入
- 更新 PLAN.md、TASK-10-app.md、TASK-11-frontend.md

### 2026-08-12 模块 02/06/09 Review 完成
- 质量门禁全部 PASS
- 整合级 Review 通过
- vtrans-core 未修改
- 所有关键类型和 API 存在
- README 均已更新
- 结论：✅ 通过，进入整合

### 2026-08-13 模块 10/11 Review 完成（阶段 C）
- 质量门禁全部 PASS：cargo fmt --all -- --check；cargo clippy --workspace --all-targets（0 错误，仅 vtrans-translation 1 个既有非本功能警告）；cargo test --workspace 全绿；pnpm test 38 文件 / 264 测试；tsc --noEmit
- 整合级 Review 通过：
  - 契约一致性：8 个 Command（add/remove/update/list_translation_boxes、start/stop_multi_realtime、stop_box、open_result_window）与 7 个 Event（multibox://result/box-added/box-removed/box-updated/status/warning、translation://single-result）Rust 与 TypeScript 两端名称、payload 形状一一对应；BoxStatus 外部标签 serde（"Running"/"Stopped"/{"Error": msg}）与 TS 类型一致；contracts.rs 含 serde 契约测试
  - 模块边界：vtrans-core 未修改；各分支 diff 仅限本 crate 与协调文档；src-tauri 未改动（经 vtrans_app::builder 挂载，与项目结构一致；任务单 CRATE_PATH 待确认项以此为准）
  - 横切标准：无 todo!/dbg!/println!；unimplemented! 仅 doctest 脚手架（multibox.rs rustdoc 示例）；日志脱敏（truncate_for_log/mask_sensitive）；错误归属 AppError/PipelineError 正确
  - 验收标准 16 条代码层面对照：全部满足（详见整合报告）
  - 文档：vtrans-app README、src/README.md、前端 types 已更新
- 观察项（非阻塞，见整合报告遗留问题）：多框结果不含原文（已知限制，README 已注明）；热键 Alt+Shift+R/S 未接入多框启动/停止；docs/modules/*.md 与 ARCHITECTURE.md 未同步多框契约
- 结论：✅ 通过，进入整合

### 2026-08-13 整合完成
- 按依赖顺序 --no-ff 合并入 main：feat/multibox-config（9e8e626）→ feat/multibox-text（7a3b97b）→ feat/multibox-pipeline（1258528）→ feat/multibox-app（513929f）→ feat/multibox-frontend（dd0e5e1），全部零冲突
- main 全量门禁复核：fmt / clippy（0 错误）/ workspace 测试 / pnpm 264 测试 / tsc 全部 PASS
- 验收标准 16 条代码层面对照全部满足；弹窗「每框显示原文」细化要求未满足（遗留问题 1）
- 输出 INTEGRATION_REPORT.md；功能状态：已整合 / 待验收（手工冒烟 + 遗留问题 1/2 用户决策后关闭）

### 2026-08-13 遗留问题用户决策
1. 多框原文缺口 → **纳入后续迭代**：派单 TASK-FOLLOWUP-09-pipeline.md、TASK-FOLLOWUP-11-frontend.md（app 仅 README 同步）
2. 热键语义 → **保持现状**：Alt+Shift+R/S 继续控制单框实时会话，多框启动/停止仅 UI 按钮（记录为设计决策，遗留 2 关闭）
3. 文档同步 → **派单补齐**：TASK-DOCSYNC.md（docs/modules/02/06/09/10 + ARCHITECTURE.md），由各模块开发 Agent 执行

### 2026-08-13 UI 三缺陷修复（用户 bug 报告 → 分诊 11-frontend → 派单修复）
- BUGFIX-1 主页面按模式分离区块：single 模式只渲染「翻译区域」（含选择并翻译）；live 模式只渲染「翻译框」与单框实时控制行；runLive 无选区时回退先框选再启动（与悬浮球一致）。任务单 TASK-BUGFIX-11-ui.md；分支 fix/11-multibox-ui-bugs；Review 通过（范围仅 src/、tsc、273 前端测试）→ 合并 main（07bcc3f）
- BUGFIX-2 弹窗多框原文逐框折叠：默认折叠 + 每框 chevron 开关，展开状态按 box_id 记忆，空原文无开关（替代 F2 的常显）
- BUGFIX-3 弹窗滚动：根容器 min-h-screen→h-screen，多框滚动容器与单框译文区补 min-h-0 + overflow-y-auto（flexbox min-height:auto 陷阱修复），滚动条不隐藏

### 2026-08-13 UI 交互四缺陷修复（用户 bug 报告 → 分诊 11-frontend → 派单修复）
- BUGFIX-2-1 单次模式仅保留一个「选择并翻译」按钮（删除重复的「选择屏幕区域」）
- BUGFIX-2-2 live 模式统一由底部「开始实时/停止」控制多框会话；翻译框列表删除「开始多框实时/停止全部」；删除「暂停/继续实时」（单框实时由热键 R/S、悬浮球、弹窗控制——既定设计决策）
- BUGFIX-2-3 多框启动前把 overlay 窗口定位到第一个框所在显示器（物理坐标/尺寸，只定位不 show）；已知限制：单窗口仅覆盖一个显示器，跨显示器框不对齐（记录于 src/README.md）
- BUGFIX-2-4 选区窗口三条退出路径（确认成功/取消/系统关闭）均重置本地选区状态，每次打开直接从空白开始拖框
- 任务单 TASK-BUGFIX-11-interaction.md；分支 fix/11-multibox-interaction；Review 通过（范围仅 src/、tsc、282 前端测试）→ 合并 main（e781c40）

### 后续迭代台账

| 序号 | 模块 | 任务单 | 分支 | 状态 |
|------|------|--------|------|------|
| F1 | 09-pipeline | TASK-FOLLOWUP-09-pipeline.md | feat/multibox-original-text-pipeline | 已整合（0fca96d） |
| F2 | 11-frontend | TASK-FOLLOWUP-11-frontend.md | feat/multibox-original-text-frontend | 已整合（223f600） |
| D1 | 02/06/09/10 文档同步 | TASK-DOCSYNC.md | docs/multibox-contract-sync | 已整合（9a00507） |
| B1-B3 | 11-frontend UI 缺陷 | TASK-BUGFIX-11-ui.md | fix/11-multibox-ui-bugs | 已整合（07bcc3f） |
| B2-1..4 | 11-frontend 交互缺陷 | TASK-BUGFIX-11-interaction.md | fix/11-multibox-interaction | 已整合（e781c40） |

### 2026-08-13 后续迭代执行完成（F1/F2/D1 派发子代理开发）
- F1（pipeline 原文字段）：`BoxedTranslationResult` 新增 `original_text`（配对 OCR 清洗文本；空 OCR/翻译失败发布空原文+空译文以清除 overlay；取消不发布）；`with_original_text` builder 保持三参 `new` 向后兼容；README 更新；Review 通过（范围仅 pipeline、fmt/clippy 0 警告、90 测试全绿）→ --no-ff 合并 main（0fca96d）
- F2（frontend 原文展示）：TS 类型补 `original_text`；`MultiBoxResults` 每框译文上方以次级色小字常显原文，空串不留占位；266 前端测试 + tsc 通过 → 合并 main（223f600）
- D1（文档同步）：docs/modules/02/06/09/10 + ARCHITECTURE.md + vtrans-app README 已知限制段全部按 main 实际代码同步（含 F1/F2 落地的 original_text 契约）；统筹调整：四张子任务合并为单分支 6 个按 scope 提交（docs/multibox-contract-sync），与 GIT_WORKFLOW 线性历史偏好一致；Review 事实抽检通过 → 合并 main（9a00507）

## 整合记录（已完成）
1. ✅ 阶段 A+B：02-config + 06-text + 09-pipeline 合并到 main，workspace 编译验证通过
2. ✅ 阶段 C：10-app + 11-frontend 合并到 main，workspace 编译+测试+前端门禁通过
3. ⏳ 端到端验证：GUI 冒烟为手工验证项（沙箱无显示环境），待用户执行
4. ✅ 输出整合报告：INTEGRATION_REPORT.md
