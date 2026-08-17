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

通过 Windows Credential Manager 或安装目录内的 DPAPI 加密文件安全存储和读取
API Key 等敏感凭据，支持按云端 Provider（OpenAI / DeepL / Google / Azure /
百度 / 预留腾讯）独立存取。禁止将凭据写入明文配置文件或日志。

自发行部署（v0.1.0）起，**凭据本地化为默认后端**：应用层把凭据存进便携
数据根内的单容器文件 `{exe}/data/credentials.bin`（[`DpapiFileStore`]，写入
前经 Windows DPAPI `CryptProtectData` 加密，绑定 Windows 用户），替代系统
凭据管理器作为默认。`WindowsCredentialStore`（Windows Credential Manager）
保留为**迁移来源**与不可用时的兼容回退实现（容器路径由调用方传入，本
crate 不假定固定位置）。

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

/// 安装目录内 DPAPI 加密文件凭据存储（发行部署默认后端）
///
/// 所有凭据存于单个容器文件，路径由调用方传入（应用层决定 data/ 根，
/// 本 crate 不假定固定位置）。写入前经 CryptProtectData（DPAPI，绑定
/// Windows 用户 + 应用熵常量）加密，容器文件永不包含明文 Key；每次变更
/// 原子替换（同目录临时文件 + rename），并发调用经内部 mutex 串行化。
pub struct DpapiFileStore { /* ... */ }

impl DpapiFileStore {
    /// 打开或创建容器文件（存在则保留已有凭据，父目录必须已存在）
    pub fn new(path: &Path) -> Result<Self, SecurityError>;
    /// 返回构造时传入的容器路径
    pub fn path(&self) -> &Path;
}

impl CredentialStore for DpapiFileStore {
    fn store(&self, target: &str, secret: &[u8]) -> Result<(), SecurityError>;
    fn load(&self, target: &str) -> Result<Option<Vec<u8>>, SecurityError>;
    fn delete(&self, target: &str) -> Result<(), SecurityError>;
    fn list_targets(&self) -> Result<Vec<String>, SecurityError>;
}

/// 一次性迁移：把 Windows 凭据管理器中所有 `VTrans:` 前缀凭据逐条迁入
/// `new_store`（读旧 → 写新 → 删旧，写失败不删旧，可安全重跑）。
/// 返回成功迁移条数（无可迁移为 0，非错误）；仅当旧库整体不可枚举时
/// 返回 Err，单条失败容忍并记录日志。
pub fn migrate_windows_to_dpapi(new_store: &DpapiFileStore) -> Result<usize, SecurityError>;
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
    #[error("credential file io error: {0}")]
    FileIo(#[from] std::io::Error),      // DpapiFileStore 容器 open/read/write/rename
    #[error("credential file is corrupted: {0}")]
    CorruptedFile(String),               // 容器结构损坏（坏 magic/版本/截断/越界长度）
    #[error("credential decryption failed: {0}")]
    DecryptionFailed(String),            // DPAPI 解密失败（篡改/其他用户上下文保护）
}
```

## 已知限制

- **DPAPI 用户绑定**：`DpapiFileStore` 的密文绑定创建时的 Windows 用户与
  应用熵常量。把 `credentials.bin` 复制到其他用户/其他机器后读取会得到
  `DecryptionFailed`（视为不可信密文，绝不静默当空库）；卸载重装**同一
  用户**下凭据仍可读。
- **路径由调用方传入**：本 crate 不假定容器位置；发行部署下由应用层固定
  为 `{exe}/data/credentials.bin`（`CREDENTIAL_FILE_NAME`）。移到无写权限
  目录（如 Program Files）会导致 store/delete 失败（`FileIo`）。
- **迁移语义**：`migrate_windows_to_dpapi` 只迁移 `VTrans:` 前缀条目；
  单条失败（含非 UTF-8 值）跳过并继续，返回值为成功条数；重跑安全
  （已迁条目覆盖写入并再次删除遗留旧条目）。`WindowsCredentialStore`
  保留为迁移来源与回退实现，未移除。
- 应用层回退链（见 10-app.md）：DPAPI 文件存储不可用 → 系统凭据管理器；
  再不可用 → 内存存储（凭据不持久化，使用时报明确错误）。

## 内部文件结构

```text
crates/vtrans-security/
├── Cargo.toml
├── README.md
└── src/
    ├── lib.rs            # re-export
    ├── manager.rs        # CredentialManager 实现
    ├── credential_store.rs # CredentialStore trait + Windows Credential Manager FFI 封装
    ├── dpapi.rs          # DpapiFileStore（DPAPI 文件容器）+ migrate_windows_to_dpapi
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
| DPAPI 文件存储往返 | 集成 | 真实 DPAPI：store/load/delete 往返、覆盖写、空密钥、容器文件不含明文（需 Windows 环境） |
| 容器格式 | 单元 | 坏 magic / 不支持的版本 / 截断 / 越界长度 / 尾随垃圾 → `CorruptedFile`；空文件 = 空容器 |
| 篡改密文 | 集成 | 结构合法但 blob 非 DPAPI → `DecryptionFailed`（Windows） |
| 迁移 | 单元/集成 | mock 旧库：只迁 `VTrans:` 前缀、单条失败继续、写失败不删旧、非 UTF-8 跳过、返回成功条数 |

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

- 使用 windows crate 的 CredWriteW/CredReadW/CredDeleteW API（WindowsCredentialStore）
- `DpapiFileStore` 使用 `crypt32.dll` 的 CryptProtectData/CryptUnprotectData
  （`CRYPTPROTECT_UI_FORBIDDEN` + 应用熵常量），unsafe 块附 SAFETY 注释；
  容器格式 magic `VTRANCRD`、版本 1，改格式必须升版本
- target 前缀统一为 "VTrans:" 避免与其他应用冲突
- CredentialType 使用 CRED_TYPE_GENERIC
- 测试中如无 Windows Credential Manager 可用，mock store 行为
- 禁止在 tracing 日志中输出完整 Key
- 凭据目标命名固定：`openai` / `deepl` / `google` / `azure` / `baidu_app_id` / `baidu_secret` / `tencent`
- 百度 APP ID 与 Secret 分存两个目标，app 层分别读取后由 `BaiduProvider` 组装签名
- provider 固定目标（`openai` 等）不做真实 Windows vault 集成测试，避免覆盖/删除用户真实凭据；
  命名空间映射由内存后端单元测试与通用 API 集成测试覆盖
