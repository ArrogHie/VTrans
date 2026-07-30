# vtrans-pipeline

流水线编排模块。编排采集、OCR、文本标准化和翻译的完整流程。

## 职责

- Pipeline：单次截屏和实时区域翻译编排
- 帧差检测、有界通道、任务取消
- 文本指纹去重

## 依赖

vtrans-core, vtrans-capture, vtrans-ocr, vtrans-text, vtrans-translation

## 构建

```powershell
cargo build -p vtrans-pipeline
cargo test -p vtrans-pipeline
```

## 详细规格

参见 docs/modules/09-pipeline.md
