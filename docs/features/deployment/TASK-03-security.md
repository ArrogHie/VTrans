# 模块开发说明：03-vtrans-security — 发行部署「凭据本地化」增量

## AGENT_DEV_PROMPT 参数
- MODULE_NUMBER: 03
- MODULE_NAME: vtrans-security
- MODULE_SLUG: security
- CRATE_PATH: crates/vtrans-security
- SCOPE: security
- BRANCH_NAME: feat/03-dpapi-file-store（从 main 拉取）

## 功能上下文
- 功能目标：安装后一切数据只落在安装目录 `{exe}/data/`，API Key 不写 C 盘用户目录与系统凭据管理器。
- 本模块承担的部分（需求 R5，P0 + P1 迁移）：
  - 新增 `DpapiFileStore`，实现既有 `CredentialStore` trait（`src/credential_store.rs:55-90`，先读该文件确认 trait 签名，**不得修改 trait 本身**）。
  - 存储介质：单文件 `data/credentials.bin`（文件路径由**构造参数传入**，本 crate 不假定任何固定路径——`data/` 根由 10-app 决定）。
  - 加密：Windows DPAPI `CryptProtectData`（用户绑定），`CryptUnprotectData` 解密；不落外部目录、不写明文。
  - 迁移函数（P1）：读取旧 `WindowsCredentialStore` 中 `VTrans:` 前缀的全部条目 → 成功写入新 store → 删除旧条目（逐条失败容忍，记录迁移报告）。
- 上游已提供：`CredentialStore` trait 与 `WindowsCredentialStore` 实现（同 crate）；`SecurityError` 枚举（`lib.rs`）。
- 下游消费方：10-app 在 `state.rs` 构造点替换为 `DpapiFileStore` 并调用迁移（其任务单已同步此契约，你无需改动 app）。

## 任务要求
- 范围：仅限 `crates/vtrans-security`。禁止修改其他 crate、禁止修改 vtrans-core。
- 新增公开 API（约束性定义，实现细节自定）：
  - `DpapiFileStore::new(path: &Path) -> Result<Self, SecurityError>`：打开/创建文件（不存在时创建空容器，首次写入再落盘）。
  - `impl CredentialStore for DpapiFileStore`：实现 trait 全部方法（`store`/`load`/`delete`/`list_targets` 或 trait 实际定义的方法集，以 `credential_store.rs` 为准）。
  - `migrate_windows_to_dpapi(new_store: &DpapiFileStore) -> Result<usize, SecurityError>`：迁移旧凭据管理器条目，返回迁移条数；无旧条目返回 `Ok(0)` 不算错误。
- 行为约束（非实现代码）：
  - DPAPI blob 存文件时禁止出现明文 Key；文件损坏/解密失败返回明确 `SecurityError`（可复用现有变体或新增 `DecryptionFailed` 变体——变体属于本 crate 自定错误，允许）。
  - 全部读写加锁/互斥保证并发安全（与 `ConfigManager` 的原子写风格一致：先写临时文件再原子替换）。
  - 日志纪律：任何路径不得记录完整 Key；引用 Key 用 `mask_key`/`mask_sensitive`；错误信息不得包含解密后的内容。
  - 测试不得使用真实 Windows 凭据管理器写入（避免污染用户真实凭据）；Windows 集成测试仅对 DPAPI 文件 store 使用测试临时目录与测试 Key。
- 测试要求（新增，映射需求验收标准 5/6）：
  - 单元：store/load/delete 文件往返（tempdir，mock 或真实 DPAPI 均可，Windows 下用真实 DPAPI）；不存在 target 返回 `Ok(None)`；损坏文件 → 明确错误；list_targets 不泄露 Key。
  - 单元：`migrate_windows_to_dpapi` 用可注入的旧 store（或按现有测试风格 mock）覆盖「有旧条目→迁移并删除」「无旧条目→0」「旧 store 读取失败→容忍并报告」。
  - 单元：日志掩码断言（同模块既有 mask 测试风格）。
- 文档要求：同步 `crates/vtrans-security/README.md`——新增 API 概要、DPAPI 用户绑定限制（用户配置文件重建后凭据不可恢复）、文件路径由调用方传入的说明。
- 提交规范：`feat(security): <一句话描述>`，可多次提交，每次可编译；PR 描述含实现说明、测试覆盖、验收 checklist。

## 横切标准提醒（逐项附带）
- 日志：`#[tracing::instrument]` 标注公开入口；错误路径 `warn!`/`error!`；禁止完整 Key（`mask_key`）。
- 错误：`SecurityError` 本 crate 定义，`#[from]` 保留错误链；不引入 anyhow 到公开 API。
- 测试与风格：`cargo fmt` 零差异、clippy pedantic 零警告；unsafe（DPAPI FFI）必须 `// SAFETY:` 注释并说明安全条件（指针/长度有效性）。
- 依赖：DPAPI 用现有 `windows` crate 特性（如已引入）；如需新增特性/依赖，仅改本 crate Cargo.toml 并在 PR 说明。

## 完成定义（DoD）
- [ ] `cargo fmt --all -- --check`；`cargo clippy -p vtrans-security --all-targets`；`cargo test -p vtrans-security` 全绿
- [ ] 验收标准第 5 条代码层满足：`DpapiFileStore` 落盘文件、旧凭据管理器条目迁移后可删除（迁移函数就绪，删除时机由 app 启动调用）
- [ ] trait 签名未改；未修改其他 crate 与 vtrans-core
- [ ] README 已更新；PR 描述含实现说明、测试覆盖、验收 checklist
