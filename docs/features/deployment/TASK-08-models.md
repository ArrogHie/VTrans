# 模块开发说明：08-vtrans-models — 发行部署「manifest 可选条目」增量

## AGENT_DEV_PROMPT 参数
- MODULE_NUMBER: 08
- MODULE_NAME: vtrans-models
- MODULE_SLUG: models
- CRATE_PATH: crates/vtrans-models
- SCOPE: models
- BRANCH_NAME: feat/08-manifest-optional-entries（从 main 拉取）

## 功能上下文
- 功能目标：OCR 模型随安装包内置（必选），403MB 翻译模型不进包、由设置页下载（可选条目），缺失不视为损坏。
- 本模块承担的部分（需求 R3，P0）：
  - `ModelEntry` 新增可选下载元数据字段；`verify_integrity` 对 optional 且缺失的条目记 **skipped** 而非 **failed**；`verify_models.rs` CLI 同语义。
  - 提供下载地址/大小的 schema 载体（`download_url` / `download_size_bytes`），供 10-app 的下载命令读取。
- 上游已提供：`vtrans-core`（冻结，不改）；本 crate 现有 `ModelEntry`/`VerifyReport`/`verify_entry`（`src/verify.rs`，供 10-app 复用，签名不得破坏）。
- 下游消费方：10-app（`ensure_data_models` 复用 `verify_entry`；下载命令读 manifest 的 URL/size；`get_model_status` 复用 `verify_integrity` 的 skipped 语义）。其任务单已按以下签名定义同步，你不得擅自改签名：
  - `ModelEntry` 字段（建议形状，serde 默认值保证旧 manifest 可反序列化）：
    ```rust
    #[serde(default)] pub optional: bool,                      // 默认 false
    #[serde(default)] pub download_url: Option<String>,        // 默认 None
    #[serde(default)] pub download_size_bytes: Option<u64>,    // 默认 None
    ```
  - `VerifyReport` 新增 `skipped: Vec<String>`（条目 id 列表），**保留既有字段**（`checked`/`passed`/`failed`）避免破坏现有调用方。

## 任务要求
- 范围：仅限 `crates/vtrans-models`。禁止修改其他 crate、禁止修改 vtrans-core。
- 行为变更：
  - `manager.rs` `verify_integrity`（现状 `manager.rs:98-138`）：条目缺失且 `optional == true` → 记入 `skipped`，不计入 `failed`，整体 `Ok`；optional 条目**存在但** sha256 不符 → 仍记 `failed`（损坏必须报出）；非 optional 缺失 → 维持现状 `failed`。
  - `src/bin/verify_models.rs` CLI：同语义——optional 缺失不算失败（退出码成功或中性提示），输出区分 skipped/failed；仍识别 `VTRANS_MODEL_DIR`（现状 `verify_models.rs:97` 已实现，保持不变）。
  - manifest 模板 `crates/vtrans-models/resources/manifest.json` 同步示例（translation.model 条目含 `optional: true` 与占位 `download_url`/`download_size_bytes`，值以 10-app 维护的运行时 manifest 为准）。
- 约束（非实现代码）：
  - 旧 manifest（无新字段）必须仍可反序列化（serde default 兜底），schema version 不变（仍是 1）——参照现有 PP-OCRv6 向后兼容章节的做法。
  - 不得在 verify 热路径引入网络访问；本 crate 不实现下载逻辑（下载在 10-app）。
- 测试要求（新增，映射需求验收标准 6）：
  - 单元：optional 缺失 → skipped、整体 Ok；optional 存在但哈希不符 → failed；非 optional 缺失 → failed；旧 manifest（无新字段）反序列化 → 字段为默认值。
  - 单元/集成：`VerifyReport` 序列化形状（含 skipped）往返；`verify_entry` 既有行为不回归。
- 文档要求：同步 `crates/vtrans-models/README.md`（新字段、skipped 语义、下载元数据由 app 消费的说明）。
- 提交规范：`feat(models): <一句话描述>`，可多次提交，每次可编译；PR 描述含实现说明、测试覆盖、验收 checklist。

## 横切标准提醒（逐项附带）
- 日志：`#[tracing::instrument]`；skipped 条目 `debug!`/`info!` 记录 id（不含敏感数据）；错误路径 `warn!`/`error!`。
- 错误：`ModelError` 本 crate 定义，新增变体仅限必要（如无新错误场景可不加）；`#[from]` 保留错误链。
- 测试与风格：fmt/clippy pedantic 零警告；公开 API rustdoc 含 `# Example`。

## 完成定义（DoD）
- [ ] `cargo fmt --all -- --check`；`cargo clippy -p vtrans-models --all-targets`；`cargo test -p vtrans-models` 全绿
- [ ] optional 语义单测覆盖完整（缺失 skipped / 存在损坏 failed / 旧 manifest 兼容）
- [ ] 既有 `verify_entry`/`VerifyReport` 调用方签名兼容（字段只增不改）
- [ ] 未修改其他 crate 与 vtrans-core；README 已更新；PR 描述含验收 checklist
