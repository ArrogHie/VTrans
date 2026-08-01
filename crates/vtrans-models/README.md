# vtrans-models

模型管理模块：负责 OCR 与翻译模型的清单定义、SHA-256 完整性校验、路径解析和加载进度状态。

## 1. 模块概述

本模块管理模型文件的生命周期元数据：解析 `manifest.json`、校验模型文件存在且 SHA-256 匹配、把相对路径解析为运行时绝对路径，并保存加载进度状态。
边界：本模块做清单 schema 解析与校验、批量完整性校验、路径解析、加载进度状态存取；不做 ONNX 推理（`vtrans-ocr` / `vtrans-translation`）、不做模型文件下载（`scripts/download_models.ps1`）、不管理模型安装布局（`src-tauri/resources/models/`）。
本模块不持有文件句柄、不启动线程，是纯同步库模块；取消与后台执行由消费方编排。

## 2. 依赖关系

上游 crate：

| crate | 本模块使用的核心概念 |
|-------|----------------------|
| `vtrans-core` | `Language`，用于翻译模型的 `supported_pairs`；依赖其 serde 表示 `"auto"` / `"zh-CN"` / `"ja"` / `"en"` |

外部 crate：

| crate | 用途 |
|-------|------|
| `serde` + `serde_json` | manifest 序列化与反序列化 |
| `thiserror` | `ModelError` 派生 |
| `tracing` | 结构化日志（加载、校验、失败路径） |
| `sha2` | SHA-256 哈希计算 |
| `tempfile`（dev） | 测试用临时模型目录 |

下游消费方（见 `docs/ARCHITECTURE.md` 依赖表）：

| 模块 | 需要本模块提供 |
|------|----------------|
| `vtrans-ocr` | `OcrModelGroup` / `ModelEntry`、`model_path`、`verify_integrity` |
| `vtrans-translation` | `TranslationModelGroup` / `ModelEntry`、`model_path`、`verify_integrity` |
| `vtrans-app` | `ModelManager` 装配入口与校验报告，驱动 `load_local_models` 流程 |

## 3. 快速上手

最小可用示例（自建临时目录并生成 manifest；真实项目由下载脚本准备文件）：

```rust
use std::path::Path;
use sha2::{Digest, Sha256};
use vtrans_models::{ModelError, ModelManager};

fn write_model(dir: &Path, rel: &str, data: &[u8]) -> String {
    let full = dir.join(rel);
    std::fs::create_dir_all(full.parent().expect("path has parent")).unwrap();
    std::fs::write(&full, data).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

fn main() -> Result<(), ModelError> {
    let dir = std::env::temp_dir().join("vtrans-models-demo");
    let det_sha = write_model(&dir, "ocr/det.onnx", b"det-model");
    let rec_ja_sha = write_model(&dir, "ocr/rec_ja.onnx", b"rec-ja-model");
    let rec_en_sha = write_model(&dir, "ocr/rec_en.onnx", b"rec-en-model");
    let manifest = format!(
        r#"{{
  "version": 1,
  "ocr": {{
    "det": {{ "id": "det", "path": "ocr/det.onnx", "sha256": "{det_sha}", "size_bytes": 9 }},
    "rec_ja": {{ "id": "rec-ja", "path": "ocr/rec_ja.onnx", "sha256": "{rec_ja_sha}", "size_bytes": 12 }},
    "rec_en": {{ "id": "rec-en", "path": "ocr/rec_en.onnx", "sha256": "{rec_en_sha}", "size_bytes": 12 }},
    "rec_multi": null,
    "dicts": {{}},
    "preprocess_params": {{ "image_size": [960, 960], "mean": [0.485, 0.456, 0.406], "std": [0.229, 0.224, 0.225], "det_threshold": 0.3, "unclip_ratio": 2.0 }}
  }},
  "translation": null
}}"#
    );
    std::fs::write(dir.join("manifest.json"), manifest).unwrap();
    let manager = ModelManager::from_manifest_dir(&dir)?;
    let report = manager.verify_integrity()?;
    if !report.is_ok() {
        for failure in &report.failed {
            eprintln!("{failure}");
        }
        return Err(ModelError::FileNotFound(Path::new("models").to_path_buf()));
    }
    let det_path = manager.model_path(&manager.manifest().ocr.det);
    println!("det model: {}", det_path.display());
    Ok(())
}
```

生命周期：`ModelManager` 由消费方创建并持有，内部只保存 manifest 与目录路径；`verify_integrity` 逐文件打开、校验后立即关闭，不保留句柄。

## 4. 公开 API 概要

所有类型在 crate 根重新导出，也可从子模块导入（如 `vtrans_models::manifest::ModelEntry`）。

| 类型 | 用途 |
|------|------|
| `ModelManifest` | 根清单：版本 + OCR 组 + 可选翻译组 |
| `OcrModelGroup` / `TranslationModelGroup` | OCR 与翻译模型组 |
| `ModelEntry` | 单个模型条目：id、相对路径、SHA-256、大小 |
| `PreprocessParams` / `InferenceParams` | OCR 预处理与翻译推理参数 |
| `ModelManager` | 加载清单、路径解析、完整性校验、进度状态 |
| `VerifyReport` | 批量校验结果汇总 |
| `ModelError` | 错误枚举 |
| `path::resolve_model_path` / `is_relative` | 路径工具 |
| `verify::verify_entry` | 单个模型条目校验 |

核心类型签名：
```rust
/// 根清单；serde 表示与 manifest.json 一一对应
pub struct ModelManifest {
    pub version: u32,                                // 当前仅支持 1
    pub ocr: OcrModelGroup,
    pub translation: Option<TranslationModelGroup>,  // null 表示无本地翻译
}

/// 单个模型条目；path 相对于模型目录
pub struct ModelEntry { pub id: String, pub path: PathBuf, pub sha256: String, pub size_bytes: u64 }

impl ModelManager {
    pub fn from_manifest_dir(dir: &Path) -> Result<Self, ModelError>;  // 缺失/JSON 错误/版本不支持返回 Err
    pub fn manifest(&self) -> &ModelManifest;
    pub fn verify_integrity(&self) -> Result<VerifyReport, ModelError>;  // 失败汇总在报告里
    pub fn model_path(&self, entry: &ModelEntry) -> PathBuf;             // 不检查文件存在性
    pub fn load_progress(&self) -> Option<f32>;
    pub fn set_load_progress(&mut self, progress: Option<f32>);
}

/// checked = 已检查数，passed = 通过数，failed = 失败描述
pub struct VerifyReport { pub checked: usize, pub passed: usize, pub failed: Vec<String> }

pub enum ModelError {
    ManifestNotFound(PathBuf),              // manifest.json 不存在
    Parse(serde_json::Error),               // JSON 解析失败/字段缺失
    FileNotFound(PathBuf),                  // 模型文件不存在
    HashMismatch { id, expected, actual },  // SHA-256 不匹配
    UnsupportedVersion(u32),                // schema 版本不受支持
    Io(std::io::Error),                     // 文件存在但读取失败
}
```
serde 表示：`ModelManifest` 及其子结构实现 `Serialize` / `Deserialize`；语言对序列化为 JSON 数组（如 `["en","zh-CN"]`），`dicts` 为对象，`image_size` 为 `[960, 960]`。`VerifyReport` 也可序列化，便于跨 IPC 传递校验结果。

## 5. 行为契约

错误语义：`from_manifest_dir` / `from_json_str` 的 `ManifestNotFound` 不可重试（需先放好文件），`Parse`、`Io` 修复后可重试，`UnsupportedVersion` 需要升级或降级 manifest；`verify_integrity` 永远返回 `Ok(report)`，`FileNotFound`、`HashMismatch`、`Io` 都以字符串进入 `report.failed`，消费方应检查 `report.is_ok()`；`HashMismatch` 重下模型后可重试。

并发模型：`ModelManager` 自动实现 `Send + Sync`，无内部锁；多线程并发调用只读方法（`manifest`、`verify_integrity`、`model_path`）安全；`set_load_progress` 需要 `&mut self`，由调用方保证独占。

取消语义：本模块没有异步 API，也不使用 `CancellationToken`；`verify_integrity` 是同步阻塞操作，中途不可取消，大模型目录应放到后台线程或任务中运行。

资源生命周期：`ModelManager` 不持有文件句柄；`verify_integrity` 逐文件打开、校验后立即关闭；drop 无副作用。

边界条件：空目录返回 `ManifestNotFound`；`rec_multi` 与 `translation` 为 `None` 合法；`dicts` 可为空；空文件可以校验；`model_path` 不检查存在性；`set_load_progress` 的范围只在 debug 构建断言。

## 6. 集成注意事项

| 坑 | 正确做法 |
|----|----------|
| `from_manifest_dir` 不会创建 manifest.json，首次运行必失败 | 先确保 `src-tauri/resources/models/manifest.json` 已随应用分发 |
| 模型文件未下载时 `verify_integrity` 报告大量失败 | 先运行 `scripts/download_models.ps1`；把失败项当作下载检查清单 |
| `verify_integrity` 同步阻塞，数百 MB 模型可能耗时数秒 | 在 `tokio::task::spawn_blocking` 或独立线程中调用，避免阻塞 UI |
| 用 `model_path` 判断文件是否可用 | 先 `verify_integrity`，再使用 `model_path` 的结果 |
| 字典 key 是语言代码字符串（`"ja"` / `"en"`） | 与 `vtrans_core::Language::code()` 对齐，不要硬编码中文名 |
| 期待 `verify_integrity` 返回 Err 来处理失败 | 检查 `report.is_ok()` 与 `report.failed`，Err 路径不会发生 |
| 发布构建中依赖 `set_load_progress` 的断言 | 调用方自行保证传入 `[0.0, 1.0]`，或先 clamp |

## 7. 设计决策记录

| 决策 | 理由 | 备选方案 |
|------|------|----------|
| `ModelError` 在规格外增加 `Io(#[from] std::io::Error)` | 文件存在但不可读不能归为 `FileNotFound`，`#[from]` 保持错误链完整 | 把 IO 错误格式化成字符串塞进 `Parse`（丢失错误类型） |
| `verify_integrity` 聚合所有失败而非短路 | 批量校验应一次给出全部失败文件，便于一次性修复下载 | 首个错误即返回（模型多时要反复校验） |
| SHA-256 以 8 KiB 分块流式计算 | 模型文件可达数百 MB，避免整文件载入内存 | `read_to_end` 一次性读入（内存峰值高） |
| 字典文件只校验存在性，不校验哈希 | 规格的 `dicts` 只含路径，无哈希字段 | 为字典引入哈希字段（改动 schema，超出规格） |
| `load_progress` 仅做状态存取，由上层驱动 | 下载/加载由应用层编排，本模块保持同步简单 | 内置异步下载与进度回调（扩大模块职责） |

## 8. 已知限制

| 限制 | 类型 | 缓解/规避 |
|------|------|-----------|
| 仅支持 manifest schema version 1 | 设计使然 | 版本字段保留，后续扩展 `SUPPORTED_MANIFEST_VERSION` |
| 字典文件没有 SHA-256 校验 | 设计使然（规格如此） | 需要强校验时自行对 dicts 文件额外做哈希 |
| `verify_integrity` 串行逐文件校验 | 性能限制 | 大目录可在上层并行调用 `verify_entry` |
| `size_bytes` 不参与校验 | 待优化 | 先做大小预检可跳过明显错误的哈希计算 |
| `load_progress` 不驱动真实下载/加载 | 待后续 Phase | 由 `vtrans-app` 的加载流程写入进度 |
| 无自动下载/修复机制 | 待后续 Phase | 使用 `scripts/download_models.ps1` 完成下载 |
| 未提供验证 CLI 二进制 | 待后续 Phase | 用 `cargo test` 覆盖校验路径，后续可加 `examples/` CLI |

## 9. 构建与测试

```powershell
cargo check -p vtrans-models
cargo test -p vtrans-models
cargo clippy -p vtrans-models --all-targets
cargo fmt -p vtrans-models -- --check
```

测试覆盖：manifest 解析、缺失字段、SHA-256 匹配/不匹配、文件不存在、路径解析、批量校验报告（单元 + 集成 + 文档测试，共 52 个用例）。当前没有独立验证 CLI；下载模型后可通过集成测试验证全流程。
## 10. 详细规格

参见 `docs/modules/08-models.md`。
