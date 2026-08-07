# 模块 05：vtrans-ocr OCR 识别

| 属性 | 值 |
|------|-----|
| Crate | `vtrans-ocr` |
| 分支 | `feat/05-ppocrv6-ocr` |
| 上游依赖 | `vtrans-core`, `vtrans-models` |
| 层级 | 2 |
| 复杂度 | 高 |
| 阶段 | Phase 2（PP-OCRv6 升级，v4 已弃用） |

## 职责

实现 OcrProvider trait，使用 ONNX Runtime 加载 PP-OCRv6 Small 检测与识别模型完成文本检测和识别。支持英文、日文与简体中文（auto / zh-CN 走统一 rec 槽位）。保留文字框、文本、置信度和阅读顺序。

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

长行处理：识别预处理按 manifest `rec_input_height`（v6 为 48）等比缩放，利用 rec ONNX 的
动态宽度输入以自然宽度单次推理（实测 320–5120px 宽度内文本完整、无接缝伪影）；仅当宽度
超过 3200px（约 4K 全宽行，输出张量约 30MB）时回退到每片 ≤320px、16px 重叠的分片识别。
60+ 字符长句可完整识别。

语言路由：`Language::Auto` 仅在 manifest 配置 `rec_multi` 多语言模型时可用；未配置时
`recognize` 返回 `OcrError::Inference`（提示语：`auto language detection requires a
multi-language recognition model; please select a language manually`），不静默回退
`rec_ja`。PP-OCRv6 manifest 已将 `rec_ja` / `rec_en` / `rec_multi` 统一指向同一 v6 rec，
`auto` / `zh-CN` 均已解锁。显式 `ja` / `en` / `zh-CN` 行为不变。

通道顺序：PP-OCR 模型按 BGR 训练（指南 §6.3），检测与识别张量均以 BGR 通道序写入；
manifest `mean` / `std` 按 B,G,R 索引。缩放复现 cv2 `INTER_LINEAR` 采样约定，检测输入与
Python 基准 `det_input.npy` 最大绝对误差 0.018（cv2 定点舍入残差）。

字典与类数：v6 rec 使用 `ppocrv6_dict.txt`（18,708 行）+ blank（index 0）+ 追加空格 =
18,710 类；加载期对输出类数 fail-fast 校验（错误信息含输出 shape、字典行数、append_space、
blank index、字典路径），v6 ONNX 无内嵌字符表（ort 对缺失属性返回空串），空表回退字典文件。

## 错误类型

> **定义位置**：`OcrError` 定义在 `vtrans-core` 中（因为 `OcrProvider` trait 需要引用它）。本模块从 `vtrans-core` 导入，不重新定义。

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
| 语言选择路由 | 单元 | `auto` 无 multi → `OcrError::Inference`；`auto` 有 multi → multi；`en`/`ja`/`zh-CN` 显式路由正确 |
| 长行分片 | 单元 | 超宽图像按 ≤320px 分片、重叠量与拼接去重正确 |
| 新参数解析 | 单元 | box_threshold / max_candidates / min_box_size / rec 参数默认值与传递 |
| DB 过滤 | 单元 | box_threshold 分数过滤、max_candidates 上限、min_box_size 最短边过滤 |
| 识别预处理 | 单元 | 48 高形状、固定 320 补零、BGR 通道序 |
| 类数校验 | 单元 | 输出类数与字典不一致时 fail-fast，错误信息含全部诊断字段 |
| 语言选择路由 | 单元 | `auto` 无 multi → `OcrError::Inference`；`auto` 有 multi → multi；`en`/`ja`/`zh-CN` 显式路由正确 |
| 长行识别回归 | 集成（需模型） | `tests/long_line_regression.rs`（默认 ignore）对 `test1_lines.png` 断言长句完整；`test1_zh.png` 断言 auto / zh-CN 中文正确；竖排不崩溃 |
| 验证 CLI | 手动 | examples/ocr_verify 对测试图片输出正确文本；`--dump-det-input` 导出张量供 §14 对照 |

## 验收标准

- [x] 可加载 v6 检测 / 识别模型（含 SHA-256 校验与类数一致性检查）
- [x] 对清晰英文图片输出正确文本（固定测试集 `test1_lines.png` 与 `test1_lines.txt` 断言）
- [x] `auto` / `zh-CN` 识别中文清晰文字（`test1_zh.png`）
- [x] 长行（60+ 字符）完整识别，无截断
- [x] 竖排文字至少不崩溃（质量不承诺，登记为已知限制）
- [x] 单次 / 实时链路、语言切换无回归（workspace 验证）
- [x] 仓库内脚本可复现「下载 → 转换 ONNX → 检查 → Python 基准 → 回填 manifest」全流程（`scripts/ppocrv6/`）
- [x] 文档同步（README、模块文档、集成报告已知限制）
- [ ] 模型只初始化一次
- [ ] 默认 CPU 推理
- [ ] 首次启动检查模型 SHA-256
- [ ] 验证 CLI 可运行
- [ ] README.md 完整

## 开发注意事项

- 模型、字典、预处理参数通过 vtrans-models 的 manifest 配置，不写死在代码中（`PreprocessParams` 扩展字段带 serde default）
- ort crate 加载 ONNX，设置 CPU execution provider
- 检测模型输出后需要 threshold、box_threshold、max_candidates、min_box_size、unclip 操作（参数来自 manifest）
- 识别模型的字典路径在 manifest 中指定；输出类数加载期 fail-fast 校验
- CTC 解码使用 greedy 或 beam search（MVP 用 greedy）
- 验证 CLI（examples/ocr_verify.rs）独立运行，不依赖 Tauri
- 日志记录模型加载耗时、推理耗时、识别行数（不记录完整文本，引用用 `truncate_for_log`）
- 通道顺序为 BGR（指南 §6.3，经 Python 基准逐文件对照确认）；禁止凭 ImageNet 直觉改为 RGB
