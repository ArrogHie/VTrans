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
    pub provider: String,          // "openai" | "deepl" | "google" | "azure" | "baidu" | "local"，默认 "openai"
    pub region: Option<String>,    // Azure 区域（可选，非敏感）
    pub app_id: Option<String>,    // 百度 APP ID（可选，非敏感；baidu Provider 必填）
    pub quality: String,           // "fast" | "balanced"，默认 "fast"
    pub source_language: Language, // 默认 Auto
    pub target_language: Language, // 默认 ChineseSimplified
    pub timeout_seconds: u32,      // 默认 30
    pub api_endpoint: String,      // 默认 "https://api.openai.com/v1/chat/completions"
    pub api_model: String,         // 默认 "gpt-4o-mini"；仅 OpenAI 必填
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
| 版本迁移 | 单元 | v0 -> v5 迁移链完整；v3 -> v4 补 `quality`、同步 `source_language = ocr.language`；v4 -> v5 `api -> openai` 重命名 + 补 `region`/`app_id`；v5 重复迁移幂等 |
| 范围校验 | 单元 | interval_ms 超范围返回 Validation |
| 质量档位校验 | 单元 | `"fast"` / `"balanced"` 接受，非法值拒绝 |
| 跨字段一致性校验 | 单元 | `ocr.language != translation.source_language` 拒绝，一致接受 |
| Provider 白名单校验 | 单元 | 6 个合法 id 接受；`"api"` / 未知 id 拒绝 |
| Provider 字段必需性校验 | 单元 | `api_model` 仅 openai 必填；`region` 非空；`app_id` 仅 baidu 必填；local 忽略全部 |
| 文件不存在时创建默认 | 集成 | 首次加载返回默认配置并写入文件 |
| 并发写入安全 | 集成 | 多线程 update 不冲突 |

## 验收标准

- [x] 配置可加载、保存、更新
- [x] 缺失字段使用默认值填充（含 `translation.quality` 缺省补 `"fast"`）
- [x] `translation.region` / `translation.app_id` 缺省补 `None`，序列化往返
- [x] 范围违规返回明确错误
- [x] `translation.quality` 非法值拒绝、`"fast"` / `"balanced"` 接受且序列化往返
- [x] v3 配置（含不一致的 `ocr.language` / `source_language`）迁移后两字段一致、`quality == "fast"`；v4 重复迁移无副作用
- [x] v4 配置 `provider == "api"` 迁移后为 `"openai"`、版本 5、`region`/`app_id` 补 `None`；v5 重复迁移无副作用
- [x] `ocr.language != translation.source_language` 拒绝保存；一致接受；`AppConfig::default()` 恒通过
- [x] provider 白名单与各 provider 字段必需性校验通过新增测试（默认 provider 为 `openai`）
- [x] 单元测试通过（`cargo test -p vtrans-config` 全绿）
- [x] README.md 完整

## 开发注意事项

- 配置文件路径：directories::config_dir() / "vtrans" / "config.json"
- 保存时先写临时文件再原子替换，避免写入中断导致损坏
- update 方法内部加锁（RwLock），保证并发安全
- 配置版本号用于未来迁移，当前为 5
- API Key / Secret 不落配置（走 `vtrans-security` 凭据库）；`app_id`（百度 APP ID）与 `region`（Azure 区域）为非敏感字段，可存配置

## 增量记录

### v4 增量：翻译质量档位 + 语言统一（分支 `feat/02-new-translate-model`）

对应功能计划 `docs/feature-plans/new-translate-model/PLAN.md`（本地翻译模型升级 en→zh / ja→zh，OCR 语言与翻译源语言强制统一）。

- schema：`TranslationConfig` 新增 `quality: String`，`#[serde(default = "fast")]`，合法值 `"fast" | "balanced"`。
- 迁移：`CURRENT_CONFIG_VERSION` 3 → 4。`migrate_v3_to_v4` 补齐缺省的 `translation.quality`（由 serde default 兜底）并强制 `translation.source_language = ocr.language`（OCR 语言为权威）；迁移幂等，v2→v3→v4 迁移链完整。
- 校验：`ocr.language != translation.source_language` 返回 `ConfigError::Validation`（错误信息提示两字段必须一致，并说明通过 `set_ocr_language` / `set_source_language` 任一联动命令修改即可保持两字段同步）；`quality` 非法值同样返回 `ConfigError::Validation`。
- 行为变更：`AppConfig::validate()` 拒绝保存「OCR 语言与源语言不一致」的配置；旧 v3 配置在加载时自动同步后放行。

### v5 增量：云端多 Provider 配置（分支 `feat/02-cloud-provider-config`）

对应功能计划 `docs/feature-plans/cloud-api-integration/PLAN.md`（多云端翻译 API 接入：OpenAI / DeepL / Google / Azure / 百度）。

- schema：`TranslationConfig` 新增 `region: Option<String>`（Azure 区域，`#[serde(default)]` 补 `None`）与 `app_id: Option<String>`（百度 APP ID，`#[serde(default)]` 补 `None`）；`provider` 白名单扩展为 `["openai","deepl","google","azure","baidu","local"]`，默认 provider 改为 `"openai"`。
- 迁移：`CURRENT_CONFIG_VERSION` 4 → 5。`migrate_v4_to_v5` 将旧 `provider == "api"` 重命名为 `"openai"`；`region` / `app_id` 由 serde default 补 `None`（文件中显式值保留）；迁移幂等。
- 校验：`validate_translation` 按 provider 校验字段必需性——所有云端 Provider 要求 `api_endpoint` 为 `http(s)://`；`api_model` 仅 `openai` 必填（DeepL / Google 可选，Azure / 百度可空）；`region` 出现时必须非空（Azure 可选）；`app_id` 仅 `baidu` 必填；`local` 忽略 endpoint / model / region / app_id。`auto` 目标语言仍拒绝；语言联动规则不变。
- 行为变更：`AppConfig::validate()` 拒绝 `"api"` 旧 id、未知 provider 与缺失必需字段的配置；旧 v4 配置在加载时自动完成 `api -> openai` 重命名。
