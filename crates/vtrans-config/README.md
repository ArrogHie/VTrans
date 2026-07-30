# vtrans-config

配置管理模块。定义应用配置 schema、持久化、迁移和默认值。

## 职责

- AppConfig 及子结构定义（CaptureConfig, OcrConfig, TranslationConfig, ResultWindowConfig）
- ConfigManager：加载、保存、更新配置
- 版本迁移和字段校验

## 依赖

vtrans-core

## 构建

```powershell
cargo build -p vtrans-config
cargo test -p vtrans-config
```

## 详细规格

参见 docs/modules/02-config.md
