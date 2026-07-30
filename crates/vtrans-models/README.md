# vtrans-models

模型管理模块。管理 OCR 和翻译模型的清单定义、完整性校验和生命周期。

## 职责

- ModelManifest：OCR 和翻译模型的清单 schema
- ModelManager：加载 manifest、解析路径、校验完整性
- SHA-256 校验

## 依赖

vtrans-core

## 构建

```powershell
cargo build -p vtrans-models
cargo test -p vtrans-models
```

## 详细规格

参见 docs/modules/08-models.md
