# vtrans-security

## 1. 模块概述

通过 Windows Credential Manager 安全存取 API Key 等敏感凭据，并提供日志安全的掩码工具。支持按云端 Provider
（OpenAI / DeepL / Google / Azure / 百度 / 预留腾讯）独立存取凭据，百度 APP ID 与 Secret 分开存储。

**边界**：

- 做：凭据的存储（`store` / `store_for_provider`）、读取（`load` / `load_for_provider`）、删除
  （`delete` / `delete_for_provider`）、列举（`list_targets`）；密钥日志掩码（`mask_key`）。
- 不做：不把任何凭据写入明文配置文件——持久化完全交给 Windows Credential Manager（OS 加密保管库）。
- 不做：不管理 API Key 之外的配置项（provider 选择、默认值等属于 `vtrans-config` / 应用层）。
- 不做：不涉及网络请求、加密算法或密钥轮换。
- 不做：不含 UI / Tauri command（由 `vtrans-app` 调用本模块）。

## 2. 依赖关系

### 上游 crate

| crate | 本模块使用的核心概念 |
|-------|---------------------|
| `vtrans-core` | `mask_sensitive`：统一日志掩码（4 前缀 + `****` + 4 后缀），`mask_key` 委托给它 |

### 外部 crate

| crate | 用途 |
|-------|------|
| `windows` 0.58 | Windows Credential Manager FFI（`Win32_Security_Credentials` feature） |
| `thiserror` | 派生 `SecurityError` |
| `tracing` | 结构化日志，入口函数 `#[tracing::instrument]` |
| `mockall`（dev） | manager 错误路径的单元测试 |

### 下游消费方

| 消费方 | 需要本模块提供什么 |
|--------|-------------------|
| `vtrans-app` | `CredentialManager` 存取各翻译 provider 的 API Key；`mask_key` 用于日志安全展示 |

## 3. 快速上手

```rust
use std::sync::Arc;
use vtrans_security::credential_store::InMemoryCredentialStore;
use vtrans_security::{mask_key, CredentialManager, CredentialTarget, SecurityError};

fn main() -> Result<(), SecurityError> {
    // 1. 实例化：内存后端（测试/开发用）。生产环境改用 CredentialManager::new()，
    //    由 Windows Credential Manager 持久化；调用方只需持有一个 manager。
    let store = Arc::new(InMemoryCredentialStore::new());
    let manager = CredentialManager::with_store(Arc::clone(&store));

    // 2. 写入 / 读取（通用字符串 target，历史 API，兼容 `translation` 目标）
    manager.store("openai", "sk-1234567890abcdef")?;
    let key = manager.load("openai")?.expect("刚写入的 key 应存在");
    println!("读取成功，日志安全形式: {}", mask_key(&key));

    // 3. 按 provider 目标存取（推荐 API）：类型安全，杜绝拼写错误。
    //    百度 APP ID 与 Secret 是两个独立目标，分别存取。
    manager.store_for_provider(CredentialTarget::OpenAI, "sk-1234567890abcdef")?;
    manager.store_for_provider(CredentialTarget::BaiduAppId, "202401010001")?;
    manager.store_for_provider(CredentialTarget::BaiduSecret, "sk-baidu-secret-0123456789")?;
    assert_eq!(
        manager.load_for_provider(CredentialTarget::BaiduAppId)?.as_deref(),
        Some("202401010001")
    );
    manager.delete_for_provider(CredentialTarget::BaiduSecret)?;

    // 4. 列举：只返回逻辑名，不含密钥值
    assert_eq!(manager.list_targets()?, ["baidu_app_id", "openai"]);

    // 5. 错误处理：删除不存在的 target 返回 NotFound，而不是静默成功
    manager.delete("openai")?;
    assert_eq!(manager.load("openai")?, None);
    if let Err(e) = manager.delete("openai") {
        assert!(matches!(e, SecurityError::NotFound(_)));
    }

    // 6. 生命周期：manager 持有 Arc<store>，此处 drop。内存后端进程退出即清除；
    //    Windows 后端由 OS 保管，drop 和进程重启都不影响已存凭据。
    drop(manager);
    Ok(())
}
```

`CredentialTarget` 是凭据目标的唯一事实来源，所有目标的逻辑名固定：

| Provider | 逻辑目标 | 说明 |
|----------|----------|------|
| OpenAI | `openai` | API Key |
| DeepL | `deepl` | API Key |
| Google | `google` | Cloud Translation API Key |
| Azure | `azure` | Translator 订阅密钥 |
| 百度 | `baidu_app_id` | APP ID（非机密，但统一走凭据库） |
| 百度 | `baidu_secret` | Secret Key |
| 腾讯 | `tencent` | 预留，本功能不接入但目标可用 |

生产环境把第 1 步换成：

```rust
let manager = CredentialManager::new()?; // Windows Credential Manager 后端
```

注意：`CredentialManager` 不是 `Clone`，多调用方共享时使用 `Arc<CredentialManager>`。

## 4. 公开 API 概要

| 公开项 | 用途 |
|--------|------|
| `CredentialManager` | 应用入口；管理 `VTrans:` 命名空间并代理到后端 |
| `CredentialTarget` | 云端 Provider 凭据目标枚举（`openai` / `deepl` / `google` / `azure` / `baidu_app_id` / `baidu_secret` / `tencent`） |
| `CredentialStore` trait | 后端抽象（`Send + Sync`），可自定义实现 |
| `WindowsCredentialStore` | 生产后端（Windows Credential Manager FFI） |
| `InMemoryCredentialStore` | 测试/开发后端（进程内存，不落盘） |
| `mask_key(key: &str) -> String` | 日志掩码，委托 `vtrans_core::mask_sensitive` |
| `SecurityError` | `StoreUnavailable` / `NotFound` / `OperationFailed` / `WindowsApi` |
| `TARGET_PREFIX` | 常量 `"VTrans:"`，target 命名空间前缀 |

`CredentialManager` 方法签名：

```rust
pub fn new() -> Result<Self, SecurityError>;                     // Windows 后端；构造不触碰保管库
pub fn with_store<S: CredentialStore + 'static>(store: Arc<S>) -> Self; // 注入自定义后端
pub fn store(&self, target: &str, api_key: &str) -> Result<(), SecurityError>; // 覆盖写入
pub fn load(&self, target: &str) -> Result<Option<String>, SecurityError>;     // 不存在 -> Ok(None)
pub fn delete(&self, target: &str) -> Result<(), SecurityError>;               // 不存在 -> NotFound
pub fn list_targets(&self) -> Result<Vec<String>, SecurityError>;              // 排序去重，仅逻辑名
pub fn store_for_provider(&self, target: CredentialTarget, api_key: &str)
    -> Result<(), SecurityError>;                                              // 类型化写入，推荐
pub fn load_for_provider(&self, target: CredentialTarget)
    -> Result<Option<String>, SecurityError>;                                  // 类型化读取，推荐
pub fn delete_for_provider(&self, target: CredentialTarget)
    -> Result<(), SecurityError>;                                              // 类型化删除，推荐
```

`CredentialTarget` 相关方法：

```rust
impl CredentialTarget {
    pub const ALL: [Self; 7];                                  // 全部目标，稳定顺序
    pub const fn as_str(self) -> &'static str;                 // 逻辑目标名（如 "openai"）
}
impl fmt::Display for CredentialTarget { /* 委托 as_str */ }
```

> 历史字符串 API（`store` / `load` / `delete`）保持完全兼容，现有 `translation` 目标不受影响；
> 新增代码应优先使用 `store_for_provider` / `load_for_provider` / `delete_for_provider`。

自定义后端需实现 `CredentialStore`（`target` 为带 `VTrans:` 前缀的限定名）：

```rust
pub trait CredentialStore: Send + Sync {
    fn store(&self, target: &str, secret: &[u8]) -> Result<(), SecurityError>;
    fn load(&self, target: &str) -> Result<Option<Vec<u8>>, SecurityError>;
    fn delete(&self, target: &str) -> Result<(), SecurityError>;
    fn list_targets(&self) -> Result<Vec<String>, SecurityError>;
}
```

serde：本模块所有类型均不实现 `Serialize`/`Deserialize`，不跨 JSON/IPC 边界；应用层把 `CredentialManager` 放进 `AppState` 通过 `Arc` 共享。

## 5. 行为契约

- **错误语义**：`load` 对不存在的 target 返回 `Ok(None)`（正常业务分支，可忽略）；`delete` 对不存在的 target 返回 `NotFound`（业务状态，重试无意义）；`WindowsApi` 表示 OS 调用失败（环境/权限问题，修复后可重试）；`OperationFailed` 表示数据问题（如非 UTF-8 blob），重试无意义；`StoreUnavailable` 表示后端不可用。
- **并发模型**：`CredentialManager` 为 `Send + Sync`，所有方法取 `&self`，可多线程并发调用；`WindowsCredentialStore` 无状态；`InMemoryCredentialStore` 内部 `Mutex` 串行化；`store` 是最后写者胜的覆盖语义。
- **取消语义**：本模块 API 全部同步，无 `CancellationToken`，无取消点（规格如此）。
- **资源生命周期**：无文件句柄或会话需要调用方关闭；`CredReadW`/`CredEnumerateW` 分配的缓冲区在内部用 `CredFree` 释放；drop `CredentialManager` 不删除任何凭据；Windows 后端凭据跨进程/重启存活，`InMemory` 后端随进程退出清除。
- **边界条件**：空 target 存入裸 `VTrans:` 前缀下；已带前缀的 target 不会重复加前缀；空字符串 key 允许存储；非 UTF-8 blob 的 `load` 返回 `OperationFailed`；空 blob 的 `load` 返回 `Some("")`。
- **Provider 目标**：`store_for_provider` 等方法的 target 为 `CredentialTarget` 枚举，逻辑名固定（见上文表格），内部同样走 `VTrans:<target>` 命名空间；百度 APP ID 与 Secret 是两个独立目标，互不覆盖；`tencent` 为预留目标，功能可用但暂无 provider 消费。
- **向后兼容**：`translation` 历史目标不属于 `CredentialTarget`（它不是 provider），继续通过字符串 API 存取；`list_targets` 会同时列出 provider 目标与历史目标。

## 6. 集成注意事项

| 坑 | 正确做法 |
|----|----------|
| `store` 会静默覆盖同 target 的旧 key（最后写者胜） | 覆盖前先 `load` 确认，或在前端做二次确认 |
| `delete` 不存在的 target 返回 `NotFound`，不是静默成功 | 用 `matches!(e, SecurityError::NotFound(_))` 分支处理，不要 `unwrap` |
| 把 API Key 存进 `config.json`（明文） | 一律通过 `CredentialManager` 存取，配置文件只放非敏感配置 |
| 在日志里打印 `load` 的返回值 | 日志只允许 `mask_key(&key)` 的掩码形式 |
| 集成测试在真实 Windows 凭据库写入 | 用唯一 target（如 `test_<pid>_<name>`）并保证清理，避免互相污染 |
| `CredentialManager::new()` 构造不校验保管库可用性 | 首次 `store`/`load` 报错即为不可用，按 `WindowsApi`/`StoreUnavailable` 处理 |

## 7. 设计决策记录

| 决策 | 理由 | 备选方案 |
|------|------|----------|
| `mask_key` 委托 `vtrans_core::mask_sensitive` | 横切标准强制统一掩码格式（4 前缀 + 4 后缀），避免两套掩码逻辑 | security 自实现 3+4 格式（与 core 冲突；规格示例 `sk-****1234` 仅为示意） |
| `VTrans:` 前缀逻辑集中在 `CredentialManager`，store 只存限定名 | store 是通用 FFI 封装，应用专属约定集中一处，便于单测与复用 | `WindowsCredentialStore` 内置前缀（把应用语义耦合进 FFI 层） |
| `CredentialStore` 抽象 + 内存后端 | 非 Windows/CI 环境无法测 FFI，内存后端保证 manager 逻辑可完整单测 | 全 mockall mock（失去真实内存行为，覆盖不了前缀/过滤逻辑） |
| `delete` 不存在返回 `NotFound` | 规格定义了该变体，且 `load` 契约已是 `Ok(None)`，`NotFound` 是 `delete` 的合法信号 | 幂等删除（调用方无法区分"已删"与"本来就没有"） |
| `new()` 保持 `Result` 签名（当前不可失败） | 规格签名 + 为未来 `StoreUnavailable` 探测预留，不破坏 API | 直接返回 `Self`（偏离规格） |
| `list_targets` 全量枚举后按类型/前缀过滤 | `CredEnumerateW` 返回名带 `LegacyGeneric:target=` 限定前缀，过滤器语义不可靠 | 传 filter 参数（实测与限定名不匹配，过滤无效） |
| 新增 `CredentialTarget` 枚举 + `*_for_provider` 方法 | 类型安全、拼写不可能出错、目标名单一事实来源；与现有字符串 API 并存 | 只加 `&str` 常量（无法编译期防拼写错误；与 `store` 无本质区别） |
| `*_for_provider` 委托字符串 API | 命名空间限定（`qualify_target`）、错误映射、日志逻辑只保留一份，行为完全一致 | 复制实现（两处维护，容易漂移） |
| 百度 APP ID 与 Secret 拆成两个目标 | 规格要求独立存取、由 `BaiduProvider` 组装签名；凭据生命周期各自独立 | 合成一个目标（无法单独轮换 Secret） |
| `tencent` 预留在枚举中但无 provider 消费 | 规格要求预留；目标可用、测试覆盖，接入时零改动 | 暂不加入（后续加变体即破坏 `ALL` 匹配的穷尽性，需同步改消费方） |
| provider 固定目标不做真实 Windows vault 集成测试 | 目标名固定（如 `VTrans:openai`），真实 vault 测试会覆盖/删除用户真实凭据；命名空间映射已由单元测试 + 通用 API 集成测试覆盖 | 真实 vault 写固定目标（危险，可能破坏用户数据） |

## 8. 已知限制

**设计使然**：

| 限制 | 缓解方式 |
|------|----------|
| 仅 Windows 可用；其他平台只能用 `InMemoryCredentialStore` | 开发/测试用内存后端，生产仅面向 Windows |
| `list_targets` 枚举整个用户凭据库再过滤 | 仅保留 `CRED_TYPE_GENERIC` 且按 `VTrans:` 前缀过滤；凭据名不敏感 |
| 掩码为 4 前缀 + 4 后缀，与规格示意 `sk-****1234` 略有出入 | 行为以 `vtrans_core::mask_sensitive` 为准，README 与代码注释已说明 |
| `new()` 不做 vault 可用性探测 | 首次操作报错即为信号 |
| provider 固定目标（`openai` 等）不做真实 vault 集成测试 | 避免覆盖/删除用户真实凭据；映射逻辑由内存后端单元测试完整覆盖 |
| `translation` 历史目标保留，不并入 `CredentialTarget` | 它是遗留 app 层目标而非 provider；后续废弃走独立流程 |

**待后续 Phase（规格未要求，当前明确不做）**：密钥轮换、托管加密（如 DPAPI 二次加密）、凭据备份迁移。如未来需要，在 `CredentialStore` 层新增后端即可，不影响 manager 与调用方。

## 9. 构建与测试

```powershell
cargo check -p vtrans-security
cargo test -p vtrans-security
cargo clippy -p vtrans-security --all-targets
cargo fmt -p vtrans-security -- --check
cargo test -p vtrans-security --doc
```

说明：`cargo test` 会同时运行单元测试（掩码、manager 逻辑）和集成测试 `tests/credential_manager.rs`（真实 Windows Credential Manager，写入唯一 target 后自动清理）。

## 10. 详细规格引用

参见 `docs/modules/03-security.md`。
