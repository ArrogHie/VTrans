# 模块 03：vtrans-security 凭据安全

| 属性 | 值 |
|------|-----|
| Crate | `vtrans-security` |
| 分支 | `feat/03-cloud-credentials` |
| 上游依赖 | `vtrans-core` |
| 层级 | 1 |
| 复杂度 | 中 |
| 阶段 | Phase 1 |

## 职责

通过 Windows Credential Manager 安全存储和读取 API Key 等敏感凭据，支持按云端 Provider（OpenAI / DeepL /
Google / Azure / 百度 / 预留腾讯）独立存取。禁止将凭据写入明文配置文件或日志。

## 公开 API

```rust
/// 凭据管理器
pub struct CredentialManager { /* ... */ }

impl CredentialManager {
    pub fn new() -> Result<Self, SecurityError>;

    /// 存储 API Key，target 用于区分不同 provider
    pub fn store(&self, target: &str, api_key: &str) -> Result<(), SecurityError>;

    /// 读取 API Key
    pub fn load(&self, target: &str) -> Result<Option<String>, SecurityError>;

    /// 删除 API Key
    pub fn delete(&self, target: &str) -> Result<(), SecurityError>;

    /// 列出所有已存储的 target（不返回 Key 本身）
    pub fn list_targets(&self) -> Result<Vec<String>, SecurityError>;

    /// 存储 Provider 凭据（类型化 target，推荐）
    pub fn store_for_provider(
        &self, target: CredentialTarget, api_key: &str,
    ) -> Result<(), SecurityError>;

    /// 读取 Provider 凭据（类型化 target，推荐）
    pub fn load_for_provider(
        &self, target: CredentialTarget,
    ) -> Result<Option<String>, SecurityError>;

    /// 删除 Provider 凭据（类型化 target，推荐）
    pub fn delete_for_provider(
        &self, target: CredentialTarget,
    ) -> Result<(), SecurityError>;
}

/// 日志安全的 Key 展示（掩码处理）
pub fn mask_key(key: &str) -> String;

/// 云端 Provider 凭据目标（唯一事实来源）
pub enum CredentialTarget {
    OpenAI,      // -> "openai"
    DeepL,       // -> "deepl"
    Google,      // -> "google"
    Azure,       // -> "azure"
    BaiduAppId,  // -> "baidu_app_id"（APP ID，非机密但统一走凭据库）
    BaiduSecret, // -> "baidu_secret"（Secret Key）
    Tencent,     // -> "tencent"（预留，本功能不接入）
}

impl CredentialTarget {
    pub const ALL: [Self; 7];                  // 全部目标，稳定顺序
    pub const fn as_str(self) -> &'static str; // 逻辑目标名
}
```

历史字符串 API（`store` / `load` / `delete` / `list_targets`）保持兼容，现有 `translation` 目标不受影响；
新增代码优先使用 `store_for_provider` / `load_for_provider` / `delete_for_provider`。

## 错误类型

```rust
[derive(Debug, thiserror::Error)]
pub enum SecurityError {
    #[error("credential store unavailable: {0}")]
    StoreUnavailable(String),
    #[error("credential not found for target: {0}")]
    NotFound(String),
    #[error("credential operation failed: {0}")]
    OperationFailed(String),
    #[error("windows api error: {0}")]
    WindowsApi(String),
}
```

## 内部文件结构

```text
crates/vtrans-security/
├── Cargo.toml
├── README.md
└── src/
    ├── lib.rs            # re-export
    ├── manager.rs        # CredentialManager 实现
    ├── credential_store.rs # Windows Credential Manager FFI 封装
    ├── mask.rs           # 日志掩码工具
    └── target.rs         # CredentialTarget 枚举（凭据目标唯一事实来源）
```

## 测试计划

| 测试项 | 类型 | 说明 |
|--------|------|------|
| mask_key 掩码 | 单元 | 12 字符 Key 显示为 sk-****1234 格式 |
| mask_key 短 Key | 单元 | 不足 8 字符时全掩码 |
| store/load 往返 | 集成 | 存储后可读取，值一致（需 Windows 环境） |
| delete 后 load | 集成 | 删除后返回 None |
| list_targets | 集成 | 不泄露 Key 值 |
| 不存在的 target load | 集成 | 返回 Ok(None) 而非错误 |
| 多目标存取 | 单元 | 每个 `CredentialTarget` 均可 store/load/delete 往返 |
| 命名空间隔离 | 单元 | 不同 provider 目标互不覆盖；原始 store 中为 `VTrans:<target>` |
| 百度双目标 | 单元 | APP ID 与 Secret 独立存取、独立删除，互不影响 |
| 腾讯预留目标 | 单元 | `tencent` 目标可用（存储/读取通过） |
| 非 UTF-8 blob | 单元 | 经 `load_for_provider` 返回 `OperationFailed` |
| 日志掩码 | 单元 | 捕获 manager 日志，断言只含 `mask_key` 掩码形式、不含原始 Key |

## 验收标准

- [x] API Key 不出现在任何明文文件中
- [x] 日志中只出现掩码后的 Key
- [x] store/load/delete 功能正常
- [x] `store_for_provider` / `load_for_provider` / `delete_for_provider` 功能正常
- [x] 多目标凭据存取通过测试（含命名空间隔离）
- [x] 百度 APP ID + Secret 两个独立目标已实现并测试
- [x] 单元测试通过（掩码逻辑）
- [x] README.md 完整

## 开发注意事项

- 使用 windows crate 的 CredWriteW/CredReadW/CredDeleteW API
- target 前缀统一为 "VTrans:" 避免与其他应用冲突
- CredentialType 使用 CRED_TYPE_GENERIC
- 测试中如无 Windows Credential Manager 可用，mock store 行为
- 禁止在 tracing 日志中输出完整 Key
- 凭据目标命名固定：`openai` / `deepl` / `google` / `azure` / `baidu_app_id` / `baidu_secret` / `tencent`
- 百度 APP ID 与 Secret 分存两个目标，app 层分别读取后由 `BaiduProvider` 组装签名
- provider 固定目标（`openai` 等）不做真实 Windows vault 集成测试，避免覆盖/删除用户真实凭据；
  命名空间映射由内存后端单元测试与通用 API 集成测试覆盖
