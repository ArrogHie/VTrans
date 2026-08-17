# 发行部署需求：单文件夹安装 + 内置 OCR + 翻译模型一键下载

> 交付给开发 Agent 的需求说明。P0=必须，P1=应当。所有"现状"锚点已核实，勿重复探索。

## 目标

安装包内置 OCR 模型（开箱即用），本地翻译模型（403MB）不进包、由设置页一键下载；**安装后除系统级例外（见下），一切数据只在安装目录内**，不写 C 盘用户目录。

## 硬性约束

- 单一数据根 `{exe}/data/`：config.json、logs、下载的模型、凭据、一切运行时文件都在这里。
- 例外（允许，写入文档即可，不改）：WebView2 运行时（系统组件）、NSIS 卸载注册表条目、MSVC 运行库。
- 支持场景：可写的自选安装目录（NSIS currentUser，如 `D:\VTrans`）。不支持 perMachine/Program Files（目录只读）。
- OCR 模型文件随仓库管理（`*.onnx` 改由 Git LFS 跟踪），直接内置进安装包；构建与用户侧均不联网拉取 OCR。翻译模型（403MB）不进包。

## 目标布局

```
{安装目录}/
  VTrans.exe
  VTrans.exe.WebView2/       # WebView2 用户数据（默认即 exe 旁，保持默认，勿在 tauri.conf 设 dataDirectory）
  resources/models/          # 只读内置源：manifest.json + ocr/** + translation/tokenizer.json（不含 model.onnx）
  data/                      # 运行时数据根（全部可写数据）
    config.json
    logs/
    credentials.bin          # DPAPI 加密的云翻译 API Key
    models/                  # 运行时模型根（ModelManager 唯一加载源）
      manifest.json          # 首启从 resources 复制
      ocr/…  translation/tokenizer.json, translation/model.onnx（下载产物）
```

## 需求清单

### R1 数据目录锚定 exe（P0）

- `setup.rs:81-93`：`app.path().app_data_dir()`（现为 `%APPDATA%\com.vtrans.app`）改为 `{exe}/data`。config（`setup.rs:85-87`）、logs（`setup.rs:48-64`）随之自动落位。
- `state.rs:234-238`：模型根改为 `{data}/models`（不再是 `app_data_dir/models`）。`config.model_dir` 保留为高级覆盖，不暴露 UI。
- 迁移（P1）：首启若 `%APPDATA%\com.vtrans.app\config.json` 存在而 `data/config.json` 缺失，复制一次。

### R2 首启模型就位（P0）

- 安装包 `bundle.resources`（`tauri.conf.json:74-81`）显式列出：`resources/models/manifest.json`、`resources/models/ocr/**`、`resources/models/translation/tokenizer.json`。**不含 translation/model.onnx**。
- 启动时 `ensure_data_models()`：对 manifest 每个条目，`data/models` 下缺失或 sha256 不符 → 从 `resources/models` 复制（复用 `vtrans_models::verify::verify_entry`）。幂等、可自恢复（用户删坏 `data/models` 重启即修）。
- 模型来源（P0）：OCR 的 ONNX+dict 是仓库内本地文件（Git LFS），打包直接使用 `resources/models/` 现有文件，全程不联网。`scripts/ppocrv6/setup_ppocrv6.ps1` 保留为开发机可选的重生成工具，不参与打包流程。

### R3 manifest 可选条目语义（P0）

- `resources/models/manifest.json` 的 `translation.model` 条目加 `"optional": true` 与 `"download_url": "<固定直链>"`、`"download_size_bytes"`（与 sha256 一致）。
- `manager.rs:98-138` `verify_integrity`：optional 且缺失 → 记 skipped，不记 failed；`verify_models.rs` CLI 同语义。
- 翻译模型制品托管：GitHub Releases 资产或 HuggingFace 直链，URL 版本化；URL 的 sha256 必须与 manifest 一致（发布流程负责回填）。
- 本地翻译 provider 在 model.onnx 缺失时返回明确的"未安装"错误（现状已有 `TranslationError::ModelLoad` 路径，`local_onnx.rs:171-179`）。

### R4 一键下载翻译模型（P0）

后端（`crates/vtrans-app/src/commands.rs`，reqwest 已带 `stream`，`Cargo.toml:49`）：

- `download_translation_model`：GET `download_url` → 写 `data/models/translation/model.onnx.part` → 完成校验 sha256 → 原子 rename。发 `model_download_progress {bytes,total,fraction}` 事件（仿 `emit_model_loading_progress`）。支持取消（CancellationToken）。
- 断点续传（P1）：`.part` 已存在时带 `Range` 头续传。
- `get_model_status`：返回各模型就位/缺失/校验失败状态（复用 `verify_integrity`）。
- 下载后触发 provider 重建（复用 `save_settings`/`prepare_translation_provider`，`commands.rs:748-766` 同模式）。

前端（`src/components/SettingsPanel.tsx`）：

- 新增「本地翻译模型」卡片：状态=未安装/下载中(进度)/已安装/校验失败；按钮=下载/取消/重新下载/删除。
- `translation.provider` 选 local 但模型未安装 → 禁用并提示先下载（`ProviderSelect.tsx`）。
- 下载期间禁止切 local。

### R5 凭据本地化（P0）

- `vtrans-security`：新增 `DpapiFileStore` 实现 `CredentialStore` trait（`credential_store.rs:55-90`），文件 `data/credentials.bin`，用 Windows DPAPI `CryptProtectData` 加密（用户绑定、不落外部目录）。替换 `WindowsCredentialStore`（`state.rs:233` 的构造点）。
- 迁移（P1）：首启读 Windows 凭据管理器旧条目（`VTrans:` 前缀）成功则写入新 store 并删除旧条目。

### R6 启动容错（P0）

- 现状无 manifest 直接启动失败（`state.rs:238` 的 `?` 上抛至 `setup.rs:157`）。改为：模型就位失败 → 应用仍启动，主窗口显示错误横幅 + 重试按钮；OCR 未就位时所有翻译入口返回明确错误。

### R7 文档与构建（P1）

- `docs/DEVELOPMENT.md:215-219` 环境变量表与实现不符：`VTRANS_CONFIG_DIR`/`VTRANS_MODEL_DIR` 应用未实现（仅 `verify_models.rs:97` CLI 认后者）——实现或删条目。
- `tauri.conf.json` bundle 保持 `targets: "all"`（NSIS+MSI）；安装模式保持 currentUser（默认装 `%LOCALAPPDATA%`，用户可改到 D 盘，无需管理员）。
- 文档同步 `docs/DEVELOPMENT.md` §4/§7 到新布局（`data/`、下载流程）。

## 验收标准

1. `cargo tauri build` 产出 NSIS+MSI；安装包含 OCR 模型与 tokenizer（约 36MB），不含 403MB 翻译模型；构建全程断网可完成。
2. 全新安装到 `D:\VTrans` 后：OCR 离线开箱即用；`%APPDATA%`、`%LOCALAPPDATA%` 下无 VTrans 数据文件（目录扫描验证）。
3. 设置页下载翻译模型：进度可见、可取消、完成即 sha256 校验通过；随后切 local 可离线翻译；重新下载/删除可用。
4. 删除或篡改 `data/models` 内容 → 重启自恢复（内置源重拷）或明确报错，应用不静默退出。
5. API Key 保存后 `data/credentials.bin` 存在、Windows 凭据管理器无 VTrans 条目。
6. `cargo test --workspace`、`cargo clippy --workspace --all-targets`、`pnpm test` 全绿；新增单测覆盖：optional 语义、`ensure_data_models` 幂等/自恢复、下载校验失败回滚。

## 范围外

OCR/翻译模型本身的精度与选型；云翻译 provider 行为；UI 外观与多语言；自动更新（updater）。
