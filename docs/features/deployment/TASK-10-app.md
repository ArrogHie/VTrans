# 模块开发说明：10-vtrans-app — 发行部署「便携数据布局 + 模型就位/下载 + 启动容错」增量

## AGENT_DEV_PROMPT 参数
- MODULE_NUMBER: 10
- MODULE_NAME: vtrans-app
- MODULE_SLUG: app
- CRATE_PATH: crates/vtrans-app
- SCOPE: app
- BRANCH_NAME: feat/10-portable-data-layout（从 main 拉取；前置：feat/03-dpapi-file-store 与 feat/08-manifest-optional-entries 已合并 main）

## 功能上下文
- 功能目标：单一数据根 `{exe}/data/`；OCR 模型内置开箱即用；翻译模型设置页一键下载；模型未就位不导致应用无法启动。
- 本模块承担的部分：R1（数据根锚定）、R2（首启模型就位 + 打包资源）、R4 后端（下载/状态命令与事件）、R5 构造点（DPAPI store + 迁移调用）、R6（启动容错）、R7 构建侧核对，以及仓库级 LFS 配置（本功能授权范围，见下）。
- 上游已提供（合并 main 后可用）：
  - `vtrans_security::DpapiFileStore`（`new(path)` + 实现 `CredentialStore`）+ `migrate_windows_to_dpapi`（返回迁移条数）。
  - `vtrans_models`：`ModelEntry` 新增 `optional`/`download_url`/`download_size_bytes`；`VerifyReport.skipped`；`verify_integrity` optional 缺失记 skipped；`verify::verify_entry` 签名不变。
- 下游消费方：11-frontend 按本任务单「IPC 契约」节实现 UI（两端一起改，app 先合并）。

## IPC 契约（Rust 侧，本任务定义；前端 TypeScript 类型见 TASK-11）

新增 5 个 Command（全部无参数、Tauri 2 默认 camelCase，注册进 `invoke_handler`）：
1. `download_translation_model() -> Result<(), AppError>`：从 manifest `translation.model.download_url` 流式下载（reqwest 已带 `stream`，`Cargo.toml:49`）到 `data/models/translation/model.onnx.part`；完成 sha256 校验后原子 rename 为 `model.onnx`；**promise 在完成/失败/取消时 resolve**；期间经 `model_download_progress` 事件推送进度；P1 断点续传：`.part` 已存在时带 `Range` 头续传（响应非 206 时从头下载）。下载成功后重建 translation provider（复用 `save_settings`/`prepare_translation_provider` 同模式，`commands.rs:748-766`）。并发下载 → 返回明确错误不重复启动。
2. `cancel_translation_model_download() -> Result<(), AppError>`：触发 AppState 保存的 CancellationToken；下载任务返回取消语义错误并清理 `.part`（或保留 `.part` 供续传——保留续传状态，但校验状态不得被污染）。
3. `delete_translation_model() -> Result<(), AppError>`：删除 `data/models/translation/model.onnx`（与残留 `.part`）；若正在下载先取消；当前 provider 为 local 时重建 provider（缺失态 → 前端「未安装」）。
4. `get_model_status() -> Result<ModelStatusReport, AppError>`：复用 `verify_integrity` 映射为 `ModelStatusReport`（**只读，不触发修复**）。
5. `retry_model_setup() -> Result<ModelStatusReport, AppError>`：重新执行 `ensure_data_models` 并返回最新状态（R6 横幅「重试」）。

Rust 侧 DTO（`#[derive(Serialize)]`，vtrans-app 内部定义，不进 core）：
```rust
pub struct ModelStatusReport {
    pub entries: Vec<ModelEntryStatus>, // { id, state: "ready"|"missing"|"invalid", optional }
    pub ocr_ready: bool,
    pub translation_ready: bool,
}
pub struct ModelEntryStatus { pub id: String, pub state: ModelState, pub optional: bool }
// ModelState 序列化为 "ready" / "missing" / "invalid"
```
语义：`ready`=存在且 sha256 通过；`missing`=缺失；`invalid`=存在但校验失败。optional 条目缺失 → `missing`（前端显示「未安装」而非「校验失败」）。

新增 1 个 Event（events.rs 常量 + 发射函数）：
- `model_download_progress`：`{ bytes: u64, total: u64, fraction: f32 }`（snake_case 字段；仿 `emit_model_loading_progress`）。

## 任务要求

- 范围：`crates/vtrans-app` + **本功能授权的打包侧文件**：`src-tauri/tauri.conf.json`（bundle.resources）、`src-tauri/resources/models/manifest.json`（translation.model 条目录入）、根 `.gitignore`/`.gitattributes`（OCR 模型随仓库 LFS 跟踪）。PR 描述中逐项说明授权范围内的根文件变更。**禁止**修改其他 crate、vtrans-core、capabilities（无新窗口/权限需求）。
- manifest.json 的 `download_url`：用户已确认「版本化直链 + 发布流程回填 sha256」方案（2026-08-17）。开发期录入版本化占位 URL（形如 `https://github.com/ArrogHie/VTrans/releases/download/v0.2.0/translation-model.onnx`，版本号与 `Cargo.toml workspace.package.version` 一致）；`download_size_bytes` 填本机 `translation/model.onnx` 实际字节数（与 manifest 既有 `sha256`/`size_bytes` 一致，若 manifest 缺 sha256 则用本机文件现算并同步 `size_bytes`）。README 注明「发布流程负责最终 URL 与 sha256 回填」。
- **2026-08-18 更新**：release 已创建（v0.1.0），资产实际名为 `model.onnx`，`download_url` 已由 fix/repack-download-url 修正为 `…/download/v0.1.0/model.onnx`。

### R1 数据目录锚定 exe（P0）
- `setup.rs:81-93`：`app.path().app_data_dir()` 改为 `{exe}/data`（`current_exe` 父目录 + `data`；创建目录失败按启动容错策略处理）。config（`setup.rs:85-87`）、logs（`setup.rs:48-64`）随之自动落位 `data/config.json`、`data/logs/`。
- `state.rs:234-238`：模型根改为 `{data}/models`（不再用 `app_data_dir/models`）；`config.model_dir` 保留为高级覆盖（`Some` 时优先），不暴露 UI、不改 schema。
- 迁移（P1）：首启若 `%APPDATA%\com.vtrans.app\config.json` 存在而 `data/config.json` 缺失 → 复制一次（失败仅 `warn!` 不阻塞启动）。

### R2 首启模型就位（P0）
- `tauri.conf.json` bundle 显式列出 `resources`：`resources/models/manifest.json`、`resources/models/ocr/**`、`resources/models/translation/tokenizer.json`；**不含 translation/model.onnx**。保持 `targets: "all"`、NSIS 安装模式 currentUser（R7 核对项，无需改）。
- 实现 `ensure_data_models()`：对 manifest 每个条目，`data/models` 下缺失或 sha256 不符 → 从内置源复制（内置源 = Tauri `resource_dir()/models`，只读）；复用 `vtrans_models::verify::verify_entry`。幂等、可自恢复（用户删坏 `data/models` 重启即修）；optional 条目（translation.model）**不复制**（无内置源）。
- 仓库配置：`.gitignore` 放行 `src-tauri/resources/models/ocr/*.onnx` 与 `src-tauri/resources/models/translation/tokenizer.json`（`*.onnx` 全局忽略保留，靠 `.gitattributes` 已有 LFS 规则跟踪；继续忽略 `translation/model.onnx` 与其余 `translation/*`）；`git add` 后确认 OCR 模型以 LFS 指针入库、403MB 模型不入库。

### R4 后端（P0 + P1 续传）
- 见「IPC 契约」节；下载任务持有 CancellationToken（存 AppState，与 provider 状态同锁域）；进度事件节流（如每 500ms 或每 1MB，实现自定并在 PR 说明）。
- 校验失败：删除/回滚 `.part`（或保留待重下——实现自定，但不得 rename、状态必须返回 `invalid`），返回明确 `AppError`。
- 下载中禁止切 local：`set_translation_provider`/`save_settings` 在下载进行中切到 local → 返回明确错误（与前端禁用双保险）。

### R5 凭据构造点（P0 + P1 迁移）
- `state.rs:233`：`WindowsCredentialStore` 替换为 `DpapiFileStore::new({data}/credentials.bin)`；构造失败按启动容错策略处理（凭据不可用不致命，翻译时返回明确错误）。
- 首启迁移（P1）：`credentials.bin` 不存在时调用 `migrate_windows_to_dpapi`（迁移条数记 `info!`，失败 `warn!` 不阻塞）。

### R6 启动容错（P0）
- 现状无 manifest 直接启动失败（`state.rs:238` 的 `?` 上抛至 `setup.rs:157`）改为：`ensure_data_models`/模型加载失败 → **应用仍启动**，记录错误状态（AppState 保存模型就位状态）；主窗口经 `get_model_status` 获知后显示错误横幅（前端任务）。
- OCR 未就位时所有翻译入口命令（`capture_once`、`start_live_translation`、`start_multi_realtime`）返回明确错误（如 `AppError::Model` 包装带「OCR 模型未就位，请重试模型修复」信息），不静默失败。

### 测试要求（新增，映射需求验收标准 6）
- 单元：`ensure_data_models` 幂等（tempdir 模拟 data + 内置源）：首次复制、二次跳过、删除后自恢复、损坏后重拷；optional 条目不复制。
- 单元：下载 sha256 校验失败回滚（不 rename、`.part` 清理、状态 invalid）；原子 rename 成功路径。
- 单元：`ModelStatusReport` 映射（ready/missing/invalid/optional）；`get_model_status` 不触发修复。
- 单元：OCR 未就位时翻译入口命令返回明确错误；下载中切 local 被拒。
- 单元：config 迁移（旧目录存在/缺失、目标已存在跳过）；contracts.rs 补 5 命令 + 1 事件 IPC 契约（camelCase、payload 形状）。
- 手工验证项（README）：`cargo tauri build` 断网构建、NSIS 安装到 D:\VTrans、OCR 离线开箱即用、`%APPDATA%`/`%LOCALAPPDATA%` 无 VTrans 数据、下载全流程（进度/取消/sha256/local 离线翻译/重新下载/删除）、删除 `data/models` 重启自恢复。

### 文档要求
- 同步 `crates/vtrans-app/README.md`：新命令/事件、数据布局、下载流程、已知限制（DPAPI 用户绑定、perMachine 不支持、开发模式 data 落 target/debug、LFS 要求）。

### 提交规范
`feat(app): <一句话描述>` 或 `chore(repack): <一句话描述>`（仓库配置类），可多次提交，每次可编译；PR 描述含实现说明、测试覆盖、验收 checklist、授权范围外文件变更清单。

## 横切标准提醒（逐项附带）
- 日志：`#[tracing::instrument]`；下载进度 `debug!`/`info!`（bytes/total/fraction，无敏感数据）；URL 只记 host 或脱敏（若 URL 含 token 查询参数——**禁止把带签名的 URL 原样写日志**）；凭据引用用 `mask_sensitive`。
- 错误：`AppError` 本 crate 定义（可新增变体，如 `ModelDownload(String)`/`ModelNotReady(String)`，映射规则与既有 `error.rs` 一致）；`#[from]` 保留错误链。
- 测试与风格：fmt/clippy pedantic 零警告；公开 API rustdoc；下载/校验 IO 走阻塞池（`spawn_blocking`），不阻塞 UI 线程。
- 冻结红线：不修改 vtrans-core；`CapturedImage` 不序列化跨 IPC（本功能无关，勿引入）。

## 完成定义（DoD）
- [ ] `cargo fmt --all -- --check`；`cargo clippy -p vtrans-app --all-targets`；`cargo test -p vtrans-app` 全绿；`cargo check --workspace` 通过
- [ ] 5 命令 + 1 事件注册并契约测试固化；下载/删除/状态命令与 provider 重建链路闭环
- [ ] 验收标准第 1/2/4 条代码层满足（打包资源清单、数据根、自恢复；安装包冒烟登记 README 手工验证项）
- [ ] 验收标准第 5 条代码层满足（DPAPI store 构造 + 迁移调用；凭据管理器清空为手工验证）
- [ ] 未修改其他 crate 与 vtrans-core；授权范围外文件零改动；README 已更新
- [ ] PR 描述含实现说明、测试覆盖、验收 checklist、LFS/仓库配置说明
