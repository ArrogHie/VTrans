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
- 本模块承担的部分：为指纹去重提供按框隔离的去重 API，使每个翻译框维护独立去重状态
- 上游已提供：vtrans-core 不变

### 任务要求
- 范围：仅限 crates/vtrans-text；禁止修改其他 crate；禁止修改 vtrans-core
- 新增公开 API：
  - `BoxFingerprintCache` struct：按 box_id 隔离的指纹缓存
  - 方法：new()、is_duplicate(box_id, text) -> bool、clear_box(box_id)、clear_all()
  - 可选：remove_box(box_id) 用于删除框时清理对应缓存
- 行为变更：
  - 现有去重 API 保持不变，新增多框版本
  - 每个框维护独立指纹集合，框间不干扰
  - 删除框时清理该框缓存，避免内存泄漏
- 约束：
  - 不修改 vtrans-core 的任何类型
  - 指纹去重算法与现有实现一致（仅隔离状态，不改变算法）
  - 错误归属：使用 TextError（本 crate 定义）
  - BoxFingerprintCache 需 Send + Sync（跨线程场景）
- 测试要求：
  - 多框去重隔离测试：框 A 重复文本不影响框 B
  - 清理测试：删除框后缓存释放
  - 并发安全测试
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
本任务为条件性任务。如 pipeline 开发 Agent 决定在 pipeline 层自行维护 per-box 去重状态，则本任务可省略。建议与 pipeline 开发 Agent 协调。

### 待确认事项
- 现有指纹去重的具体实现（开发 Agent 需阅读 crates/vtrans-text/src/ 确认现有 API）
- 去重算法的具体细节（哈希函数、比较方式等）
