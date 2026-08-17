# VTrans 开发环境说明

本文档指导开发者从零搭建 VTrans 开发环境，进行构建、测试和调试。

## 1. 前置条件

### 1.1 操作系统

- Windows 10 1903+ 或 Windows 11
- x64 架构

### 1.2 Rust 工具链

```powershell
winget install Rustlang.Rustup
rustc --version
cargo --version
rustup toolchain install nightly
rustup component add clippy rustfmt
```

### 1.3 Node.js 与前端工具链

```powershell
winget install OpenJS.NodeJS.LTS
node --version
npm --version
npm install -g pnpm
```

### 1.4 Tauri 2 CLI

```powershell
cargo install tauri-cli --version "^2.0"
```

### 1.5 Windows SDK

- 安装 Visual Studio 2022 Build Tools
- 勾选 "Desktop development with C++" 工作负载
- 包含 Windows 10 SDK (10.0.19041+)

### 1.6 WebView2 Runtime

Windows 11 已预装。Windows 10 需安装 Evergreen Runtime。

## 2. 项目结构

详见 docs/ARCHITECTURE.md 第 8 节。关键点：

- crates/ 下每个 vtrans-* 是独立的 Rust crate
- src/ 是 React 前端
- src-tauri/ 是 Tauri 应用入口（薄层，委托给 vtrans-app）
- docs/modules/ 是各模块的详细规格

## 3. 克隆与初始化

```powershell
git clone <repo-url> VTrans
cd VTrans
pnpm install
cargo fetch
```

## 4. 模型文件准备

OCR 模型为 PP-OCRv6 Small（det + rec）。自 v0.1.0 发行部署起模型文件分两类管理：

- **OCR 模型随仓库入库（Git LFS）**：`.gitattributes` 将 `*.onnx` / `*.bin`
  标记为 Git LFS。`src-tauri/resources/models/` 下的 `manifest.json`、
  `ocr/det.onnx`、`ocr/rec.onnx`、`ocr/ppocrv6_dict.txt` 与
  `translation/tokenizer.json` 均已入库，并经 `tauri.conf.json` 的
  `bundle.resources` 打包内置（安装包与开发版都以此作为模型自愈源）。
- **翻译模型不进包**：`translation/model.onnx`（`opus-mt-en-zh-int8`，
  约 403 MB）不在仓库、不进安装包。manifest 中该条目标记
  `"optional": true` 并携带下载元数据（`download_url` /
  `download_size_bytes`，以 manifest 为准）。用户安装后在设置页「本地翻译
  模型」卡片下载到 `{exe}/data/models/translation/`（下载/续传/校验流程见
  `docs/modules/10-app.md`）。

仓库内模型目录布局：

```text
src-tauri/resources/models/
  manifest.json
  ocr/
    det.onnx             # PP-OCRv6 Small det（LFS）
    rec.onnx             # rec_ja / rec_en / rec_multi 三槽位共享同一文件（LFS）
    ppocrv6_dict.txt     # 字典（随仓库入库）
  translation/
    tokenizer.json       # 打包内置；model.onnx 不在仓库，运行时下载
```

### 4.1 一键准备（开发机重生成工具）

`scripts/ppocrv6/setup_ppocrv6.ps1` 提供「下载 → 转换 ONNX → 检查 → Python
基准 → manifest 回填」全流程。该脚本仅服务于**开发机重生成/更新模型**，
不参与打包与安装流程——安装包内容以仓库 LFS 文件与 `bundle.resources` 为准；
更新模型后需重新提交 LFS 文件与 manifest 变更。

```powershell
.\scripts\ppocrv6\setup_ppocrv6.ps1
```

参数：

```powershell
.\scripts\ppocrv6\setup_ppocrv6.ps1 -SkipConversion   # 使用已提供的 ONNX（跳过 PaddleX 转换）
.\scripts\ppocrv6\setup_ppocrv6.ps1 -SkipBaseline     # 跳过 Python 基准
```

### 4.2 开发机要求（转换/检查/基准）

- Python 3.10 或 3.11（Windows 下 3.12 亦可）
- PaddlePaddle 3.0+（脚本锁定 3.3.1）、`paddlex[ocr]`、paddle2onnx 2.0.2rc3 插件
- `onnx`、`onnxruntime`、`opencv-python-headless`、`numpy`、`pyclipper`、`pyyaml`
- Windows 转换若遇到 paddle2onnx DLL 问题，使用 WSL2 执行本脚本
- 转换工具链版本漂移后需重跑固定回归集（见接入指南 §21）

### 4.3 手动校验

模型文件就位后运行完整性校验 CLI（与 `load_local_models` 同语义，详见
`docs/modules/08-models.md`）：

```powershell
cargo run --bin vtrans-verify-models                        # 默认目录 src-tauri/resources/models
cargo run --bin vtrans-verify-models -- --models <dir>      # 显式指定目录
$env:VTRANS_MODEL_DIR="<dir>"; cargo run --bin vtrans-verify-models   # 环境变量（仅 CLI 生效）
```

全部必选文件通过即输出 `all model files are valid`；optional 条目缺失时输出
`skipped: ...（optional, not installed）` 行且不影响退出码。

### 4.4 翻译模型来源（可选）

本地翻译模型（`translation/model.onnx` + `tokenizer.json`）不属于 v6 OCR
升级范围，旧版 `scripts/download_models.ps1` 已随 v4 一并移除。**最终用户
不需要本节**：安装后在设置页「本地翻译模型」卡片下载即可。开发机如需重
生成翻译模型，使用 `teradata-opus-translate`：

```powershell
python -m pip install teradata-opus-translate
python -c "from teradata_opus_translate import convert_model, convert_tokenizer; convert_model('Helsinki-NLP/opus-mt-en-zh', output_path='src-tauri/resources/models/translation/model.onnx', precision='int8'); convert_tokenizer('Helsinki-NLP/opus-mt-en-zh', output_path='src-tauri/resources/models/translation/tokenizer.json')"
```

注意：`tokenizer.json` 随仓库入库并打包内置；`model.onnx` 生成物仅留在开发
机（不入库、不进包），运行时由下载流程安装到 `{exe}/data/models/`。

## 5. 构建与运行

### 5.1 开发模式

```powershell
cargo tauri dev
pnpm dev
cargo check --workspace
```

### 5.2 Release 构建

```powershell
cargo tauri build
```

产物在 src-tauri/target/release/bundle/

### 5.3 单个 crate 构建

```powershell
cargo build -p vtrans-core
cargo build -p vtrans-ocr
```

## 6. 测试

### 6.1 全量测试

```powershell
cargo test --workspace
```

### 6.2 单个 crate 测试

```powershell
cargo test -p vtrans-core
cargo test -p vtrans-text
```

### 6.3 验证 CLI

```powershell
cargo run -p vtrans-ocr --example ocr_verify -- --image tests/fixtures/ja_horizontal.png
cargo run -p vtrans-translation --example translation_verify -- --text "Hello" --source en --target ja
```

### 6.4 前端测试

```powershell
pnpm test
```

### 6.5 Clippy 和格式化

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
```

## 7. 日志与调试

### 7.1 日志位置

开发模式日志输出到控制台。生产模式日志写入便携数据根下的
`{exe}/data/logs/`（不再使用 `%APPDATA%\com.vtrans.app\logs\`，布局见 §9）。

日志按小时轮转，保留最近 5 个文件。

### 7.2 日志级别控制

通过环境变量控制：

```powershell
$env:RUST_LOG="vtrans_ocr=debug,vtrans_pipeline=info,vtrans_core=warn"
cargo tauri dev
```

### 7.3 调试技巧

- Rust 侧使用 VS Code 的 rust-analyzer + CodeLLDB
- 前端使用 Chrome DevTools（Tauri 开发模式下可用 F12）
- Tauri DevTools 在 dev 模式自动启用

## 8. 环境变量

| 变量 | 说明 | 默认值 |
|------|------|--------|
| RUST_LOG | 日志级别过滤 | info |
| VTRANS_MODEL_DIR | 模型目录覆盖（**仅 `vtrans-verify-models` CLI 生效**；应用未实现，应用数据固定于 `{exe}/data`） | src-tauri/resources/models |
| VTRANS_API_KEY | API 翻译密钥（仅测试） | 无 |

## 9. 便携数据布局

应用自 v0.1.0 发行部署起采用**便携数据根**：所有可变状态位于可执行文件旁
的 `data/` 目录（`resolve_data_root` = `{exe_dir}/data`），安装版与开发版都
不再写 `%APPDATA%` / `%LOCALAPPDATA%`。

```text
{exe}/data/
  config.json       # 应用配置（ConfigManager 原子写）
  credentials.bin   # DPAPI 加密凭据容器（API Key，绑定 Windows 用户）
  logs/             # 滚动日志（小时轮转，保留 5 个，见 §7）
  models/           # 运行时模型：manifest.json + ocr/ + translation/
                    # 每次启动自愈：缺失/损坏的必选文件从包内只读源
                    # （resource_dir()/resources/models）重新复制；
                    # 默认位置，可被 config.json 的 model_dir 高级设置覆盖
  models/translation/model.onnx.part   # 翻译模型下载中的 .part 续传文件
```

- **一次性迁移**：首次启动时若 `%APPDATA%\com.vtrans.app\config.json` 存在
  且便携 `config.json` 缺失，旧配置会被复制进便携数据根（失败仅警告，
  不阻断启动）。
- **系统级例外**（不落在 `data/`，属操作系统/安装器职责）：
  - WebView2 Evergreen Runtime（系统级运行时，见 §1.6）；
  - NSIS 安装/卸载注册表项（卸载器自身记录，卸载时清除）；
  - Microsoft Visual C++ 运行库（VC Redist，系统级）。
- **不支持 perMachine 安装**：数据目录锚定安装目录（`{exe}/data`），
  per-user（NSIS currentUser 默认）之外的模式不受支持；把程序放进
  Program Files 等无写权限目录会导致 `data/` 无法创建、配置只读（启动仍
  继续，容错语义见 `docs/modules/10-app.md`）。
- **开发模式**：`cargo tauri dev` 的可执行文件位于 `target/debug/`，因此
  开发模式数据根为 `target/debug/data/`（`target/release/data/` 同理）；
  开发时若 `resource_dir()/resources/models` 不可用，会回退到源码检出目录
  `src-tauri/resources/models/` 作为模型自愈源。

## 10. IDE 配置建议

### VS Code

推荐扩展：rust-analyzer, Tauri, Even Better TOML, Tailwind CSS IntelliSense

.vscode/settings.json 配置：

```json
{
  "rust-analyzer.linkedProjects": ["Cargo.toml"],
  "rust-analyzer.cargo.features": "all"
}
```

## 11. 常见问题

### Q: cargo tauri dev 报 WebView2 错误
安装 WebView2 Evergreen Runtime。

### Q: Windows Graphics Capture 报权限错误
确保 Windows 版本 >= 1903，且应用在桌面会话中运行（非 RDP）。

### Q: ONNX 模型加载失败
检查 manifest.json 中的路径和 SHA-256。运行 cargo run --bin vtrans-verify-models。

### Q: 前端修改后不更新
确认 pnpm dev 或 cargo tauri dev 正在运行，检查 Vite HMR 连接。

### Q: 多显示器下选区位置偏移
检查应用是否启用了 Per-Monitor DPI Awareness V2，坐标是否转换为物理像素。
