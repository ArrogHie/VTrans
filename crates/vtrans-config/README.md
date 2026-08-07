# vtrans-config

## 1. 模块概述

管理应用配置的 schema 定义、默认值、校验、版本迁移与 JSON 持久化。

边界：

- 做：定义 `AppConfig` 及子结构；首次运行生成默认配置；缺失字段自动补默认值；范围与一致性校验；旧版本配置自动迁移；以原子方式读写配置文件。
- 不做：不管理 API Key / 凭据（属于 `vtrans-security`，配置文件不含密钥字段）。
- 不做：不提供配置热重载（由应用层监听文件变化）。
- 不做：不管理模型文件（属于 `vtrans-models`）。
- 不做：不依赖 Tauri / UI，纯标准库 + 文件系统，可在任意 Rust 上下文使用。

## 2. 依赖关系

| 方向 | crate | 用途 |
|------|-------|------|
| 上游 | `vtrans-core` | 使用 `Language` 枚举（serde 表示 `auto` / `zh-CN` / `ja` / `en`）；`OcrConfig.language`、`TranslationConfig.source/target_language` 直接使用它 |
| 外部 | `serde` / `serde_json` | 结构定义与 JSON 序列化 |
| 外部 | `thiserror` | 派生 `ConfigError` |
| 外部 | `tracing` | 入口 `#[instrument]` 与错误路径日志 |
| 外部 | `directories` | 计算平台默认配置目录 |

下游消费方（见 `docs/ARCHITECTURE.md` 依赖表）：`vtrans-app`（模块 10）。应用层需要本模块提供：启动时加载配置、`save_settings` 命令保存用户设置、默认配置目录路径。

## 3. 快速上手

```rust
use vtrans_config::ConfigManager;

fn main() -> Result<(), vtrans_config::ConfigError> {
    // 谁创建 / 谁持有：ConfigManager 由调用方创建并长期持有（应用通常
    // 放在 AppState 中共享）；无资源需手动关闭，drop 无副作用。
    let config_dir = std::env::temp_dir().join("vtrans-demo");
    let manager = ConfigManager::new(&config_dir)?;

    // 首次加载：文件不存在时自动创建默认配置并写入磁盘
    let config = manager.load()?;
    println!("target language: {}", config.translation.target_language.code());

    // 更新：闭包内修改，内部加锁保证并发安全，自动校验并原子保存
    manager.update(|c| c.capture.interval_ms = 1000)?;

    // 显式保存：跳过加载，直接写入（同样经过校验 + 原子写）
    let mut cfg = vtrans_config::AppConfig::default();
    cfg.log_level = "debug".to_string();
    manager.save(&cfg)?;

    // 错误处理：捕获并分类
    match manager.load() {
        Ok(_) => {}
        Err(vtrans_config::ConfigError::Validation(msg)) => println!("invalid config: {msg}"),
        Err(e) => println!("load failed: {e}"),
    }
    Ok(())
}
```

## 4. 公开 API 概要

| 类型 / 函数 | 用途 |
|------|------|
| `AppConfig` | 配置根结构（capture / ocr / translation / result_window / floating_ball / hotkeys / log_level / model_dir / version） |
| `CaptureConfig` | 采集：`interval_ms`、`difference_threshold` |
| `OcrConfig` | OCR：`language`、`min_confidence` |
| `TranslationConfig` | 翻译：provider、`quality`（`"fast"` / `"balanced"`，默认 `"fast"`）、语言对、超时、API endpoint/model、重试次数 |
| `ResultWindowConfig` | 结果窗口：`always_on_top`、`opacity`（0.3–1.0，默认 0.95）、`font_size_px`（12–24，默认 14） |
| `FloatingBallConfig` | 悬浮球：`enabled`（默认 `false`，不显示）、`opacity`（0.3–1.0，默认 1.0）、`size_px`（32–72，默认 48） |
| `HotkeyConfig` | 三个全局热键字符串 |
| `ConfigManager` | 配置持久化入口 |
| `ConfigError` | 错误枚举（NotFound / Parse / Validation / Io / UnsupportedVersion） |
| `CURRENT_CONFIG_VERSION` | 当前 schema 版本（`4`） |
| `CONFIG_FILE_NAME` | 配置文件名（`config.json`） |
| `default_config_path()` | 平台默认配置路径（`config_dir/vtrans/config.json`） |

核心类型签名：

```rust
pub struct ConfigManager { /* 私有字段 */ }

impl ConfigManager {
    /// 绑定配置目录（自动创建目录），不读写文件。
    pub fn new(config_dir: &Path) -> Result<Self, ConfigError>;
    /// 解析后的配置文件完整路径。
    pub fn config_path(&self) -> &Path;
    /// 加载并迁移配置；文件不存在时创建默认配置并写盘。
    pub fn load(&self) -> Result<AppConfig, ConfigError>;
    /// 校验后原子写入（临时文件 + rename）；并发调用 last-writer-wins。
    pub fn save(&self, config: &AppConfig) -> Result<(), ConfigError>;
    /// 加锁的读-改-写；要求配置文件已存在（先调用 load）。
    pub fn update<F: FnOnce(&mut AppConfig)>(&self, f: F) -> Result<(), ConfigError>;
    /// 显式把旧版本配置升级到当前版本。
    pub fn migrate(&self) -> Result<AppConfig, ConfigError>;
}

impl AppConfig {
    /// 校验全部字段，返回首个违规（错误信息含字段路径）。
    pub fn validate(&self) -> Result<(), ConfigError>;
}
```

serde 表示（跨 JSON / IPC 边界）：`Language` 序列化为字符串 `auto` / `zh-CN` / `ja` / `en`；`translation.quality` 为字符串 `"fast"` / `"balanced"`；`model_dir` 为字符串或 `null`；`version` 为整数；`floating_ball` 为 `{"enabled": bool, "opacity": f64, "size_px": u32}` 对象；反序列化时未知字段被忽略（前向兼容）。完整字段规格见 `docs/modules/02-config.md`。

## 5. 行为契约

- **错误语义**：
  - `NotFound`：仅 `update` 在文件不存在时返回；调用顺序错误，先 `load` 即可恢复。
  - `Parse`：JSON 非法或字段类型不匹配（如 `language` 为未知字符串）；修复文件内容后可重试。
  - `Validation`：字段值超范围或不一致（信息含字段路径）；修改内容后可重试。
  - `UnsupportedVersion`：文件版本高于当前应用支持；需升级应用，不可手动降级。
  - `Io`：目录不可写、磁盘错误等；环境问题，可稍后重试。
- **并发模型**：`ConfigManager` 为 `Send + Sync`；`update` 内部 `RwLock` 串行化读-改-写；多线程调用安全；`save` 并发安全但无序（last-writer-wins）。
- **取消语义**：无异步 API、无 `CancellationToken`；所有读写为同步文件 IO，单次调用时长有限，不阻塞事件循环之外的资源。
- **资源生命周期**：`ConfigManager` 无需显式关闭；drop 无副作用；每次写入先 `sync_all` 再原子重命名，进程崩溃不会留下半写文件。
- **边界条件**：空文件 / 非法 JSON → `Parse`；缺失字段 → 默认值填充；`NaN` 阈值 → `Validation`；无 `version` 字段 → 按 v0 自动迁移。
- **版本迁移**：v0（无 `version`）→ v1 仅补版本号；v1 → v2 补齐 `result_window.opacity`、`result_window.font_size_px` 与 `floating_ball` 字段；v2 → v3 补齐 `floating_ball.opacity` / `floating_ball.size_px`——缺失字段由 `serde(default)` 兜底（`0.95` / `14` / `false` / `1.0` / `48`），已存在的字段原样保留；v3 → v4 补齐 `translation.quality`（默认 `"fast"`）并强制 `translation.source_language = ocr.language`（以 OCR 语言为权威，修复历史配置两字段不一致）；迁移结果写回磁盘。迁移幂等：v4 配置重复迁移无副作用。

## 6. 集成注意事项

| 坑 | 正确做法 |
|----|----------|
| `update` 要求文件已存在，未先 `load` 会返回 `NotFound` | 启动时先调用一次 `load()`（同时创建默认文件），之后再用 `update` |
| 用户改坏配置文件时 `load` 返回 `Parse` / `Validation`，应用不应崩溃 | 捕获 `ConfigError`，提示用户或回退到内存默认值 |
| 文件版本高于当前应用时返回 `UnsupportedVersion` | 提示用户升级应用，不要自行改写 `version` 字段 |
| 手动编辑 JSON 使 `ocr.language` 与 `translation.source_language` 不一致时保存被拒 | 两字段是联动设置，走 `set_ocr_language` / `set_source_language` 命令修改，或手动改成一致 |
| 并发 `save` 是 last-writer-wins，可能覆盖彼此的变更 | 变更依赖当前值的场景一律用 `update`（内部加锁） |
| 配置 schema 无密钥字段，往 JSON 里塞 `api_key` 会被反序列化忽略 | 凭据用 `vtrans-security` 管理，配置只保存非敏感设置 |

## 7. 设计决策记录

| 决策 | 理由 | 备选方案 |
|------|------|----------|
| 写入用「临时文件 + `sync_all` + 原子重命名」 | 中断 / 崩溃不会留下半写配置 | 直接覆盖写（写入中断即损坏） |
| `update` 内部 `RwLock` 串行化读-改-写 | 多线程并发更新不丢变更 | 无锁直接读写（并发不安全） |
| `update` 要求文件已存在 | 暴露「未加载就更新」的调用顺序错误，避免静默写入被篡改的默认值 | 缺失时从默认值开始（掩盖编程错误） |
| 缺失字段用 serde default 填充 | 旧文件 / 手写精简文件自动补全，前向兼容 | 严格 schema（旧文件全部解析失败） |
| 无 `version` 字段按 v0 迁移 | 兼容早期无版本号文件，升级透明 | 要求用户手动补版本号 |
| v1→v2 / v2→v3 新字段由 serde default 补齐而非迁移函数显式赋值 | 迁移函数只负责版本戳；缺字段自动补默认值、已有值不覆盖 | 迁移函数逐字段写默认值（会覆盖旧文件中已存在的新字段） |
| v3→v4 由迁移函数强制 `source_language = ocr.language` | OCR 语言是权威（功能目标：OCR 语言与源语言强制统一），历史漂移在此修复 | 只在校验层拒绝不一致（旧文件升级后无法通过校验，等于用户数据不可加载） |
| `quality` 用 `String` + 白名单校验而非枚举 | 与 `provider` 字段风格一致，便于未来扩展档位且 serde 表示稳定 | 定义 `Quality` 枚举（需额外映射与迁移） |
| 版本解析单一来源（`VersionProbe` + `migrate_value`） | 避免多处解析版本导致行为漂移 | 独立 `raw_version` 函数（review 后移除） |
| 配置中不保存 API Key | 凭据归 `vtrans-security`，缩小泄露面 | 密钥写入配置 JSON（泄露风险高） |

## 8. 已知限制

待实现（后续 Phase）：

- `VTRANS_CONFIG_DIR` 环境变量覆盖未集成。缓解：消费方构造 `ConfigManager` 前自行读取环境变量并传入目录。
- 配置热重载未实现。缓解：应用层轮询 mtime 或使用文件 watcher 后重新 `load()`。

设计使然：

- 校验取值范围（`difference_threshold` 0.0–1.0、`timeout_seconds` 1–3600、`max_retries` 0–10、`opacity` 0.3–1.0、`font_size_px` 12–24、`floating_ball.size_px` 32–72）是工程默认，可评审调整。缓解：修改 `validation.rs` 中常量并同步规格文档。
- `translation.quality` 仅负责持久化与合法性校验；具体如何影响本地模型（beam 参数等）由 `vtrans-translation` 消费，本 crate 不解释其语义。缓解：07 模块按 `"fast"` / `"balanced"` 映射引擎参数。
- 并发 `save` 无序（last-writer-wins）。缓解：高频写场景用 `update` 或调用方自行串行化。

## 9. 构建与测试

```powershell
cargo check -p vtrans-config
cargo test -p vtrans-config
cargo clippy -p vtrans-config --all-targets
cargo fmt -p vtrans-config -- --check
```

本模块为纯库，规格未要求验证 CLI，故无 `examples/`。

## 10. 详细规格引用

参见 `docs/modules/02-config.md`。
