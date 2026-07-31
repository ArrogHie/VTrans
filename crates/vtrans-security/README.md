# vtrans-security

通过 Windows Credential Manager 安全存储和读取 API Key 的凭据安全模块。

## 职责

凭据安全模块。API Key 等敏感凭据只写入 Windows Credential Manager（OS 加密保管库），
绝不写入明文配置文件或日志；日志中只允许出现 `mask_key` 掩码后的形式。

## 依赖

| 类型 | Crate | 用途 |
|------|-------|------|
| 上游 | `vtrans-core` | 日志掩码工具 `mask_sensitive` |
| 外部 | `windows` (0.58) | Windows Credential Manager FFI（`Win32_Security_Credentials`） |
| 外部 | `thiserror` | 错误类型派生 |
| 外部 | `tracing` | 结构化日志 |
| dev | `mockall` | store 错误路径单元测试 |

新增 `Win32_Security_Credentials` feature 的理由：Windows Credential Manager
（`CredWriteW`/`CredReadW`/`CredDeleteW`/`CredEnumerateW`/`CredFree`）需要该 feature，
workspace 根 `Cargo.toml` 未启用，按规则在本 crate 的 `Cargo.toml` 中追加（feature 合并）。

## 公开 API

```rust
// 凭据管理器（应用入口）
pub struct CredentialManager;
impl CredentialManager {
    pub fn new() -> Result<Self, SecurityError>;              // Windows Credential Manager 后端
    pub fn with_store<S: CredentialStore + 'static>(store: Arc<S>) -> Self; // 注入自定义后端
    pub fn store(&self, target: &str, api_key: &str) -> Result<(), SecurityError>;
    pub fn load(&self, target: &str) -> Result<Option<String>, SecurityError>; // 不存在返回 Ok(None)
    pub fn delete(&self, target: &str) -> Result<(), SecurityError>;           // 不存在返回 NotFound
    pub fn list_targets(&self) -> Result<Vec<String>, SecurityError>;          // 仅返回逻辑名，不含密钥
}

// Store 抽象与后端
pub trait CredentialStore: Send + Sync { /* store / load / delete / list_targets */ }
pub struct WindowsCredentialStore;      // 生产后端（FFI 封装）
pub struct InMemoryCredentialStore;     // 测试/开发后端（进程内存）

// 日志掩码
pub fn mask_key(key: &str) -> String;   // 委托 vtrans_core::mask_sensitive

pub enum SecurityError {
    StoreUnavailable(String),
    NotFound(String),
    OperationFailed(String),
    WindowsApi(String),
}
```

设计要点：

- 所有 target 自动加 `VTrans:` 命名空间前缀（`load`/`delete` 传入已带前缀的 target 也不会重复加），
  `list_targets` 返回去掉前缀的逻辑名，并过滤掉其他应用的凭据。
- Windows 后端使用 `CRED_TYPE_GENERIC` + `CRED_PERSIST_LOCAL_MACHINE`（本地持久化、不漫游）。
- `list_targets` 使用 `CredEnumerateW`，并处理 Windows 返回的
  `LegacyGeneric:target=` 限定名格式（见「已知限制」）。
- 公开方法均标注 `#[tracing::instrument]`，密钥参数一律 `skip`，错误路径记录 `warn`。

## 构建与测试

```powershell
cargo build -p vtrans-security
cargo test -p vtrans-security
cargo clippy -p vtrans-security --all-targets
cargo fmt --all -- --check
```

单元测试（掩码、manager 逻辑、内存 store）不依赖系统；集成测试
`tests/credential_manager.rs` 使用真实 Windows Credential Manager，每次运行写入
`VTrans:test_<pid>_<name>` 唯一 target 并在结束后删除（含 panic 场景），不会残留。

## 已知限制

- `mask_key` 委托 `vtrans_core::mask_sensitive`，格式为「前 4 字符 + **** + 后 4 字符」；
  模块规格中的 `sk-****1234` 为该格式的示意（例如 12 字符 key 显示为 `sk-1****1234`）。
- Windows 枚举返回的 target 名带 `LegacyGeneric:target=` 限定前缀，store 已剥离；
  因此 `list_targets` 只返回 CRED_TYPE_GENERIC 凭据。
- `CredentialManager::new()` 按规格保持 `Result` 签名，当前构造不会失败
  （保留给未来的可用性探测，避免 API 破坏）。
- 本模块为 Windows 专用；`InMemoryCredentialStore` 可在非 Windows 环境用于测试与开发。
- 集成测试会真实读写用户凭据保管库（写入后立即清理）。

## 详细规格

参见 `docs/modules/03-security.md`
