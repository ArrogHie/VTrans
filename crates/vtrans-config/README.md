# vtrans-config

VTrans 配置管理模块：定义应用配置的 schema、默认值、校验、版本迁移与持久化。

## 模块职责

管理应用配置的 schema 定义、持久化、加载、迁移和默认值。配置以 JSON 格式存储在 `directories::config_dir()/vtrans/config.json`，通过「临时文件 + 原子重命名」保证写入中断不会损坏配置。

## 依赖

- 上游 crate：`vtrans-core`（`Language` 等核心类型）
- 外部 crate：`serde`、`serde_json`、`thiserror`、`tracing`、`directories`
- dev-dependencies：`tempfile`、`pretty_assertions`

## 公开 API 概要

### 类型

| 类型 | 说明 |
|------|------|
| `AppConfig` | 配置根结构（capture / ocr / translation / result_window / hotkeys / log_level / model_dir / version） |
| `CaptureConfig` | 屏幕采集设置（interval_ms、difference_threshold） |
| `OcrConfig` | OCR 设置（language、min_confidence） |
| `TranslationConfig` | 翻译引擎设置（provider、source/target_language、timeout_seconds、api_endpoint、api_model、max_retries） |
| `ResultWindowConfig` | 结果窗口设置（always_on_top） |
| `HotkeyConfig` | 全局快捷键（select_and_translate、live_translate、stop_live） |
| `ConfigManager` | 配置管理器 |
| `ConfigError` | 错误枚举（NotFound / Parse / Validation / Io / UnsupportedVersion） |
| `CURRENT_CONFIG_VERSION` | 当前配置版本（`1`） |

### 主要函数签名

```rust
pub struct ConfigManager { /* ... */ }

impl ConfigManager {
    pub fn new(config_dir: &Path) -> Result<Self, ConfigError>;
    pub fn load(&self) -> Result<AppConfig, ConfigError>;
    pub fn save(&self, config: &AppConfig) -> Result<(), ConfigError>;
    pub fn update<F: FnOnce(&mut AppConfig)>(&self, f: F) -> Result<(), ConfigError>;
    pub fn migrate(&self) -> Result<AppConfig, ConfigError>;
    pub fn config_path(&self) -> &Path;
}

pub fn default_config_path() -> Option<PathBuf>;
impl AppConfig {
    pub fn validate(&self) -> Result<(), ConfigError>;
}
```

### 行为约定

- 首次加载（文件不存在）自动创建默认配置并写入文件
- 缺失字段使用默认值填充（`serde` default 属性，单一默认值来源 `defaults.rs`）
- `update` 内部 `RwLock` 串行化读-改-写，多线程调用不丢更新
- 保存前校验，范围违规返回 `ConfigError::Validation`（错误信息含字段路径）
- `version > CURRENT_CONFIG_VERSION` 返回 `ConfigError::UnsupportedVersion`
- 无 `version` 字段的旧文件按版本 0 处理，自动迁移到版本 1
- `update` 要求配置文件已存在（先调用 `load`），否则返回 `ConfigError::NotFound`

## 构建 / 测试

```powershell
cargo build -p vtrans-config
cargo test -p vtrans-config
cargo clippy -p vtrans-config --all-targets
cargo fmt --all -- --check
```

## 已知限制

- 未集成 `VTRANS_CONFIG_DIR` 环境变量覆盖（由应用层负责）
- 校验规则中的取值范围（如 `difference_threshold` 0.0–1.0、`timeout_seconds` 1–3600、`max_retries` 0–10）为工程合理默认值，可在评审时调整
- 配置文件中不保存 API Key（由 `vtrans-security` 模块管理）
- 不支持配置热重载（由应用层监听文件变化）

## 详细规格

参见 docs/modules/02-config.md
