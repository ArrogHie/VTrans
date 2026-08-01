# vtrans-ocr

OCR 识别模块：使用 ONNX Runtime 加载 PP-OCR 检测与识别模型，把屏幕截图
转换为带置信度、文字框和阅读顺序的文本结果。

## 模块职责

- 实现 `vtrans_core::OcrProvider` trait（`PaddleOcrProvider`）。
- 文本检测（PP-OCR det ONNX）、文本框排序、透视矫正、文字识别、CTC 解码。
- 横排文本按行/列排序，竖排日文按从右到左、从上到下排序。
- 模型加载时校验 SHA-256，并保证会话只初始化一次。

## 依赖关系

### 上游 crate

- `vtrans-core`：`OcrProvider` trait、`CapturedImage`、`OcrOptions`、
  `OcrResult`、`OcrError`、`Language` 等共享类型。
- `vtrans-models`：`ModelManifest`、`ModelManager`、`PreprocessParams`、
  SHA-256 完整性校验。

### 外部 crate

| crate | 用途 | 许可证 |
|-------|------|--------|
| `ort` | ONNX Runtime 推理 | MIT |
| `ndarray` | 张量构造与形状转换 | MIT/Apache-2.0 |
| `image` | RGB 转换、缩放、图像读写 | MIT/Apache-2.0 |
| `tokio` / `tokio-util` | 异步 trait 与取消令牌 | MIT |
| `async-trait` | 异步 trait 实现 | MIT/Apache-2.0 |
| `tracing` | 结构化日志 | MIT |

## 公开 API 概要

```rust
pub struct PaddleOcrProvider { /* ... */ }

impl PaddleOcrProvider {
    pub fn from_manifest(manifest: &ModelManifest) -> Result<Self, OcrError>;
    pub fn from_manifest_dir(manifest: &ModelManifest, models_dir: &Path)
        -> Result<Self, OcrError>;
    pub fn from_manager(manager: &ModelManager) -> Result<Self, OcrError>;
}

impl OcrProvider for PaddleOcrProvider {
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

模块内部还提供可复用的纯逻辑函数：

- `preprocess::det_preprocess` / `prepare_rec_input`
- `detect::extract_probability_map`
- `postprocess::boxes_from_map` / `sort_boxes` / `ctc_greedy_decode`
- `geometry::warp_perspective` / `min_area_rect` / `offset_polygon`

## 构建与测试

```powershell
cargo build -p vtrans-ocr
cargo test -p vtrans-ocr
cargo clippy -p vtrans-ocr --all-targets
cargo fmt --all -- --check
```

验证 CLI（需要已下载的 ONNX 模型和 manifest）：

```powershell
cargo run --example ocr_verify -- `
  --models src-tauri/resources/models `
  --image crates/vtrans-ocr/tests/fixtures/sample_text.png `
  --language ja
```

模型文件不提交 Git，通过 `src-tauri/resources/models/manifest.json` 和
`scripts/download_models.ps1` 管理。

## 已知限制

- `from_manifest` 按当前工作目录解析相对路径；应用代码应优先使用
  `from_manifest_dir` 或 `from_manager`。
- 识别模型预处理参数（32 高、最大 320 宽、mean/std 0.5）使用 PP-OCR
  标准值；`vtrans-models` 的 manifest schema 目前只提供检测模型参数。
- 未实现方向分类器；竖排文本会旋转 90° 后送入识别模型，质量可后续优化。
- `Language::Auto` 在存在 `rec_multi` 时使用多语言模型，否则回退日文模型。
- 当前使用 greedy CTC 解码，未实现 beam search。
- 单次推理串行化（ONNX `Session::run` 需要独占访问），同一 provider 的
  并发识别请求会排队。

## 详细规格

参见 `docs/modules/05-ocr.md` 和 `docs/ARCHITECTURE.md`。
