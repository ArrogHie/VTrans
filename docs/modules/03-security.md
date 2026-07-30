# 模块 03：vtrans-security 凭据安全

| 属性 | 值 |
|------|-----|
| Crate | `vtrans-security` |
| 分支 | `feat/03-security` |
| 上游依赖 | `vtrans-core` |
| 层级 | 1 |
| 复杂度 | 中 |
| 阶段 | Phase 1 |

## 职责

通过 Windows Credential Manager 安全存储和读取 API Key 等敏感凭据。禁止将凭据写入明文配置文件或日志。

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
}

/// 日志安全的 Key 展示（掩码处理）
pub fn mask_key(key: &str) -> String;
```

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
    └── mask.rs           # 日志掩码工具
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

## 验收标准

- [ ] API Key 不出现在任何明文文件中
- [ ] 日志中只出现掩码后的 Key
- [ ] store/load/delete 功能正常
- [ ] 单元测试通过（掩码逻辑）
- [ ] README.md 完整

## 开发注意事项

- 使用 windows crate 的 CredWriteW/CredReadW/CredDeleteW API
- target 前缀统一为 "VTrans:" 避免与其他应用冲突
- CredentialType 使用 CRED_TYPE_GENERIC
- 测试中如无 Windows Credential Manager 可用，mock store 行为
- 禁止在 tracing 日志中输出完整 Key
