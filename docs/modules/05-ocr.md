# 模块 05：vtrans-ocr OCR 识别

| 属性 | 值 |
|------|-----|
| Crate | `vtrans-ocr` |
| 分支 | `feat/05-ocr` |
| 上游依赖 | `vtrans-core`, `vtrans-models` |
| 层级 | 2 |
| 复杂度 | 高 |
| 阶段 | Phase 2 |

## 职责

实现 OcrProvider trait，使用 ONNX Runtime 加载 PP-OCR 模型完成文本检测和识别。支持日文横排、竖排和英文识别。保留文字框、文本、置信度和阅读顺序。

## 公开 API

实现 `vtrans_core::OcrProvider` trait。

```rust
/// PP-OCR ONNX 识别器
pub struct PaddleOcrProvider { /* ... */ }

impl PaddleOcrProvider {
    /// 从模型清单加载检测模型和识别模型
    pub fn from_manifest(manifest: &ModelManifest) -> Result<Self, OcrError>;
}
```

## OCR 流程

```text
图像裁剪
-> 缩放与归一化 (preprocess)
-> 文本检测 (det model)
-> 文本框排序 (postprocess)
-> 透视裁剪/旋转
-> 文字识别 (rec model)
-> CTC 解码
-> 置信度过滤
-> 合并文本行
-> OcrResult
```

## 错误类型

```rust
[derive(Debug, thiserror::Error)]
pub enum OcrError {
    #[error("model load failed: {0}")]
    ModelLoad(String),
    #[error("inference failed: {0}")]
    Inference(String),
    #[error("preprocess failed: {0}")]
    Preprocess(String),
    #[error("postprocess failed: {0}")]
    Postprocess(String),
    #[error("model manifest invalid: {0}")]
    InvalidManifest(String),
    #[error("cancelled")]
    Cancelled,
    #[error("ort runtime error: {0}")]
    OrtRuntime(String),
}
```

## 内部文件结构

```text
crates/vtrans-ocr/
├── Cargo.toml
├── README.md
├── src/
│   ├── lib.rs              # re-export, PaddleOcrProvider
│   ├── provider.rs         # OcrProvider impl
│   ├── preprocess.rs       # 图像缩放、归一化
│   ├── detect.rs            # 文本检测模型推理
│   ├── recognize.rs        # 文字识别模型推理
│   ├── postprocess.rs       # 框排序、CTC 解码、合并
│   └── geometry.rs         # 透视变换、裁剪、旋转
├── examples/
│   └── ocr_verify.rs       # 独立验证 CLI
└── tests/
    └── fixtures/
```

## 测试计划

| 测试项 | 类型 | 说明 |
|--------|------|------|
| 归一化参数 | 单元 | 输入图像正确缩放到模型期望尺寸 |
| 框排序逻辑 | 单元 | 横排从左到右、从上到下 |
| 竖排排序 | 单元 | 竖排从上到下、从右到左 |
| CTC 解码 | 单元 | 合并重复字符、去 blank |
| 置信度过滤 | 单元 | 低于阈值的行被丢弃 |
| 文本合并 | 单元 | 多行合并为段落，保留换行 |
| 验证 CLI | 手动 | examples/ocr_verify 对测试图片输出正确文本 |

## 验收标准

- [ ] 可加载检测模型和识别模型
- [ ] 对清晰日文横排图片输出正确文本
- [ ] 对清晰英文图片输出正确文本
- [ ] 支持竖排日文（至少不崩溃，质量可后续优化）
- [ ] 模型只初始化一次
- [ ] 默认 CPU 推理
- [ ] 首次启动检查模型 SHA-256
- [ ] 验证 CLI 可运行
- [ ] README.md 完整

## 开发注意事项

- 模型、字典、预处理参数通过 vtrans-models 的 manifest 配置，不写死在代码中
- ort crate 加载 ONNX，设置 CPU execution provider
- 检测模型输出后需要 box threshold 和 unclip 操作
- 识别模型的字典路径在 manifest 中指定
- CTC 解码使用 greedy 或 beam search（MVP 用 greedy）
- 验证 CLI（examples/ocr_verify.rs）独立运行，不依赖 Tauri
- 日志记录模型加载耗时、推理耗时、识别行数（不记录完整文本）
