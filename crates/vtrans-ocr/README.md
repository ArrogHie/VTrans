# vtrans-ocr

OCR 识别模块。使用 ONNX Runtime 加载 PP-OCR 模型完成文本检测和识别。

## 职责

- PaddleOcrProvider：实现 OcrProvider trait
- 文本检测、识别、CTC 解码
- 横排/竖排支持、预处理/后处理

## 依赖

vtrans-core, vtrans-models

## 构建

```powershell
cargo build -p vtrans-ocr
cargo test -p vtrans-ocr
cargo run --example ocr_verify
```

## 详细规格

参见 docs/modules/05-ocr.md
