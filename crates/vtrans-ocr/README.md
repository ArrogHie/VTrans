# vtrans-ocr

## 1. 模块概述

`vtrans-ocr` 是 VTrans 的 OCR 识别模块：使用 ONNX Runtime 加载 PP-OCR 检测与识别模型，把截图转换为带文字框、置信度和阅读顺序的文本结果。

边界：本模块负责图像预处理、文本检测、文本框排序、透视矫正、文字识别、CTC 解码和文本合并，并实现 `vtrans_core::OcrProvider` trait。本模块不采集屏幕（属于 `vtrans-capture`）、不翻译文本（属于 `vtrans-translation`）、不下载或删除模型文件（属于 `vtrans-models`），也不负责流程编排与 UI 事件（属于 `vtrans-pipeline` / `vtrans-app`）。

## 2. 依赖关系

| 类型 | 名称 | 使用方式 |
|------|------|----------|
| 上游 crate | `vtrans-core` | 实现 `OcrProvider` trait；使用 `CapturedImage`、`OcrOptions`、`OcrResult`、`OcrError`、`Language`、`ScreenRegion` |
| 上游 crate | `vtrans-models` | 使用 `ModelManifest` / `ModelManager` / `PreprocessParams` 读取模型路径、字典与检测预处理参数；复用 SHA-256 校验 |
| 外部 crate | `ort` | ONNX Runtime 会话与 CPU execution provider |
| 外部 crate | `ndarray` | NCHW 张量构造与输出形状转换 |
| 外部 crate | `image` | RGB 转换、缩放、透视采样 |
| 外部 crate | `tokio` / `tokio-util` | 阻塞推理任务与 `CancellationToken` |
| 外部 crate | `async-trait` / `tracing` | 异步 trait 实现与结构化日志 |

下游消费方：`vtrans-pipeline`（层级 3）通过 `OcrProvider` trait 调用本模块；`vtrans-app` 负责组装具体 provider 实例。消费方需要本模块提供：可构造的 `PaddleOcrProvider`、稳定的异步 `recognize` 调用、可取消的长任务语义。

## 3. 快速上手

以下示例假设模型已通过 `scripts/download_models.ps1` 下载到 `src-tauri/resources/models`，并存在 `manifest.json`。

```rust,no_run
use std::path::PathBuf;

use tokio_util::sync::CancellationToken;
use vtrans_core::OcrProvider;
use vtrans_core::types::{CapturedImage, Language, OcrOptions, PixelFormat, ScreenRegion};
use vtrans_models::ModelManager;
use vtrans_ocr::PaddleOcrProvider;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 加载 manifest 并解析模型相对路径
    let models_dir = PathBuf::from("src-tauri/resources/models");
    let manager = ModelManager::from_manifest_dir(&models_dir)?;

    // 2. 构造 provider：初始化一次 ONNX 会话；manager 之后可以释放
    let provider = PaddleOcrProvider::from_manager(&manager)?;
    drop(manager);

    // 3. 构造输入图像和识别选项（真实图像来自 vtrans-capture）
    let image = CapturedImage::new(64, 32, PixelFormat::Rgba8, vec![255_u8; 64 * 32 * 4])?;
    let region = ScreenRegion::new("display-1", 0, 0, 64, 32);
    let options = OcrOptions {
        language: Language::Japanese,
        min_confidence: 0.55,
        detect_vertical: true,
    };

    // 4. 调用 trait 方法识别；provider 是 Send + Sync，可跨任务共享
    let cancel = CancellationToken::new();
    let result = provider
        .recognize(&image, &region, &options, cancel.clone())
        .await?;
    println!("merged text: {}", result.merged_text);

    // 5. 取消：取消后 recognize 立即返回 OcrError::Cancelled
    cancel.cancel();
    let err = provider
        .recognize(&image, &region, &options, cancel)
        .await
        .err();
    assert!(matches!(err, Some(vtrans_core::OcrError::Cancelled)));
    Ok(())
}
```

生命周期约定：provider 由消费方创建并持有，通常放进 `Arc<PaddleOcrProvider>` 共享；drop 时 ONNX 会话由 `ort` 释放，不需要显式 close。

## 4. 公开 API 概要

| 类型 / 函数 | 用途 |
|-------------|------|
| `PaddleOcrProvider` | 模块唯一公开入口，实现 `OcrProvider` |
| `Detector` / `Recognizer` | 检测 / 识别 ONNX 会话封装，一般由 provider 内部使用 |
| `preprocess::to_rgb` / `rgb_region` | 像素格式转换与 region 裁剪 |
| `preprocess::det_preprocess` / `prepare_rec_input` | 检测 / 识别输入张量预处理 |
| `detect::extract_probability_map` | 将检测输出归一化为 `(H, W)` 概率图 |
| `postprocess::boxes_from_map` | 概率图转文本框（连通域 + unclip） |
| `postprocess::sort_boxes` / `merge_lines` | 阅读顺序排序与多行合并 |
| `postprocess::ctc_greedy_decode` | greedy CTC 解码 |
| `geometry::min_area_rect` / `warp_perspective` / `rotate_90_cw` | 最小外接矩形、透视矫正、旋转 |

核心类型签名：

```rust
pub struct PaddleOcrProvider { /* 私有字段 */ }

impl PaddleOcrProvider {
    /// 从 manifest 加载；相对路径基于当前工作目录，应用代码慎用
    pub fn from_manifest(manifest: &ModelManifest) -> Result<Self, OcrError>;
    /// 从 manifest + 模型目录加载；加载前校验 OCR 模型 SHA-256 与字典存在性
    pub fn from_manifest_dir(manifest: &ModelManifest, models_dir: &Path) -> Result<Self, OcrError>;
    /// 从 ModelManager 加载；只校验 OCR 组条目
    pub fn from_manager(manager: &ModelManager) -> Result<Self, OcrError>;
}

impl vtrans_core::OcrProvider for PaddleOcrProvider {
    fn id(&self) -> &'static str;
    async fn recognize(
        &self,
        image: &CapturedImage,
        region: &ScreenRegion,
        options: &OcrOptions,
        cancel: CancellationToken,
    ) -> Result<OcrResult, OcrError>;
    fn supported_languages(&self) -> &[Language];
}
```

serde 表示：`OcrResult`、`OcrLine`、`OcrOptions` 等来自 `vtrans-core`，可 JSON 序列化（IPC 用）；`CapturedImage` 不实现 `Serialize`，图像数据不跨边界传输。完整规格见 `docs/modules/05-ocr.md`。

## 5. 行为契约

- 错误语义：`from_manifest_dir` / `from_manager` 失败表示模型文件缺失、SHA-256 不匹配或 ONNX 无法加载，修复文件前重试无意义；`recognize` 失败可能是输入或模型输出格式问题，修正后可重试；`OcrError::Cancelled` 表示调用方主动取消，重试需重新发起请求。错误类型统一为 `vtrans_core::OcrError`。
- 并发模型：`PaddleOcrProvider` 是 `Send + Sync`；内部 `Detector` / `Recognizer` 用 `Mutex<Session>` 串行推理，多线程并发调用安全但会排队。
- 取消语义：`recognize` 在预处理、检测前后、每行识别前检查令牌；正在执行的 ONNX run 通过 `RunOptions::terminate()` 中止，返回 `OcrError::Cancelled`。已取消的令牌复用会立即返回 `Cancelled`。
- 资源生命周期：ONNX 会话由 provider 持有，drop 时由 `ort` 释放；模型文件由 `vtrans-models` 管理，本模块不下载、不删除；`ModelManager` 在 `from_manager` 之后可以 drop。
- 边界条件：零尺寸、负偏移或完全在图像外的 region 返回 `OcrError::Preprocess`；空图像返回 `Preprocess`；检测不到文本返回 `OcrResult::empty()`（`lines` 为空、`merged_text` 为空串、`detected_language` 为 `None`）；大图按 manifest `image_size` 缩放，内存峰值受模型输入限制。

## 6. 集成注意事项

| 坑 | 正确做法 |
|----|----------|
| 不导入 trait 直接调用 `recognize`，报 method not found | 在调用处 `use vtrans_core::OcrProvider;` |
| `from_manifest` 依赖进程当前工作目录，运行时路径飘移 | 应用使用 `from_manifest_dir` 或 `from_manager` |
| 构造 provider 会同步加载并优化模型（秒级），阻塞 UI | 在应用启动的后台任务中构造一次，用 `Arc<PaddleOcrProvider>` 共享 |
| `CapturedImage` 不实现 `Serialize`，无法通过 Tauri IPC 传输 | 只传输文本、状态或缩略图，图像留在 Rust 侧 |
| capture 已按 region 裁剪后，仍传入带偏移的 region，导致二次裁剪 | 传入 `(0, 0, image.width, image.height)` 或与图像一致的 region |
| 误以为 `from_manager` 会校验全部模型（含翻译模型） | 需要全量校验时，在应用层先调用 `manager.verify_integrity()` |

## 7. 设计决策记录

| 决策 | 理由 | 备选方案 |
|------|------|----------|
| `OcrError` 从 `vtrans-core` 导入 | `OcrProvider` trait 签名引用 core 错误，跨 crate 保持一致 | 各 crate 自建错误（trait 无法编译） |
| ONNX `Session` 用 `Mutex` 串行推理 | `ort` 2.0 的 `Session::run` 需要 `&mut self`，底层非线程安全 | 每线程独立 session（内存翻倍） |
| 取消时调用 `RunOptions::terminate()` | 能真正中断正在执行的 ONNX run | 仅阶段间协作检查（长推理无法中止） |
| 竖排文本旋转 90° 后识别 | 不引入方向分类器，MVP 可运行 | PP-OCR direction classifier（额外模型与依赖） |
| 混合横竖排按方向分组排序 | 横排 / 竖排各自保持阅读顺序，组间按版面位置决定先后 | 多数派统一方向（混排场景顺序错误） |
| `min_box_area` 提取为 `DEFAULT_MIN_BOX_AREA` 常量 | manifest schema 冻结时无该字段，先代码常量并注明来源 | 直接改 schema（需变更评审） |

## 8. 已知限制

待后续 Phase：

| 限制 | 影响 | 缓解方式 |
|------|------|----------|
| 未实现方向分类器（PP-OCR 完整流程含 cls） | 竖排质量依赖 90° 旋转，倾斜文本可能误判 | 后续按规格增加 cls 模型或优化旋转判定 |
| 使用 greedy CTC，未实现 beam search | 长文本识别正确率上限较低 | 后续 Phase 按需实现 beam search |
| 识别预处理参数（32 高 / 320 宽 / mean 0.5 / std 0.5）硬编码 | 更换 rec 模型版本时参数可能不匹配 | 走 manifest schema 变更评审，加入 rec 参数 |

设计使然：

| 限制 | 影响 | 缓解方式 |
|------|------|----------|
| 推理串行 | 同一 provider 的并发识别请求排队 | 减少并发请求，或按模型拆分实例 |
| `from_manager` 只校验 OCR 组 | 翻译模型缺失不影响 OCR 构造 | 需要全量校验时应用层自行调用 |
| 混排排序为启发式 | 复杂版式阅读顺序仍可能不完美 | 接受质量回归，或后续引入版面分析 |

## 9. 构建与测试

```powershell
cargo check -p vtrans-ocr
cargo test -p vtrans-ocr
cargo clippy -p vtrans-ocr --all-targets
cargo fmt -p vtrans-ocr -- --check
```

验证 CLI（需要已下载的模型文件）：

```powershell
cargo run --example ocr_verify -- `
  --models src-tauri/resources/models `
  --image crates/vtrans-ocr/tests/fixtures/sample_text.png `
  --language ja
```

## 详细规格
参见 `docs/modules/05-ocr.md`。