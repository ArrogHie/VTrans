# 模块 10：vtrans-app 应用层

| 属性 | 值 |
|------|-----|
| Crate | `vtrans-app` |
| 分支 | `feat/10-app` |
| 上游依赖 | 全部 Rust crate (core, config, security, capture, ocr, text, translation, models, pipeline) |
| 层级 | 4 |
| 复杂度 | 高 |
| 阶段 | Phase 4 |

## 职责

定义 Tauri Commands 和 Events，管理 AppState 生命周期，注册全局快捷键，组装所有模块的具体实现并注入 Pipeline。是 Rust 侧与前端通信的唯一桥梁。

## 公开 API

### Tauri Commands

```rust
t::generate_handler![
    start_region_selection,
    capture_once,
    start_live_translation,
    stop_live_translation,
    update_live_region,
    set_ocr_language,
    set_translation_provider,
    load_local_models,
    save_settings,
    get_app_status,
]
```

### AppState

```rust
pub struct AppState {
    config: RwLock<ConfigManager>,
    credentials: CredentialManager,
    pipeline: RwLock<Option<Pipeline>>,
    ocr_provider: RwLock<Box<dyn OcrProvider>>,
    translation_provider: RwLock<Box<dyn TranslationProvider>>,
    capture_source: WindowsCaptureSource,
    model_manager: ModelManager,
}

impl AppState {
    pub fn new(app_data_dir: &Path) -> Result<Self, AppError>;
}
```

### Events

```rust
pub fn emit_pipeline_event(app: &AppHandle, event: PipelineEvent);
```

### 全局快捷键

```rust
pub fn register_hotkeys(app: &AppHandle) -> Result<(), AppError>;
// Alt+Shift+A: start_region_selection (单次)
// Alt+Shift+R: start_live_translation (实时)
// Alt+Shift+S: stop_live_translation
```

## 错误类型

```rust
[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("state not initialized")]
    NotInitialized,
    #[error("pipeline error: {0}")]
    Pipeline(#[from] PipelineError),
    #[error("config error: {0}")]
    Config(#[from] ConfigError),
    #[error("security error: {0}")]
    Security(#[from] SecurityError),
    #[error("model error: {0}")]
    Model(#[from] ModelError),
    #[error("capture error: {0}")]
    Capture(#[from] CaptureError),
    #[error("ocr error: {0}")]
    Ocr(#[from] OcrError),
    #[error("translation error: {0}")]
    Translation(#[from] TranslationError),
    #[error("tauri error: {0}")]
    Tauri(String),
    #[error("hotkey registration failed: {0}")]
    HotkeyFailed(String),
}
```

## 内部文件结构

```text
crates/vtrans-app/
├── Cargo.toml
├── README.md
└── src/
    ├── lib.rs          # re-export, init_app
    ├── state.rs         # AppState
    ├── commands.rs      # Tauri command handlers
    ├── events.rs        # 事件发送封装
    ├── hotkeys.rs       # 全局快捷键注册
    └── setup.rs         # 应用启动初始化
```

## 测试计划

| 测试项 | 类型 | 说明 |
|--------|------|------|
| AppState 初始化 | 集成 | 创建后所有字段就绪 |
| save_settings | 集成 | 调用后配置文件更新 |
| get_app_status | 集成 | 返回正确状态 |
| Provider 切换 | 集成 | set_translation_provider 后使用新 provider |
| 快捷键注册 | 集成 | register_hotkeys 不报错 |
| 错误映射 | 单元 | 各模块错误正确映射到 AppError |
| 事件发送 | 集成 | PipelineEvent 正确转为前端事件 |

## 验收标准

- [ ] 所有 Commands 可被前端调用
- [ ] 所有 Events 正确推送到前端
- [ ] AppState 正确组装各模块实现
- [ ] 快捷键可注册和触发
- [ ] 错误信息对用户友好
- [ ] UI 线程不被阻塞
- [ ] Release 构建关闭不必要 capability
- [ ] README.md 完整

## 开发注意事项

- AppState 使用 RwLock 保护可变状态
- Commands 通过 tauri::State 访问 AppState
- Pipeline 事件通过 app.emit 转发到前端
- 快捷键冲突时允许用户修改（配置中定义）
- 所有 Command 返回 Result<T, AppError>
- AppError 实现 Serialize 用于前端错误展示
- src-tauri/main.rs 只调用 vtrans-app::init_app，保持薄层
