# vtrans-app 模块改动交接说明

> 交付对象:vtrans-app 模块负责人
> 背景:frontend 模块已完成一轮 bug 修复(设置面板可编辑、Provider 切换契约、选区交互、选区窗口透明背景),其中部分功能依赖 app 层补齐后端命令才能闭环。本文档自包含,无需回看其他 bug 报告。
> 状态:2026-08-05,基于当前 `integration/mvp` 分支代码核实。

## 0. 一句话结论

app 层**必须**新增「API Key 写入/删除/状态查询」命令;**建议**新增完整配置读取命令、IPC 契约测试与热键重注册;**可选**提供区域缩略预览能力(需架构确认)。其余事项前端已对齐或已由 app 层/宿主修复,无需重复处理(见 §6)。

---

## 1. 必改:API Key 管理命令(前端设置面板已预留入口)

### 1.1 现状

- 前端设置面板已可编辑 API 端点、模型名、超时、重试并调用 `save_settings` 保存,但**无法在应用内输入/保存 API Key**,因为 app 层没有写凭据的命令。
- 后端已有读取链路:`AppState::new` 里 `load_api_key(&credentials, &config)`(`crates/vtrans-app/src/state.rs`),通过 `CredentialManager` 读取逻辑目标 `"translation"`(Windows Credential Manager 中的 `VTrans:translation`)。
- `AppState.credentials` 字段已存在(`state.rs`,类型 `CredentialManager`)。
- vtrans-security 的 `CredentialManager::store(target, key)` / `delete(target)` / `load(target)` 已可用,**security crate 无需改动**。
- `AppError::Security(#[from] SecurityError)` 已存在(`crates/vtrans-app/src/error.rs`),错误自动转换,无需新增错误变体。

### 1.2 需要新增的命令

全部放在 `crates/vtrans-app/src/commands.rs`,并注册进文件底部 `invoke_handler` 的 `tauri::generate_handler![...]` 列表。

#### 1.2.1 `set_api_key`(必做)

```rust
#[tauri::command]
#[tracing::instrument(skip(state, api_key))]
pub async fn set_api_key(
    api_key: String,
    state: State<'_, AppState>,
) -> Result<(), AppError>
```

行为要求:

1. **拒绝空 key**:`api_key.trim().is_empty()` 时返回明确错误(建议复用 `AppError::Tauri("api key must not be empty".into())` 或新增语义化变体,见 §1.4),并 `warn!` 记录。
2. 调用 `state.credentials.store("translation", &api_key)`(内部自动加 `VTrans:` 前缀)。
3. **存储成功后重建运行时 Provider**:若当前配置 `config.translation.provider == "api"`,调用 `state.prepare_translation_provider(config.clone()).await?` 再用 `state.replace_translation_provider(provider)` 替换,否则内存中的 API Provider 仍持有旧 key。若当前是 `local`,只存凭据、不重建(避免无谓地重载 ONNX 模型)。
4. 成功后 `info!("translation api key stored")`,**绝不记录 key 本身**。

#### 1.2.2 `delete_api_key`(建议)

```rust
#[tauri::command]
#[tracing::instrument(skip(state))]
pub async fn delete_api_key(state: State<'_, AppState>) -> Result<(), AppError>
```

- 调用 `state.credentials.delete("translation")`。
- 注意 `CredentialManager::delete` 对不存在的目标返回 `SecurityError::NotFound`。前端需要「删除不存在的 key」不报错,所以建议在此命令内把 `NotFound` 当作成功(`Ok(())`)处理,其余错误透传。
- 删除后若当前 provider 为 `api`,同样重建 Provider(否则内存中旧 key 仍在用)。

#### 1.2.3 `get_api_key_status`(建议)

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ApiKeyStatus {
    /// Whether a translation API credential is stored.
    pub configured: bool,
    /// Masked tail for display, e.g. `sk-****1234`; `None` when not configured.
    pub masked: Option<String>,
}

#[tauri::command]
#[tracing::instrument(skip(state))]
pub async fn get_api_key_status(state: State<'_, AppState>) -> Result<ApiKeyStatus, AppError>
```

- 用 `state.credentials.load("translation")` 读取;存在时 `masked` 用 `vtrans_core::mask_sensitive(&key)` 生成。
- **只返回 `configured` 与 `masked`,绝不返回明文 key**。该结构体建议定义在 `state.rs`(与 `AppStatus` 同处)或 `commands.rs`,保持与现有 `AppStatus` 风格一致。

### 1.3 与前端约定的 IPC 契约(必须按此实现)

| 命令 | 前端 invoke 调用 | 参数 JSON key |
| --- | --- | --- |
| `set_api_key(api_key)` | `invoke("set_api_key", { apiKey })` | `apiKey`(Tauri 2 默认将 Rust 参数 `api_key` 映射为 camelCase) |
| `delete_api_key()` | `invoke("delete_api_key")` | 无 |
| `get_api_key_status()` | `invoke("get_api_key_status")` | 无 |

> 注意:这是 Tauri 2 默认行为(Rust 参数 `snake_case` → 前端 JSON `camelCase`),与本次已修复的 `set_translation_provider` 参数 `providerId` 同一机制。请勿给这些命令加 `rename_all = "snake_case"`,除非同步调整前端(见 §6.1)。

### 1.4 错误与日志纪律

- 错误路径必须 `warn!` 或 `error!` 记录,保持错误链完整(`#[from]` 自动转换即可)。
- 日志中禁止出现 key 明文;`#[tracing::instrument(skip(state, api_key))]` 确保 key 不进 span 字段。
- 若需在日志中引用 key,用 `vtrans_core::mask_sensitive`;本任务里正常路径不需要记录 key 的任何形式。

### 1.5 测试建议

- 把「存 key → 读状态」的核心逻辑拆成可注入 `CredentialManager` 的纯函数(仿照 `update_translation_provider_config` 的既有做法,用 `CredentialManager::with_store(Arc::new(InMemoryCredentialStore::new()))`),单测覆盖:空 key 拒绝、store 后 `load` 一致、delete 后 `configured=false`、`masked` 不含明文。
- `crates/vtrans-app/tests/contracts.rs` 增加契约断言:新命令返回结构体 JSON 序列化后的字段名(`configured` / `masked`)。
- 依赖真实 Windows Credential Manager 的路径登记到 app README「手工验证项」(可复用现有 save_settings 全链路条目)。

---

## 2. 建议:完整配置读取命令 `get_app_config`

### 2.1 为什么需要

前端 `save_settings` 是整包保存,但前端目前**没有**完整配置读取命令:store 以 `DEFAULT_CONFIG` 起步,只通过 `get_app_status` 水合了 `translation.provider` 一个字段。用户在设置面板编辑并保存时,会把表单未包含的字段(如 `ocr.language`、`log_level`)以前端默认值写回,存在覆盖后端配置的风险(已登记在 frontend README 已知限制)。

### 2.2 建议实现

```rust
#[tauri::command]
#[tracing::instrument(skip(state))]
pub async fn get_app_config(state: State<'_, AppState>) -> Result<AppConfig, AppError>
```

- 内部调用现有 `state.load_config()`(`set_translation_provider_id` 已在用同一方法)并返回。
- 前端启动水合流程改为:`get_app_status` + `get_app_config` 并行拉取,用完整配置替换默认值后再允许编辑保存。
- 注册进 `invoke_handler`。

> 若短期不实现,保持现状也可运行,但必须在 app README「已知限制」里保留整包覆盖风险的说明。

---

## 3. 建议:固化 IPC 参数名契约测试

### 3.1 背景

此前 bug:前端传 `{ provider_id }` 而 Tauri 2 期待 `{ providerId }`,导致 `set_translation_provider` 报 `missing required key providerId`。**前端已修复**(现在传 `{ providerId }`),后端无需改代码,但建议把契约固化,防止回归。

### 3.2 建议做法

在 `crates/vtrans-app/tests/contracts.rs` 增加:

1. 文档性测试/注释,明确 `set_translation_provider` 的前端 JSON 参数 key 是 `providerId`(Tauri 默认 camelCase 映射 Rust 参数 `provider_id`)。
2. 若使用 `tauri::test::mock_builder` 能跑通真实 invoke(取决于本机 Tauri 测试环境),用 mock 注册命令并断言 `{"providerId": "local"}` 可到达;若环境不允许,则登记为手工验证项并在注释中说明。

> 不建议给 `set_translation_provider` 加 `rename_all = "snake_case"`:那会破坏已合入的前端调用,且其他命令参数(`region`、`config`、`settings`)无下划线不受影响,维持现状即可。

---

## 4. 建议:保存热键后重新注册全局快捷键

前端设置面板现已开放快捷键编辑(选择并翻译/实时翻译/停止实时),通过 `save_settings` 保存。但 app README 已知限制写明:「通过 `save_settings` 修改热键配置后需要重启应用才会重新注册」。为闭环体验,建议:

- `save_settings` 在检测到 `hotkeys` 变化后,调用现有 `register_hotkeys(app.handle())` 重新注册;注册失败时返回 `AppError::HotkeyFailed`(此时配置已保存,需提示用户重启生效)。
- 注意现有 `register_hotkeys` 每次调用会注册一组新 shortcut;若实现重注册,需先注销旧 shortcut(查阅 `tauri-plugin-global-shortcut` 的注销 API)或整体重建,避免重复注册。

---

## 5. 可选:区域缩略预览(需架构确认,不阻塞)

- 前端对 Bug「选区不常驻显示」采用了**纯前端方案**:主窗口按坐标画等比缩放示意图(不跨 IPC 传图)。
- 若产品要求真实屏幕缩略图,需要 app 层提供缩略图能力。**红线:规格禁止 `CapturedImage` 跨越 IPC**,只能传缩小后的图片数据(如小尺寸 PNG/JPEG 的 base64/字节)。
- 可选实现方向:选区确认后 app 捕获区域并缩放成缩略图,通过事件(如 `region_thumbnail`)或命令推送给前端。此改动涉及架构决策,建议单独立项,不在本次范围内强推。

---

## 6. 已修复/无需处理的事项(避免重复劳动)

| 事项 | 状态 | 说明 |
| --- | --- | --- |
| `set_translation_provider` 参数名 | 前端已修 | 前端 `tauri.ts` 现传 `{ providerId }`,后端无改动;§3 建议补测试固化 |
| 选区窗口白屏 | 纯前端 | 背景色已移到 `body` 并按窗口隔离,后端无改动 |
| `register_hotkeys` 缺 `#[tracing::instrument]` | 已修 | `crates/vtrans-app/src/hotkeys.rs` 已带注解 |
| capability 缺 `core:window:allow-set-focus` / 权限超发 | 已修 | `src-tauri/capabilities/default.json` 已含 `allow-set-focus` 且权限与前端实际调用匹配(宿主项目维护,app 模块无需动) |
| 实时热键默认值不一致 | 已修 | 前端 `DEFAULT_CONFIG` 与 `vtrans-config` 默认均为 `Alt+Shift+R` |
| `translation_provider` 值域不一致(`local-onnx` vs `local`) | 已契约化 | 后端保留实现 id(`"api"`/`"local-onnx"`),前端 `normalizeProviderId` 映射;两端均有测试与 README 说明。**新增 Provider 时**需同步改后端 `validate_translation_provider_id` 白名单与前端 `normalizeProviderId` |
| `set_source_language` / `set_target_language` | 已存在 | 前端语言选择器已启用,对称于 `set_ocr_language`;无新工作 |
| `save_settings` | 已存在且前端已调用 | 无需改签名;§2/§4 为建议增强 |

---

## 7. 验收标准

- [ ] `set_api_key` 命令实现并注册,空 key 被拒绝,key 存入 `VTrans:translation`,且当前为 API Provider 时重建 Provider 使新 key 立即生效
- [ ] `delete_api_key`(若实现)对不存在的 key 视为成功
- [ ] `get_api_key_status`(若实现)只返回 `configured` 与 `masked`,不含明文
- [ ] 所有新增命令的日志路径不出现 key 明文(span 已 skip)
- [ ] `get_app_config`(若实现)返回完整 `AppConfig` 并注册
- [ ] `tests/contracts.rs` 固化 `providerId` 参数名契约与新增命令的 JSON 契约
- [ ] 相关单元测试覆盖纯逻辑;依赖真实桌面/凭据管理器的路径登记进 app README「手工验证项」
- [ ] `cargo fmt --all -- --check`、`cargo clippy -p vtrans-app --all-targets`、`cargo test -p vtrans-app` 全部通过
- [ ] README 的 commands 列表与已知限制同步更新

## 8. 质量门禁

```powershell
cargo fmt --all -- --check
cargo clippy -p vtrans-app --all-targets
cargo test -p vtrans-app
```
