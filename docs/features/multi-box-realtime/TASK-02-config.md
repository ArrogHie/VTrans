## 模块开发说明：02-config — 多框实时翻译增量

### AGENT_DEV_PROMPT 参数
- MODULE_NUMBER: 02
- MODULE_NAME: vtrans-config
- MODULE_SLUG: config
- CRATE_PATH: crates/vtrans-config
- SCOPE: config
- BRANCH_NAME: feat/multibox-config

### 功能上下文
- 功能目标：支持多框实时翻译的配置管理
- 本模块承担的部分：AppConfig 新增多框配置字段，支持翻译框列表的持久化、迁移、默认值
- 上游已提供：vtrans-core 的 ScreenRegion（不变，字段 x/y/width/height）

### 任务要求
- 范围：仅限 crates/vtrans-config；禁止修改其他 crate；禁止修改 vtrans-core
- 新增公开 API：
  - `TranslationBoxConfig` struct：含 `id: u32`、`region: ScreenRegion`、`color: String`（hex 格式）
  - `MultiBoxConfig` struct（或直接在 AppConfig 上加字段）：含 `boxes: Vec<TranslationBoxConfig>`、`max_boxes: u32`、`warning_threshold: u32`
  - AppConfig 新增字段 `translation_boxes: Vec<TranslationBoxConfig>`、`max_boxes: u32`（默认 8）、`warning_threshold: u32`（默认 4）
- 行为变更：
  - 现有单框实时翻译的配置字段（如有）保持兼容，不破坏
  - 新增多框配置字段有合理默认值（空列表、max_boxes=8、warning_threshold=4）
  - 配置迁移：从无 translation_boxes 字段的旧版本加载时，默认为空列表
  - 颜色分配策略：提供默认颜色调色板（至少 8 种），新增框时自动分配下一个可用颜色
- 约束：
  - `TranslationBoxConfig` 必须实现 `Serialize`/`Deserialize`（用于持久化和 IPC）
  - `region` 使用 vtrans-core 的 `ScreenRegion`，不重复定义
  - `color` 用 String 存储 hex 色值（如 "#FF6B6B"），便于前端直接使用
  - 错误归属：使用 `ConfigError`（本 crate 定义），不引入 core 错误
- 测试要求：
  - 配置序列化/反序列化往返测试
  - 迁移测试：从无多框字段的旧配置加载
  - 颜色分配测试：分配不重复、超过调色板大小后循环
  - max_boxes 限制校验测试
- 文档要求：API 变化同步本 crate README
- 提交规范：`feat(config): add multi-box translation config fields`，可多次提交

### 横切标准提醒
- 日志：使用 tracing；翻译框配置变更时记录 info 级日志；不记录完整区域内容（仅记录 box_id 和颜色）
- 错误：使用 thiserror；错误归属 ConfigError；`#[from]` 错误链
- 测试与风格：fmt/clippy 通过；rustdoc 注释公开 API

### 完成定义（DoD）
- [ ] cargo fmt --all -- --check 通过
- [ ] cargo clippy -p vtrans-config --all-targets 通过
- [ ] cargo test -p vtrans-config 通过
- [ ] 未修改其他 crate 与 vtrans-core
- [ ] PR 描述含实现说明、测试覆盖、验收 checklist

### 待确认事项
- AppConfig 现有字段结构（开发 Agent 需阅读 crates/vtrans-config/src/ 确认现有 schema）
- 现有是否有单框 region 配置字段（探测未找到 region/screen_region/realtime_region 字段名，需确认实际字段名）
- 颜色调色板的具体值（建议 8 种高对比度颜色，开发 Agent 可调整）

*** Add File: D:\~~~rust\VTrans\docs\features\multi-box-realtime\TASK-06-text.md
## 模块开发说明：06-text — 多框实时翻译增量（条件性）

### AGENT_DEV_PROMPT 参数
- MODULE_NUMBER: 06
- MODULE_NAME: vtrans-text
- MODULE_SLUG: text
- CRATE_PATH: crates/vtrans-text
- SCOPE: text
- BRANCH_NAME: feat/multibox-text

### 功能上下文
- 功能目标：支持多框场景下的文本去重
- 本模块承担的部分：为指纹去重提供按框隔离的去重 API（如 `BoxFingerprintCache` 或类似），使每个翻译框维护独立的去重状态
- 上游已提供：vtrans-core 不变

### 任务要求
- 范围：仅限 crates/vtrans-text；禁止修改其他 crate；禁止修改 vtrans-core
- 新增公开 API：
  - `BoxFingerprintCache`（或类似名称）struct：按 `box_id: u32` 隔离的指纹缓存
  - 方法：`new()`、`is_duplicate(box_id, text) -> bool`、`clear_box(box_id)`、`clear_all()`
  - 可选：`remove_box(box_id)` 用于删除框时清理对应缓存
- 行为变更：
  - 现有去重 API（如果有单框版本）保持不变，新增多框版本
  - 每个框维护独立的指纹集合，框之间不干扰
  - 删除框时清理该框的缓存，避免内存泄漏
- 约束：
  - 不修改 vtrans-core 的任何类型
  - 指纹去重算法与现有实现一致（不改变去重逻辑，仅隔离状态）
  - 错误归属：使用 `TextError`（本 crate 定义）
  - `BoxFingerprintCache` 需 `Send + Sync`（用于跨线程场景）
- 测试要求：
  - 多框去重隔离测试：框 A 的重复文本不影响框 B
  - 清理测试：删除框后缓存释放
  - 并发安全测试（如使用 std::sync::Mutex 或 parking_lot::Mutex）
- 文档要求：API 变化同步本 crate README
- 提交规范：`feat(text): add per-box fingerprint dedup API`

### 横切标准提醒
- 日志：使用 tracing；去重命中时 debug 级日志（记录 box_id，不记录完整文本）
- 错误：使用 thiserror；错误归属 TextError
- 测试与风格：fmt/clippy 通过；并发安全测试

### 完成定义（DoD）
- [ ] cargo fmt --all -- --check 通过
- [ ] cargo clippy -p vtrans-text --all-targets 通过
- [ ] cargo test -p vtrans-text 通过
- [ ] 未修改其他 crate 与 vtrans-core
- [ ] PR 描述含实现说明、测试覆盖

### 条件性说明
本任务为条件性任务。如果 pipeline 开发 Agent 决定在 pipeline 层自行维护 per-box 去重状态（使用 HashMap<u32, FingerprintCache>），则本任务可省略。建议与 pipeline 开发 Agent 协调，避免重复实现。

### 待确认事项
- 现有指纹去重的具体实现（开发 Agent 需阅读 crates/vtrans-text/src/ 确认现有 API）
- 去重算法的具体细节（哈希函数、比较方式等）
