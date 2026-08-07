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

模型文件不提交 Git（字典 `ppocrv6_dict.txt` 例外，随 manifest 入库）。
OCR 模型为 PP-OCRv6 Small（det + rec）；自 v0.3.0 起翻译模型为
**双引擎**（Bergamot en→zh + CTranslate2 INT8 ja→zh，manifest v2）。
首次开发前目标布局：

```text
src-tauri/resources/models/
  manifest.json        # v2：OCR 段 + translation 双引擎段
  ocr/
    det.onnx
    rec.onnx          # rec_ja / rec_en / rec_multi 三槽位共享同一文件
    ppocrv6_dict.txt
  translation/
    en-zh/
      model.enzh.intgemm.alphas.bin
      srcvocab.enzh.spm
      trgvocab.enzh.spm
      lex.50.50.enzh.s2t.bin
    ja-zh/
      model.bin
      config.json
      source_vocabulary.json
      target_vocabulary.json
      source.spm
      target.spm
```

### 4.1 一键准备（推荐）

`scripts/ppocrv6/setup_ppocrv6.ps1` 提供「下载 → 转换 ONNX → 检查 → Python
基准 → manifest 回填」全流程：

```powershell
.\scripts\ppocrv6\setup_ppocrv6.ps1
```

参数：

```powershell
.\scripts\ppocrv6\setup_ppocrv6.ps1 -SkipConversion   # 使用已提供的 ONNX（跳过 PaddleX 转换）
.\scripts\ppocrv6\setup_ppocrv6.ps1 -SkipBaseline     # 跳过 Python 基准
```

### 4.2 翻译模型准备（v0.3.0+）

`scripts/translation/setup_translation_models.ps1` 提供「下载 en-zh → 转换
ja-zh → 体积审计（200 MB 门禁）→ manifest 回填」全流程：

```powershell
.\scripts\translation\setup_translation_models.ps1
```

分步脚本：

```powershell
python scripts\translation\fetch_firefox_enzh.py --download        # en-zh（Mozilla registry）
.\scripts\translation\convert_ja_zh_ct2.ps1                         # ja-zh（CTranslate2 INT8）
python scripts\translation\audit_model_sizes.py --self-test         # 门禁自测
python scripts\translation\backfill_translation_manifest.py --update-template
```

### 4.3 开发机要求（转换/检查/基准）

- Python 3.10 或 3.11（Windows 下 3.12 亦可）
- PaddlePaddle 3.0+（脚本锁定 3.3.1）、`paddlex[ocr]`、paddle2onnx 2.0.2rc3 插件
- `onnx`、`onnxruntime`、`opencv-python-headless`、`numpy`、`pyclipper`、`pyyaml`
- Windows 转换若遇到 paddle2onnx DLL 问题，使用 WSL2 执行本脚本
- 转换工具链版本漂移后需重跑固定回归集（见接入指南 §21）

翻译模型（`scripts/translation/`）额外要求：

- Python 3.10+（推荐 3.12）
- 网络访问：Mozilla 模型 registry（Google Storage）、Hugging Face（`shun89/opus-mt-ja-zh` 约 314 MB）
- 首次运行脚本会创建专用 venv（`scripts/translation/work/.venv`）并安装：
  - `ctranslate2==4.8.1`（锁定版本，勿升级）
  - `transformers>=4.50`、`torch`、`sentencepiece`、`huggingface_hub`
- 模型 revision 与 SHA-256 由脚本冻结进 `manifest.json` 的 `translation.metadata`；registry / HF 版本漂移后必须重跑回填

07 模块（native bridge）构建额外要求：

- CMake 3.20+（Bergamot v0.4.5 / CTranslate2 4.8.1 的 C++ 构建）
- Visual Studio 2022 Build Tools（含 "Desktop development with C++" 工作负载，即 §1.5 的 MSVC 工具链）

### 4.4 手动放置

也可手动放置模型文件后运行完整性校验：

```powershell
cargo run --bin vtrans-verify-models
```

校验 CLI 输出 `all model files are valid` 即通过。

注意：旧版 403 MB ONNX 单模型路径（`translation/model.onnx` +
`tokenizer.json`）已随 v0.3.0 移除（计划 A3）；`scripts/download_models.ps1`
亦已随 v4 删除。`.gitignore` 忽略 `src-tauri/resources/models/translation/`
整个目录，翻译模型二进制不入库。

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

开发模式日志输出到控制台。生产模式日志写入：%APPDATA%\com.vtrans.app\logs\

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
| VTRANS_CONFIG_DIR | 配置目录覆盖 | 系统默认 |
| VTRANS_MODEL_DIR | 模型目录覆盖 | resources/models |
| VTRANS_API_KEY | API 翻译密钥（仅测试） | 无 |

## 9. IDE 配置建议

### VS Code

推荐扩展：rust-analyzer, Tauri, Even Better TOML, Tailwind CSS IntelliSense

.vscode/settings.json 配置：

```json
{
  "rust-analyzer.linkedProjects": ["Cargo.toml"],
  "rust-analyzer.cargo.features": "all"
}
```

## 10. 常见问题

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
