# vtrans-core

VTrans 核心类型与接口模块。定义全项目共享的数据结构、Provider trait、错误类型和日志初始化工具。

## 职责

- 核心数据结构：Language, ScreenRegion, CapturedImage, OcrResult, TranslationRequest, TranslationResult 等
- Provider trait：OcrProvider, TranslationProvider, CaptureSource, CaptureSession
- 错误类型：CoreError
- 日志初始化：init_logging()

## 依赖关系

无内部 crate 依赖。所有其他 vtrans-* crate 依赖本 crate。

## 构建

```powershell
cargo build -p vtrans-core
cargo test -p vtrans-core
cargo clippy -p vtrans-core
```

## 详细规格

参见 docs/modules/01-core.md
