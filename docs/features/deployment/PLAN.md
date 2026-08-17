# 功能开发计划：发行部署（单文件夹安装 + 内置 OCR + 翻译模型一键下载）

## 概述
- 需求来源：`docs/features/deployment/REQUIREMENTS.md`（用户需求文档；P0=必须，P1=应当；"现状"锚点已核实）
- 功能目标：安装包内置 OCR 模型（开箱即用），403MB 本地翻译模型不进包、由设置页一键下载；安装后除系统级例外（WebView2、NSIS 卸载注册表、MSVC 运行库）外，一切数据只落在安装目录 `{exe}/data/`，不写 C 盘用户目录
- 使用场景：全新安装到可写自选目录（NSIS currentUser，如 `D:\VTrans`）→ OCR 离线可用；设置页「本地翻译模型」卡片执行下载/取消/重新下载/删除；删除或篡改 `data/models` 后重启自恢复
- 优先级 / 版本目标：P0（R1-R6 均为 P0；R7 与两处迁移为 P1）/ v0.2.0 发布前
- 状态：开发中（2026-08-17 拆解完成、3 项假设经用户确认，阶段 A 已派发）

## 验收标准（用户可验证，与需求文档一致）
- [ ] `cargo tauri build` 产出 NSIS+MSI；安装包含 OCR 模型与 tokenizer（约 36MB），不含 403MB 翻译模型；构建全程断网可完成
- [ ] 全新安装到 `D:\VTrans` 后：OCR 离线开箱即用；`%APPDATA%`、`%LOCALAPPDATA%` 下无 VTrans 数据文件（目录扫描验证）
- [ ] 设置页下载翻译模型：进度可见、可取消、完成即 sha256 校验通过；随后切 local 可离线翻译；重新下载/删除可用
- [ ] 删除或篡改 `data/models` 内容 → 重启自恢复（内置源重拷）或明确报错，应用不静默退出
- [ ] API Key 保存后 `data/credentials.bin` 存在、Windows 凭据管理器无 VTrans 条目
- [ ] `cargo test --workspace`、`cargo clippy --workspace --all-targets`、`pnpm test` 全绿；新增单测覆盖：optional 语义、`ensure_data_models` 幂等/自恢复、下载校验失败回滚

## 需求条目 → 模块映射（影响面分析）

| 需求 | 主责模块 | 协同 | 说明 |
|------|----------|------|------|
| R1 数据目录锚定 exe | 10-app | — | setup.rs/state.rs；config 迁移（P1）在 app 启动层 |
| R2 首启模型就位 | 10-app | 08-models（复用 `verify::verify_entry`） | 含 bundle.resources 与仓库 LFS 配置（授权 10 分支内完成） |
| R3 manifest 可选条目语义 | 08-models | 10-app（manifest.json 条目录入） | 07-translation 现状已满足（`TranslationError::ModelLoad` 路径），**不派单**，整合时复核 |
| R4 一键下载翻译模型 | 10-app（后端）+ 11-frontend（UI） | — | 两端契约见下 |
| R5 凭据本地化 | 03-security | 10-app（构造点替换 + 迁移调用） | |
| R6 启动容错 | 10-app | 11-frontend（错误横幅 + 重试） | |
| R7 文档与构建 | DOCSYNC（文档任务） | 10-app（tauri.conf targets/currentUser 核对） | |

**排除项**：01-core（冻结，无契约变更）、02-config（schema 不变，`model_dir` 保留为高级覆盖）、04-capture、05-ocr、06-text、09-pipeline（OCR 未就位拦截在 app 层命令入口，pipeline 无改动）。

## 涉及模块与顺序

| 序号 | 模块 | 任务类型 | 依赖 | 建议分支 | 状态 |
|------|------|----------|------|----------|------|
| 1 | 03-security | 新增+修改 | — | feat/03-dpapi-file-store | 待分配 |
| 2 | 08-models | 修改 | — | feat/08-manifest-optional-entries | 待分配 |
| 3 | 10-app | 新增+修改 | 依赖 1, 2 | feat/10-portable-data-layout | 待分配 |
| 4 | 11-frontend | 新增+修改 | 依赖 3（IPC 契约已定） | feat/11-model-download-ui | 待分配 |
| 5 | 文档同步 | 修改 | 依赖 3, 4 | docs/deployment-doc-sync | 待分配 |

### 阶段安排
- **阶段 A（并行）**：03-security + 08-models（层级 1，互不依赖）
- **阶段 B**：10-app（层级 4，依赖 A 两个模块的新 API 合并入 main 后从 main 拉分支）
- **阶段 C**：11-frontend（IPC 契约在本计划「IPC 契约」节已定稿，可与 B 并行开发；**合并顺序 10 先于 11**——前端调用新命令，先合前端会运行时 command 不存在，与多框功能同类教训一致）
- **阶段 D**：文档同步（依赖 B、C 合并后执行）

**整合顺序**：03 → 08 → 10 → 11 → DOCSYNC。每次合并后跑 workspace 门禁，失败即打回对应模块。

## 契约变更

### 冻结契约（vtrans-core）
- **不涉及**。不新增/修改任何 core 类型、trait 方法、错误变体或 serde 表示。
- `CredentialStore` trait 定义在 `vtrans-security`（`credential_store.rs`），`DpapiFileStore` 是**新增实现**，trait 签名不变。
- manifest 的 `optional`/`download_url`/`download_size_bytes` 是 `vtrans-models` 内部 schema 扩展，非 core 类型。
- 新增的 Command/Event 属于 10-app ↔ 11-frontend 两端契约，由 app 定义 DTO（不落入 core）。

### IPC 契约（10-app 与 11-frontend，两端一起改，先 app 后 frontend 合并）

**Commands（Rust 侧定义，前端调用；Tauri 2 默认 camelCase，均无参数）：**
- `download_translation_model() -> Result<(), AppError>`：开始下载 `data/models/translation/model.onnx`（`.part` 临时文件 → sha256 校验 → 原子 rename）；**invoke promise 在下载完成（成功或失败）时 resolve**；进度经事件推送；下载成功后复用既有 provider 重建路径（`commands.rs:748-766` 同模式）。并发下载返回明确错误（不重复启动）。
- `cancel_translation_model_download() -> Result<(), AppError>`：触发 AppState 中保存的 CancellationToken；正在下载的 promise 以 Cancelled 语义返回错误。
- `delete_translation_model() -> Result<(), AppError>`：删除 `data/models/translation/model.onnx`（含残留 `.part`）；若当前 provider 为 local 则重建 provider（切回失败态 → 前端提示未安装）。
- `get_model_status() -> Result<ModelStatusReport, AppError>`：返回各模型条目状态（复用 `verify_integrity` 的 optional 语义），不触发修复。
- `retry_model_setup() -> Result<ModelStatusReport, AppError>`：重新执行 `ensure_data_models`（R6 错误横幅的「重试」入口），返回最新状态。

**Events（Rust 侧推送，前端监听）：**
- `model_download_progress`：`{ bytes: number, total: number, fraction: number }`（仿 `model_loading_progress` 模式，字段名 snake_case）

**TypeScript 类型（前端定义，与 Rust 侧 serde 表示一一对应）：**
```typescript
type ModelState = 'ready' | 'missing' | 'invalid';
interface ModelEntryStatus { id: string; state: ModelState; optional: boolean; }
interface ModelStatusReport { entries: ModelEntryStatus[]; ocr_ready: boolean; translation_ready: boolean; }
interface ModelDownloadProgress { bytes: number; total: number; fraction: number; }
```
Rust 侧 `ModelStatusReport` 定义为 `vtrans-app` 内部 DTO（`#[derive(Serialize)]`），不进入 core；`state` 语义：`ready`=存在且 sha256 通过；`missing`=缺失；`invalid`=存在但校验失败（optional 且缺失归入 `missing`，前端据此显示「未安装」而非「校验失败」）。

### 配置 / Provider / 模型变更
- AppConfig schema **不变**（无新配置字段、无版本迁移）；`config.model_dir` 保留为高级覆盖，不暴露 UI。
- manifest：`translation.model` 条目新增 `"optional": true`、`"download_url"`、`"download_size_bytes"`（与 sha256 一致），URL 版本化、sha256 由发布流程回填（待确认问题 1）。
- Provider id 白名单与前端 `normalizeProviderId` **均不变**（local 语义不变，仅增加「未安装」状态）。

## 风险与假设

### 假设（信息不足处，需用户确认后生效）
1. 翻译模型 `download_url` 采用**版本化直链**（GitHub Releases 资产或 HuggingFace），最终 URL 与 sha256 由发布流程回填；开发期任务单使用占位 URL，DoD 不校验真实下载成功（校验到「URL 可配置、sha256 校验逻辑正确」为止）。
2. `VTRANS_CONFIG_DIR` / `VTRANS_MODEL_DIR` 环境变量（`DEVELOPMENT.md:215-219`）应用未实现：**按「删除文档条目」处理**（`VTRANS_MODEL_DIR` 保留 CLI `verify_models` 的说明）；若用户选择「实现」，10-app 任务追加此需求（待确认问题 2）。
3. 开发模式（`cargo tauri dev`）下 exe 位于 `target/debug/`，`data/` 随之落在 `target/debug/data/`，可接受且无需特殊处理。
4. `.gitattributes` 已含 `*.onnx filter=lfs`（已核实），远程仓库需启用 Git LFS 存储，否则推送失败（风险 1）。

### 风险
- **LFS 远程未启用**：OCR 模型入库后 push 失败 → 需仓库管理员先启用 LFS 并配置配额。
- **exe 目录不可写**：perMachine/Program Files 不支持（需求已声明）；安装器保持 currentUser。
- **DPAPI 用户绑定**：Windows 用户配置文件重建后凭据不可恢复——记录为已知限制，不做跨用户迁移。
- **下载中断/磁盘满**：`.part` 必须可清理、失败必须回滚（不留下半文件污染状态）、校验失败不得 rename。
- **下载与 provider 切换并发**：下载中禁止切 local（前端禁用 + 后端命令拒绝双重防护）；删除模型时若正在下载需先取消。
- **36MB OCR 模型入库**：仓库体积与 clone 时长增大；`git lfs` 指针与对象需正确提交。

### 已知限制排除（对照现状）
- 本地模型仅 `en → zh-CN`：本功能不改变语言对限制。
- 修改快捷键需重启生效：与本功能无关。
- 大图像不跨 IPC：下载/状态均为文本载荷，无关。
- 注：统筹提示词引用的全局 `docs/integration-report.md` 不存在（已知限制以各 crate README 与功能级报告为准），本次按 crate README 对照。

## 用户已确认决策（2026-08-17）
1. 翻译模型 `download_url`：接受「版本化直链 + 发布流程回填 sha256」方案；开发期按 GitHub Releases 版本化占位 URL（`https://github.com/ArrogHie/VTrans/releases/download/v<版本>/translation-model.onnx`）录入 manifest，发布流程负责最终 URL 与 sha256 回填。
2. `VTRANS_CONFIG_DIR` / `VTRANS_MODEL_DIR`：**删除文档条目**（不补实现）；`VTRANS_MODEL_DIR` 保留 CLI `verify_models` 的说明并注明仅 CLI 生效。
3. 开发模式 `data/` 落在 `target/debug/data/`：接受。
