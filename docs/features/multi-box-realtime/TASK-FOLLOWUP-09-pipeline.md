# 模块开发说明：09-pipeline — 多框结果原文增量（后续迭代 F1）

## AGENT_DEV_PROMPT 参数
- MODULE_NUMBER: 09
- MODULE_NAME: vtrans-pipeline
- MODULE_SLUG: pipeline
- CRATE_PATH: crates/vtrans-pipeline
- SCOPE: pipeline
- BRANCH_NAME: feat/multibox-original-text-pipeline（从 main 拉分支，main 已含多框实时翻译整合结果）

## 功能上下文
- 背景：多框实时翻译已整合（2026-08-13）。用户确认：弹窗每框应显示原文+译文，但当前 `BoxedTranslationResult` 仅携带译文（`TranslationResult`），原文缺失。
- 本模块承担的部分：在 `BoxedTranslationResult` 中补充 OCR 原文字段，并在每框任务链路中配对。
- 上游已提供：本 crate 的 `MultiBoxPipeline` / `BoxedTranslationResult`（crates/vtrans-pipeline/src/multibox.rs）；vtrans-core `TranslationRequest`（含 `text` 字段，翻译请求已携带原文）。

## 任务要求
- 范围：仅限 crates/vtrans-pipeline；禁止修改其他 crate；禁止修改 vtrans-core（冻结契约不涉及——本增量仅改本 crate 自有类型）。
- 新增公开 API（约束性定义，非实现代码）：
  - `BoxedTranslationResult` 增加字段 `original_text: String`（serde 字段名 `original_text`），保留现有字段 `box_id`/`color`/`result`/`timestamp` 不变（向后兼容：前端按需读取新字段）。
- 行为变更：
  - 每框任务在 OCR 产出文本后、发起翻译前，将 OCR 原文与最终 `BoxedTranslationResult` 配对（OCR 文本来源为该框 OCR 结果，如 `merged_text` 或等价文本，具体以 crate 现有链路为准，由开发 Agent 对照源码确定）。
  - 翻译失败或无 OCR 文本时的降级：`original_text` 为空字符串，不阻塞结果发布。
- 约束：
  - 不得把图像或像素数据放入结果（只放文本）。
  - 日志红线：不记录完整原文/译文，引用用 `truncate_for_log`。
  - 错误归属不变：仍为 `PipelineError`。
- 测试要求（补充现有 tests/pipeline_multibox.rs）：
  - 结果携带原文：mock provider 下断言 `original_text` 与 OCR 输入一致。
  - 翻译失败/OCR 空文本时 `original_text` 为空且结果仍发布。
  - serde：`original_text` 字段名与 JSON 表示断言。
  - 回归：既有 909 行多框测试全部保持通过。
- 文档要求：更新本 crate README 的 `BoxedTranslationResult` 字段说明；移除/更新「多框结果不含原文」相关已知限制段落（vtrans-app README 的限制段由 F2 同步处理，本任务不越界）。
- 提交规范：`feat(pipeline): add original text to boxed multi-box results`，可多次提交，每次可编译。

## 横切标准提醒
- 日志：tracing；原文/译文日志用 truncate_for_log。
- 错误：thiserror / PipelineError 归属不变；无新增错误变体需求（如需，先与统筹确认）。
- 测试与风格：cargo fmt --all -- --check；cargo clippy -p vtrans-pipeline --all-targets；cargo test -p vtrans-pipeline；无 todo!()/dbg!()。

## 完成定义（DoD）
- [ ] 质量门禁通过：fmt / clippy -p vtrans-pipeline / test -p vtrans-pipeline
- [ ] `BoxedTranslationResult.original_text` 存在且 serde 字段名为 `original_text`
- [ ] 新增测试覆盖配对、降级、serde 三种情形
- [ ] 未修改其他 crate 与 vtrans-core
- [ ] PR 描述含契约变更说明（新字段、向后兼容性）与测试覆盖
