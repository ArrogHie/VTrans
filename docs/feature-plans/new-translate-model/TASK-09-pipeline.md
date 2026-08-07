## 模块开发说明：09 vtrans-pipeline — 翻译模型升级 增量

### AGENT_DEV_PROMPT 参数

- MODULE_NUMBER: 09
- MODULE_NAME: vtrans-pipeline
- MODULE_SLUG: pipeline
- CRATE_PATH: crates/vtrans-pipeline
- SCOPE: pipeline
- BRANCH_NAME: feat/09-new-translate-model

### 功能上下文

- 功能目标：本地翻译升级为双引擎后，`source=auto` 的翻译必须按 OCR 检测结果/Unicode heuristic 解析为具体源语言；长文本分块升级为标点感知（接入指南 §8/§9）
- 本模块承担的部分：OCR 完成后解析翻译源语言（纯函数，可单测）；`translate_text` 分块规则升级；`normalize_result` 保持（继续复用 `detected_language`）
- 上游已提供：`vtrans-core` 契约不变（`OcrResult.detected_language`、`Language`）；07 的 Provider 拒绝 `Auto` 源（本任务保证送入 Provider 的源语言为具体语言）

### 任务要求

- 范围：仅限本模块（`crates/vtrans-pipeline`）；禁止修改其他 crate；禁止修改 vtrans-core
- 新增公开 API（约束性定义，`pub(crate)` 或 `pub` 均可，07/10 不依赖）：
  ```rust
  /// 解析实际翻译源语言：配置为具体语言时原样返回；
  /// 配置为 Auto 时优先用 OCR detected_language（仅 en/ja/zh-CN），
  /// 无检测结果时用 Unicode heuristic 兜底；无法判定时保持 Auto。
  pub fn resolve_translation_source(detected: Option<Language>, configured: Language) -> Language;

  /// Unicode heuristic（指南 §8）：存在平假名/片假名/半角片假名 → Japanese；
  /// 否则以拉丁字母为主 → English；其余 None。
  pub fn heuristic_detect_language(text: &str) -> Option<Language>;
  ```
- 行为变更：
  - `single.rs` / `live.rs`：OCR 完成后，用 `resolve_translation_source(result.detected_language, params.source)` 得到实际 source 用于翻译（`normalize_result` 的日文标点判断同步使用解析后的 source）
  - `translate_text` 分块规则升级：
    - 优先在句子边界切分（`。！？.!?`，日文/英文各自标点集），其次逗号/分号（`，、,;`），最后硬切
    - 字符预算按语言：`ja` 512 字符 / `en` 1024 字符（对齐新模型 `max_input_tokens=256` 的保守估算；常量可配，单测锁定）
    - 保留换行与 `MAX_TRANSLATION_CHUNK_CHARS` 的既有上限语义（作为最终硬切兜底）
- 约束（非实现代码）：
  - 不引入 tokenizer 依赖：分块是字符/标点级近似，token 精确性由 Provider 侧 `max_input_tokens` 截断兜底
  - `resolve_translation_source` 的判定顺序（detected → heuristic → Auto）必须可单测；`zh-CN` 检测结果保持原样（本地 Provider 不支持中文源，由 Provider 返回 `UnsupportedPair`，UI 已提示）
  - 不得改变 Pipeline 事件流与 `PipelineConfig` 形状（无 IPC 契约变化）
- 测试要求：
  - `resolve_translation_source`：具体语言直通；`Auto+Some(en/ja/zh-CN)` 各分支；`Auto+None` → heuristic（日文假名 / 拉丁 / 混合 / 空文本）；heuristic 无法判定 → `Auto`
  - 分块：句子边界优先、逗号兜底、硬切兜底、unicode 标量不拆分、日文预算 512 / 英文预算 1024、既有回归（短文本单块、2000 字符硬切上限）
  - 既有单次/实时集成测试全量回归
- 文档要求：crate README（auto 路由与分块说明）；`docs/modules/09-pipeline.md` 同步（流程图中语言路由步骤、测试计划、验收标准）

### 横切标准提醒

- 日志：解析结果 `debug!`（`source=auto -> resolved=en`）；不记录原文完整内容（`truncate_for_log`）
- 错误：复用 `PipelineError`，无新增变体
- 测试与风格：纯函数覆盖率 > 80%；fmt / clippy 零警告；公开 API rustdoc

### 完成定义（DoD）

- [ ] 质量门禁通过：`cargo fmt --all -- --check`；`cargo clippy -p vtrans-pipeline --all-targets`；`cargo test -p vtrans-pipeline`
- [ ] auto 路由与分块规则单测全绿；既有 pipeline 测试无回归
- [ ] 未修改其他 crate 与 vtrans-core
- [ ] PR 描述含实现说明、测试覆盖、验收 checklist
