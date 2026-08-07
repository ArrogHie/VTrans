## 模块开发说明：02 vtrans-config — 翻译模型升级 / 语言统一 增量

### AGENT_DEV_PROMPT 参数

- MODULE_NUMBER: 02
- MODULE_NAME: vtrans-config
- MODULE_SLUG: config
- CRATE_PATH: crates/vtrans-config
- SCOPE: config
- BRANCH_NAME: feat/02-new-translate-model

### 功能上下文

- 功能目标：本地翻译模型升级为 en→zh + ja→zh 双引擎（见 `docs/feature-plans/new-translate-model/PLAN.md`）；OCR 语言与翻译源语言强制统一
- 本模块承担的部分：配置 schema 扩展（翻译质量档位）、版本迁移 v3→v4、跨字段一致性校验（OCR 语言 == 源语言）
- 上游已提供：无（02 是层级 1，仅依赖 vtrans-core）

### 任务要求

- 范围：仅限本模块（`crates/vtrans-config`）；禁止修改其他 crate；禁止修改 vtrans-core；禁止修改 workspace 根 Cargo.toml
- 新增公开 API / 字段（约束性定义）：
  - `TranslationConfig` 新增 `quality: String`（serde default = `"fast"`），合法值 `"fast" | "balanced"`；非法值在 `validation.rs` 返回 `ConfigError::Validation`
  - `CURRENT_CONFIG_VERSION` 3 → 4；`migration.rs` 新增 v3→v4 迁移：
    1. `translation.quality` 缺省补 `"fast"`
    2. 强制 `translation.source_language = ocr.language`（以 OCR 语言为权威；解决历史配置两字段不一致）
  - `validation.rs` 新增跨字段规则：`ocr.language != translation.source_language` → `ConfigError::Validation`（错误信息说明两个字段必须一致，提示使用语言联动命令）
- 行为变更：`AppConfig::validate()` 对「OCR 语言与源语言不一致」的配置拒绝保存
- 约束（非实现代码）：
  - 所有新字段必须带 `#[serde(default = ...)]`，保持「缺失字段用默认值填充」的既有约定
  - 迁移必须是幂等的：v4 配置重复 migrate 无副作用；v3→v4 与 v2→v3 迁移链完整
  - 不得改变既有字段的 serde 名称（`source_language`、`ocr.language` 等保持 snake_case）
- 测试要求：
  - v3 配置（含不一致的 `ocr.language`/`source_language`）迁移后两字段一致、`quality == "fast"`
  - `quality` 非法值校验拒绝；`"fast"`/`"balanced"` 接受；序列化往返
  - 跨字段校验：不一致拒绝、一致接受；`AppConfig::default()` 恒通过
  - 既有测试全量回归（`cargo test -p vtrans-config`）
- 文档要求：crate README 公开 API 段补充 `quality` 与迁移说明；`docs/modules/02-config.md` 同步（schema、迁移、校验规则、验收标准勾选）

### 横切标准提醒

- 日志：迁移与校验失败路径 `warn!`/`error!`；不记录用户配置原文（字段名与错误信息即可）
- 错误：复用 `ConfigError::Validation` / `UnsupportedVersion`，不新增变体；`#[from]` 错误链保持
- 测试与风格：核心逻辑覆盖率 > 80%；fmt / clippy 零警告；公开 API 有 rustdoc

### 完成定义（DoD）

- [ ] 质量门禁通过：`cargo fmt --all -- --check`；`cargo clippy -p vtrans-config --all-targets`；`cargo test -p vtrans-config`
- [ ] 验收标准中本模块相关条目全部满足（quality 持久化、迁移一致、跨字段校验）
- [ ] 未修改其他 crate 与 vtrans-core
- [ ] PR 描述含实现说明、测试覆盖、验收 checklist
