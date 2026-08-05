# VTrans

VTrans 是一款面向 Windows 的桌面屏幕翻译工具：框选屏幕任意区域即可 OCR 识别其中的文字并翻译为目标语言；也可以固定一个区域，在画面或文本变化时自动实时翻译。

技术栈：Rust + Tauri 2 + React + TypeScript + ONNX Runtime（ort）+ Tokio。

## 功能特性

- **单次框选翻译**：按下-拖动-松开确定区域 → 截屏 → OCR → 文本标准化 → 翻译 → 结果展示。
- **固定区域实时翻译**：持续捕获 + 帧差检测，仅当画面或文本变化时触发 OCR/翻译，指纹去重避免重复输出；通道有界，任务不堆积。
- **双翻译引擎**：云端 API 与本地 ONNX 模型可通过配置切换，业务流水线无需改动。
- **多窗口架构**：主窗口（控制与设置）、选区窗口（透明框选）、结果窗口（翻译展示）、overlay 窗口（屏幕常驻选区方框）。
- **全局快捷键**：Alt+Shift+A 选区翻译、Alt+Shift+R 实时翻译、Alt+Shift+S 停止实时。
- **托盘与单实例**：关闭主窗口隐藏到系统托盘（左键/菜单恢复、菜单退出）；重复启动自动恢复已有实例，避免全局热键冲突。
- **Debug 模式**：`--debug` 或 `VTRANS_DEBUG=1` 启动时，实时显示进入 OCR 之前的捕获帧缩略图（仅显示、不保存、不持久化）。
- **隐私优先**：默认不保存截图、OCR 文本与译文；API Key 存入 Windows Credential Manager，不进入配置文件和日志；截图图像不跨 IPC 传输。

## 仓库结构

| 路径 | 说明 |
| --- | --- |
| `crates/vtrans-core` | 核心类型、Provider trait、错误类型、日志初始化（层级 0，契约冻结） |
| `crates/vtrans-config` | AppConfig schema、持久化与迁移（层级 1） |
| `crates/vtrans-security` | Windows Credential Manager 凭据存取（层级 1） |
| `crates/vtrans-text` | 文本清洗、行合并、指纹去重（层级 1） |
| `crates/vtrans-models` | 模型 manifest、SHA-256 校验、生命周期（层级 1） |
| `crates/vtrans-capture` | Windows Graphics Capture 屏幕采集（层级 2） |
| `crates/vtrans-ocr` | PP-OCR ONNX 检测 + 识别（层级 2） |
| `crates/vtrans-translation` | 云端 API 与本地 ONNX 翻译 Provider（层级 2） |
| `crates/vtrans-pipeline` | 捕获-OCR-翻译编排、帧差检测、有界通道、Debug 帧出口（层级 3） |
| `crates/vtrans-app` | Tauri Commands/Events、AppState、全局快捷键、托盘、overlay、Debug 模式（层级 4） |
| `src/` | React + TypeScript 前端（主/选区/结果/overlay 窗口，层级 4） |
| `src-tauri/` | Tauri 2 宿主（薄层，委托给 vtrans-app） |
| `scripts/` | 模型下载脚本 |
| `docs/` | 架构、开发环境、Git 工作流、模块规格与整合报告 |

> 层级 N 的 crate 只能依赖层级 < N 的 crate。模块拆分、冻结契约与依赖图详见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。

## 快速开始

### 环境要求

- Windows 10 1903+ 或 Windows 11（x64，桌面会话，支持 Graphics Capture）
- Rust 工具链（rustup，≥ 1.75），含 clippy / rustfmt
- Node.js LTS + pnpm
- Visual Studio 2022 Build Tools（Desktop development with C++，含 Windows 10 SDK）
- WebView2 Runtime（Windows 11 已预装）
- Tauri 2 CLI

```powershell
winget install Rustlang.Rustup
winget install OpenJS.NodeJS.LTS
npm install -g pnpm
cargo install tauri-cli --version "^2.0"
```

### 克隆、安装依赖与准备模型

```powershell
git clone <repo-url> VTrans
cd VTrans
pnpm install
cargo fetch

# 下载 OCR/翻译模型（需网络；模型文件不提交 Git）
.\scripts\download_models.ps1

# 校验模型完整性（与应用内 load_local_models 同一逻辑）
cargo run --bin vtrans-verify-models
```

模型目录布局（`src-tauri/resources/models/`，`manifest.json` 提交 Git，模型文件由 `.gitignore` 排除）：

```text
manifest.json
ocr/det.onnx
ocr/rec_ja.onnx
ocr/rec_en.onnx
ocr/dict_ja.txt
ocr/dict_en.txt
translation/model.onnx
translation/tokenizer.json
```

### 开发模式运行

```powershell
cargo tauri dev
```

也可以分别启动前端与宿主：

```powershell
pnpm dev          # Vite 开发服务器（HMR）
cargo run -p vtrans
```

### Debug 模式

```powershell
cargo tauri dev -- --debug
# 或设置环境变量
$env:VTRANS_DEBUG="1"; cargo tauri dev
```

开启后主窗口出现「Debug 模式 · 仅显示不保存」面板，实时显示进入 OCR 前的捕获帧缩略图（最长边 ≤ 480px、≤ 10fps 节流），用于定位「识别文字与选区方框内容不符」等捕获/区域问题。Debug 关闭时整条链路零开销，不落盘、不写日志。

### Release 构建

```powershell
cargo tauri build
```

安装包输出到 `src-tauri/target/release/bundle/`（MSI + NSIS）。

## 测试与质量门禁

| 命令 | 说明 |
| --- | --- |
| `cargo test --workspace` | 全部 Rust 测试 |
| `cargo clippy --workspace --all-targets` | 零警告（workspace 级 pedantic） |
| `cargo fmt --all -- --check` | 零差异 |
| `pnpm test` | 前端 vitest 单测 |
| `pnpm build` | tsc + vite 生产构建 |
| `cargo tauri build` | Release 打包 |

## 无头验证 CLI

无需启动 GUI 即可验证模块链路（OCR/翻译仍需要模型目录与真实桌面捕获环境）：

```powershell
# 单次链路：本地翻译（当前模型仅支持 en -> zh-CN）
cargo run -p vtrans-pipeline --example pipeline_verify -- `
  --models src-tauri/resources/models --language en --target zh-CN --mode single

# 实时链路：静止区域不重复输出，Ctrl+C 停止
cargo run -p vtrans-pipeline --example pipeline_verify -- `
  --models src-tauri/resources/models --language en --target zh-CN --mode live `
  --region 100,100,800,400 --interval-ms 500

# 单次链路：API 翻译（日文 -> 中文）
cargo run -p vtrans-pipeline --example pipeline_verify -- `
  --models src-tauri/resources/models --api-endpoint <url> --api-model <name> `
  --api-key <key> --language ja --target zh-CN --mode single

# 单独验证 OCR / 翻译 / 采集
cargo run -p vtrans-ocr --example ocr_verify -- `
  --models src-tauri/resources/models --image path/to/image.png
cargo run -p vtrans-translation --example translation_verify -- `
  --text "hello" --source en --target zh-CN --models src-tauri/resources/models
cargo run -p vtrans-capture --example capture_demo
```

## 文档索引

| 文档 | 内容 |
| --- | --- |
| `windows_screen_translator_agent_spec.md` | 原始产品规格 |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | 模块拆分、冻结契约、横切标准 |
| [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) | 开发环境搭建、构建/测试命令 |
| [docs/GIT_WORKFLOW.md](docs/GIT_WORKFLOW.md) | 分支策略与提交规范 |
| [docs/modules/NN-*.md](docs/modules/) | 11 个模块的详细规格 |
| [docs/integration-report.md](docs/integration-report.md) | MVP 整合报告 |
| `crates/*/README.md` | 各模块 README（公开 API、已知限制、手工验证项） |
| [src/README.md](src/README.md) | 前端模块说明 |

## 已知限制（MVP）

- 本地翻译模型仅支持 **en → zh-CN**；其它源语言（如日文）必须使用云端 API Provider。UI 在本地引擎 + 不支持语言对时会给出明确提示，不会静默失败。
- 修改全局快捷键后需**重启应用**生效（当前已知限制）。
- 仅支持 Windows（依赖 Graphics Capture / Credential Manager），需在桌面会话运行。
- Debug 模式下的捕获帧缩略图仅保存在内存最新一帧，随窗口销毁或退出释放；默认关闭。

## 许可证

MIT OR Apache-2.0
