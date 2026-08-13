# 模块开发说明：11-frontend — 多框结果原文展示增量（后续迭代 F2）

## AGENT_DEV_PROMPT 参数
- MODULE_NUMBER: 11
- MODULE_NAME: frontend
- MODULE_SLUG: frontend
- CRATE_PATH: src
- SCOPE: frontend
- BRANCH_NAME: feat/multibox-original-text-frontend（依赖 F1：pipeline 的 original_text 字段合并入 main 后，从 main 拉分支）

## 功能上下文
- 背景：多框实时翻译已整合。用户确认弹窗每框应显示「原文、译文、框编号/颜色标识」；当前多框区域仅显示译文。
- 本模块承担的部分：在翻译弹窗多框区域（`src/components/MultiBoxResults.tsx`）展示每框原文；同步类型与测试。
- 上游已提供：pipeline 的 `BoxedTranslationResult.original_text: String`（F1 完成后合并入 main）。

## 任务要求
- 范围：仅限 src/（前端代码）；禁止修改 Rust crate。
- 类型同步：
  - `src/types/index.ts` 的 `BoxedTranslationResult` 增加 `original_text: string`。
- 组件变更：
  - `MultiBoxResults` 每框区域内显示原文：原文与译文同框（如原文小字/次级色显示在译文上方，或可折叠），排版与现有弹窗风格一致（参考单框布局的「原文」折叠样式）。
  - 原文为空时不渲染原文区域（避免空占位）。
  - 框编号、颜色标识、状态徽章保持现状。
- 约束：
  - 不改动 IPC 命令/事件名称；`multibox://result` payload 仅新增字段。
  - 事件监听清理（unlisten）与错误处理遵循现有组件惯例。
  - 不在 UI 文本中暴露敏感信息。
- 测试要求（补充 src/test/）：
  - `MultiBoxResults`：有原文时显示、空原文不渲染、布局含分隔线与彩色边框回归。
  - 类型/契约测试同步 `original_text` 字段。
  - 既有 264 个前端测试保持通过。
- 文档要求：`src/README.md` 同步多框弹窗展示说明；与 F1 协作更新 vtrans-app README 的「多框结果不含原文」已知限制段（该段位于 crates/vtrans-app/README.md，属文档同步，不涉及 Rust 代码；如越界争议，交回统筹协调）。
- 提交规范：`feat(frontend): show original text per multi-box result`，可多次提交。

## 横切标准提醒
- 错误处理：invoke/事件失败显示 toast，不崩溃。
- 测试与风格：pnpm test；pnpm exec tsc --noEmit；无 console.log 残留。

## 完成定义（DoD）
- [ ] pnpm test 与 pnpm exec tsc --noEmit 通过
- [ ] 弹窗每框显示原文+译文（原文为空时不占位）
- [ ] 未修改 Rust crate
- [ ] PR 描述含组件变更说明与测试覆盖
