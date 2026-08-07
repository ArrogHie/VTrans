## 模块开发说明：07 vtrans-translation — PreprocessParams 新字段测试兼容 增量

### AGENT_DEV_PROMPT 参数

- MODULE_NUMBER: 07
- MODULE_NAME: vtrans-translation
- MODULE_SLUG: translation
- CRATE_PATH: crates/vtrans-translation
- SCOPE: translation（仅测试代码，见任务要求）
- BRANCH_NAME: `fix/07-ppocrv6-params-test`（依赖 08 合并到 main 后从 main 拉分支）

### 功能上下文

- 功能目标：08（vtrans-models）为 PP-OCRv6 扩展了 `PreprocessParams` 具体类型字段（serde default），导致本模块 `#[cfg(test)]` 中 1 处结构体字面量构造缺字段、测试目标无法编译
- 本模块承担：仅修复该测试字面量；运行时代码零改动
- 上游已提供：08 合并后的 `PreprocessParams` 新字段（box_threshold / max_candidates / min_box_size / rec_input_height / rec_input_width / rec_append_space / rec_blank_index）

### 任务要求

- 范围：仅限 `crates/vtrans-translation/src/local_onnx.rs` 约 1094 行的测试中 `PreprocessParams { .. }` 字面量；禁止修改其他文件、其他 crate 与 vtrans-core
- 修复方式：补齐新字段（取 v6 默认值：box_threshold 0.45、max_candidates 3000、min_box_size 3.0、rec_input_height 48、rec_input_width 320、rec_append_space true、rec_blank_index 0），或使用 `..Default::default()`（若该结构实现 Default）；不得改变测试语义
- 测试要求：`cargo test -p vtrans-translation` 全绿
- 提交规范：`fix(translation): complete PreprocessParams literal in test after schema extension`（或等价）

### 横切标准提醒

- 错误 / 日志 / 文档：无运行时行为变化，不涉及
- 测试与风格：fmt / clippy / test 零警告零失败

### 完成定义（DoD）

- [ ] `cargo fmt --all -- --check`；`cargo clippy -p vtrans-translation --all-targets`；`cargo test -p vtrans-translation`
- [ ] 仅修改上述测试字面量 1 处
- [ ] 未修改其他 crate 与 vtrans-core
- [ ] PR 描述含实现说明
