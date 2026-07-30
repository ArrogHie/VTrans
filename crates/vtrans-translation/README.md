# vtrans-translation

翻译引擎模块。提供 API 翻译和本地 ONNX 翻译两种实现。

## 职责

- ApiTranslationProvider：HTTP/JSON API 翻译
- LocalTranslationProvider：本地 ONNX 翻译
- 取消、超时、重试、语言对校验

## 依赖

vtrans-core, vtrans-security, vtrans-models

## 构建

```powershell
cargo build -p vtrans-translation
cargo test -p vtrans-translation
cargo run --example translation_verify
```

## 详细规格

参见 docs/modules/07-translation.md
