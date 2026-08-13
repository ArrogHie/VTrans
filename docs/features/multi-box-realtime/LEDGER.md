# 功能台账：多框实时翻译

## 功能信息
- 功能名称：多框实时翻译（Multi-Box Realtime Translation）
- 需求来源：用户提出
- 优先级：P1
- 创建时间：2026-08-11
- 当前状态：已拆解（待分配开发）

## 用户已确认决策（2026-08-11）
1. 优先级：P1（统筹 Agent 设定，用户认可）
2. 多线程方案：接受（Tokio 异步任务，每框独立 task）
3. 最大框数 8、警告阈值 4（统筹 Agent 设定，用户认可）
4. 热键：复用现有 Alt+Shift+R/S（用户认可）
5. 颜色调色板：由开发 Agent 选择（用户认可）

## 用户补充需求（2026-08-11）
- 翻译弹窗布局：多框结果由上到下依次排列，框间分隔线隔开，每框内容用同色边框包含
- 主页面变更：删除翻译结果显示、删除坐标/大小/形状显示、新增「打开翻译弹窗」按钮（已存在则置顶）
- 单次翻译也使用翻译弹窗展示结果（不在主页面显示）
- 以上变更同时适用于单次翻译和实时翻译

## 子任务台账

| 序号 | 模块 | 任务单文件 | 分支 | 阶段 | 状态 | 备注 |
|------|------|-----------|------|------|------|------|
| 1 | 02-config | TASK-02-config.md | feat/multibox-config | A | 待分配 | 层级 1，与 06 并行 |
| 2 | 06-text | TASK-06-text.md | feat/multibox-text | A | 待分配 | 层级 1，条件性任务 |
| 3 | 09-pipeline | TASK-09-pipeline.md | feat/multibox-pipeline | B | 待分配 | 主任务，依赖 1+2 |
| 4 | 10-app | TASK-10-app.md | feat/multibox-app | C | 待分配 | 依赖 3，含弹窗/主页面变更 |
| 5 | 11-frontend | TASK-11-frontend.md | feat/multibox-frontend | C | 待分配 | 依赖 4 IPC 契约，含弹窗布局/主页面变更 |

## 状态流转记录

### 2026-08-11 功能拆解完成
- 完成需求澄清、影响面分析、契约影响分析、任务拆解与排序
- 输出功能开发计划（PLAN.md）和 5 份模块开发说明（TASK-*.md）
- 冻结契约：不涉及（TranslationBox 定义在 pipeline 层，不修改 vtrans-core）
- IPC 契约：新增 8 个 Commands（含 open_result_window）+ 7 个 Events（含 translation://single-result）+ 2 个 TypeScript 类型
- 多线程决策：Tokio 异步任务（用户已确认接受）
- 用户已确认全部决策

### 2026-08-11 用户补充需求纳入
- 翻译弹窗布局更新：多框由上到下、彩色边框、分隔线
- 主页面变更：删除结果显示和坐标显示、新增弹窗按钮
- 单次翻译也使用弹窗
- 已更新 PLAN.md、TASK-10-app.md、TASK-11-frontend.md

## 整合计划（待执行）
1. 阶段 A：02-config + 06-text 合并到 main，验证 workspace 编译
2. 阶段 B：09-pipeline 合并到 main，验证 workspace 编译+测试
3. 阶段 C：10-app + 11-frontend 合并到 main，验证 workspace 编译+测试+前端
4. 端到端验证：cargo tauri dev 下多框实时翻译功能可用
5. 输出整合报告

## 文档清单
- PLAN.md：功能开发计划（含验收标准、模块顺序、契约变更、多线程决策、主页面与弹窗设计）
- TASK-02-config.md：配置管理任务单
- TASK-06-text.md：文本去重任务单（条件性）
- TASK-09-pipeline.md：流水线任务单（主任务）
- TASK-10-app.md：应用层任务单（含弹窗管理、主页面变更）
- TASK-11-frontend.md：前端任务单（含弹窗布局、主页面精简）
- LEDGER.md：本台账
