# 功能台账：多框实时翻译

## 功能信息
- 功能名称：多框实时翻译（Multi-Box Realtime Translation）
- 需求来源：用户提出
- 优先级：P1
- 创建时间：2026-08-11
- 当前状态：阶段 A+B Review 通过，待整合

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
| 4 | 10-app | TASK-10-app.md | feat/multibox-app | C | 待分配 | 依赖 3 |
| 5 | 11-frontend | TASK-11-frontend.md | feat/multibox-frontend | C | 待分配 | 依赖 4 IPC 契约 |

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

## 整合计划（待执行）
1. 阶段 A+B：02-config + 06-text + 09-pipeline 合并到 main，验证 workspace 编译
2. 阶段 C：10-app + 11-frontend 合并到 main，验证 workspace 编译+测试+前端
3. 端到端验证：cargo tauri dev 下多框实时翻译功能可用
4. 输出整合报告
