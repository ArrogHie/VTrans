# 模块 02：vtrans-config 配置管理

| 属性 | 值 |
|------|-----|
| Crate | `vtrans-config` |
| 分支 | `feat/02-config` |
| 上游依赖 | `vtrans-core` |
| 层级 | 1 |
| 复杂度 | 低 |
| 阶段 | Phase 1 |

## 职责

管理应用配置的 schema 定义、持久化、加载、迁移和默认值。配置以 JSON 格式存储在 Tauri AppConfig 目录下。

## 公开 API

```rust
/// 应用配置根结构
[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub capture: CaptureConfig,
    pub ocr: OcrConfig,
    pub translation: TranslationConfig,
    pub result_window: ResultWindowConfig,
    pub hotkeys: HotkeyConfig,
    pub log_level: String,           // 默认 "info"
    pub model_dir: Option<PathBuf>,  // None = 使用默认路径
    pub version: u32,
}

pub struct CaptureConfig {
    pub interval_ms: u32,          // 默认 500, 范围 250-2000
    pub difference_threshold: f32, // 默认 0.03
}

pub struct OcrConfig {
    pub language: Language,        // 默认 Auto
    pub min_confidence: f32,       // 默认 0.55
}

pub struct TranslationConfig {
    pub provider: String,          // "api" | "local"
    pub source_language: Language, // 默认 Auto
    pub target_language: Language, // 载 ChineseSimplified
    pub timeout_seconds: u32,      // 默认 30
    pub api_endpoint: String,      // 默认 "https://api.openai.com/v1/chat/completions"
    pub api_model: String,         // 默认 "gpt-4o-mini"
    pub max_retries: u32,          // 默认 3
}

pub struct ResultWindowConfig {
    pub always_on_top: bool,       // 默认 true
}

pub struct HotkeyConfig {
    pub select_and_translate: String,  // 默认 "Alt+Shift+A"
    pub live_translate: String,        // 默认 "Alt+Shift+R"
    pub stop_live: String,             // 默认 "Alt+Shift+S"
}

/// 配置管理器
pub struct ConfigManager { /* ... */ }

impl ConfigManager {
    pub fn new(config_dir: &Path) -> Result<Self, ConfigError>;
    pub fn load(&self) -> Result<AppConfig, ConfigError>;
    pub fn save(&self, config: &AppConfig) -> Result<(), ConfigError>;
    pub fn update<F: FnOnce(&mut AppConfig)>(&self, f: F) -> Result<(), ConfigError>;
    pub fn migrate(&self) -> Result<AppConfig, ConfigError>;
}
```

## 错误类型

```rust
[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config file not found: {0}")]
    NotFound(PathBuf),
    #[error("config parse error: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("config validation failed: {0}")]
    Validation(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("unsupported config version: {0}")]
    UnsupportedVersion(u32),
}
```

## 内部文件结构

```text
crates/vtrans-config/
├── Cargo.toml
├── README.md
└── src/
    ├── lib.rs          # re-export
    ├── schema.rs       # AppConfig 及子结构定义
    ├── manager.rs      # ConfigManager 实现
    ├── migration.rs    # 版本迁移逻辑
    ├── defaults.rs     # 默认值实现
    └── validation.rs   # 配置校验规则
```

## 测试计划

| 测试项 | 类型 | 说明 |
|--------|------|------|
| 默认值生成 | 单元 | 所有字段有合理默认值 |
| 序列化往返 | 单元 | to_json -> from_json 一致 |
| 版本迁移 | 单元 | v0 -> v1 字段补全 |
| 范围校验 | 单元 | interval_ms 超范围返回 Validation |
| 文件不存在时创建默认 | 集成 | 首次加载返回默认配置并写入文件 |
| 并发写入安全 | 集成 | 多线程 update 不冲突 |

## 验收标准

- [ ] 配置可加载、保存、更新
- [ ] 缺失字段使用默认值填充
- [ ] 范围违规返回明确错误
- [ ] 单元测试通过
- [ ] README.md 完整

## 开发注意事项

- 配置文件路径：directories::config_dir() / "vtrans" / "config.json"
- 保存时先写临时文件再原子替换，避免写入中断导致损坏
- update 方法内部加锁（RwLock），保证并发安全
- 配置版本号用于未来迁移，当前为 1
