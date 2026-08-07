# vtrans-ocr

## 1. 模块概述

`vtrans-ocr` 是 VTrans 的 OCR 识别模块：使用 ONNX Runtime 加载 PP-OCRv6 Small 检测与识别模型，把截图转换为带文字框、置信度和阅读顺序的文本结果。

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
| `preprocess::det_preprocess` / `prepare_rec_input` | 检测 / 识别输入张量预处理（BGR 通道序、48 高识别输入） |
| `detect::extract_probability_map` | 将检测输出归一化为 `(H, W)` 概率图 |
| `postprocess::boxes_from_map` | 概率图转文本框（连通域 + unclip） |
| `postprocess::sort_boxes` / `merge_lines` | 阅读顺序排序与多行合并 |
| `postprocess::ctc_greedy_decode` | greedy CTC 解码 |
| `geometry::min_area_rect` / `warp_perspective` / `rotate_90_cw` | 最小外接矩形、透视矫正、旋转 |

核心类型签名：

```rust
pub struct PaddleOcrProvider { /* 私有字段 */ }

impl PaddleOcrProvider {
    pub fn from_manifest(manifest: &ModelManifest) -> Result<Self, OcrError>;
    pub fn from_manifest_dir(manifest: &ModelManifest, models_dir: &Path) -> Result<Self, OcrError>;
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
- 语言路由：`Language::Auto` 在 manifest 配置了 `rec_multi` 多语言识别模型时可用（PP-OCRv6 统一后 `rec_multi` 指向 v6 rec，`auto` / `zh-CN` 均已解锁）；未配置 `rec_multi` 时 `recognize` 返回 `OcrError::Inference`（提示语：`auto language detection requires a multi-language recognition model; please select a language manually`），**不会**静默回退到日文模型。显式 `ja` / `en` / `zh-CN` 行为不变。该错误经 `vtrans-pipeline` 的 `PipelineError` → `vtrans-app` 的 `pipeline_error` 事件展示到前端，无需前端改动。
- 长行识别：PP-OCRv6 rec ONNX 输入是动态宽度 `[N,3,48,W]`，识别预处理按 manifest `rec_input_height`（48）等比缩放裁剪图后以**自然宽度单次推理**，不再压缩、不再分片，长句（60+ 字符）完整识别且无接缝伪影。仅当裁剪图宽度超过 3200px（约 4K 全宽行的病态场景，输出张量约 30MB）时才回退到 320px 重叠分片。
- 通道顺序：PP-OCR 模型按 OpenCV 风格 BGR 训练，检测与识别张量均以 BGR 通道序写入（manifest `mean` / `std` 按 B,G,R 索引）。该决策经 Python 基准逐文件对照验证：Rust 检测输入与 `det_input.npy` 最大绝对误差 0.018（cv2 定点舍入残差）。
- 字典：v6 rec 使用 `ppocrv6_dict.txt`（18,708 行）+ blank（index 0）+ 追加空格 = 18,710 类；加载期对模型输出类数做 fail-fast 校验（错误信息含输出 shape、字典行数、append_space、blank index、字典路径）。
- 模型共享：v6 manifest 中 `rec_ja` / `rec_en` / `rec_multi` 指向同一 rec ONNX 与同一字典，provider 只创建一个识别会话并共享（`Arc<Recognizer>`）。
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
| 字典行数与模型输出类数不匹配 | 加载期 fail-fast 报 `InvalidManifest`（含输出 shape / 字典行数 / append_space / blank_index / 字典路径）；v6 模型无内嵌字符表（ort 对缺失属性返回空串），空表会回退 manifest 字典文件 `ppocrv6_dict.txt` |
| 误以为 v6 与 v4 可混用 | v4 已彻底弃用：不保留 v4 分支、不回退、不兼容 v4 专有参数路径；manifest 只引用 v6 模型与字典 |

## 7. 设计决策记录

| 决策 | 理由 | 备选方案 |
|------|------|----------|
| `OcrError` 从 `vtrans-core` 导入 | `OcrProvider` trait 签名引用 core 错误，跨 crate 保持一致 | 各 crate 自建错误（trait 无法编译） |
| ONNX `Session` 用 `Mutex` 串行推理 | `ort` 2.0 的 `Session::run` 需要 `&mut self`，底层非线程安全 | 每线程独立 session（内存翻倍） |
| 取消时调用 `RunOptions::terminate()` | 能真正中断正在执行的 ONNX run | 仅阶段间协作检查（长推理无法中止） |
| 竖排文本旋转 90° 后识别 | 不引入方向分类器，MVP 可运行 | PP-OCR direction classifier（额外模型与依赖） |
| 混合横竖排按方向分组排序 | 横排 / 竖排各自保持阅读顺序，组间按版面位置决定先后 | 多数派统一方向（混排场景顺序错误） |
| DB 后处理参数（box_threshold / max_candidates / min_box_size）进 manifest | v6 指南 §6.1 规定这些值，随模型版本走 manifest 更可维护 | 代码常量（换模型版本需改代码） |
| 长行以动态宽度单次推理（≤3200px），超宽才分片 | v6 rec ONNX 宽度动态；实测单次推理在 320–5120px 宽度范围内文本完整且无接缝伪影，分片反而会在接缝处产生重复字符 | 固定 320 宽压缩（长句失败）；一律分片（接缝伪影）；改 schema 加宽度上限参数（需评审） |
| 检测 / 识别缩放复现 cv2 `INTER_LINEAR` 采样约定 | Python 基准用 cv2 缩放；image crate 的 Triangle 滤波在缩小时做低通平均，字形边缘差异可达 95/255，复现 cv2 约定后检测输入与基准最大误差 0.018 | 使用 image crate 内置滤波（与基准输入不一致） |
| `auto` 无 `rec_multi` 时报错而非回退 `rec_ja` | 静默回退会用日文模型识别英文，产出乱码且调用方无从知晓；报错让用户显式选择语言 | 保持旧回退行为（错误输出无法定位） |

## 8. 已知限制

待后续 Phase：

| 限制 | 影响 | 缓解方式 |
|------|------|----------|
| 未实现方向分类器（PP-OCR 完整流程含 cls） | 竖排质量依赖 90° 旋转，倾斜文本可能误判 | 后续按规格增加 cls 模型或优化旋转判定 |
| 使用 greedy CTC，未实现 beam search | 长文本识别正确率上限较低 | 后续 Phase 按需实现 beam search |
| 超过 3200px 的超宽行回退到 320px 重叠分片 | 4K 全宽行等病态场景可能因接缝出现个别字符误识别 | 后续优化分片去重，或提高单次推理宽度上限并重新验收 |
| 日文 / 竖排质量未专项验收 | v6 通用模型对日文与竖排无官方精度承诺；本次验收只覆盖英文固定测试集与中文（auto / zh-CN），竖排仅保证不崩溃 | 后续按需建立日文 / 竖排专项测试集 |
| PP-OCRv4 已弃用 | 不保留 v4 模型与回退路径，无 v4 对比基线 | — |

设计使然：

| 限制 | 影响 | 缓解方式 |
|------|------|----------|
| 推理串行 | 同一 provider 的并发识别请求排队 | 减少并发请求，或按模型拆分实例 |
| `from_manager` 只校验 OCR 组 | 翻译模型缺失不影响 OCR 构造 | 需要全量校验时应用层自行调用 |
| 混排排序为启发式 | 复杂版式阅读顺序仍可能不完美 | 接受质量回归，或后续引入版面分析 |
| v6 模型无内嵌字符表 | 字符表由 manifest 字典文件提供（`ppocrv6_dict.txt`）；若 ONNX 输出类数与字典行数约定不一致，加载期 fail-fast | 为这类模型单独导出匹配字典 |

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
