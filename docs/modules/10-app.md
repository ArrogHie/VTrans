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
    set_source_language,
    set_target_language,
    set_translation_provider,
    load_local_models,
    save_settings,
    set_api_key,
    get_app_config,
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
pub fn emit_overlay_region(app: &AppHandle, region: &ScreenRegion);
pub fn emit_overlay_hidden(app: &AppHandle);
```

### 全局快捷键

```rust
pub fn register_hotkeys(app: &AppHandle) -> Result<(), AppError>;
// Alt+Shift+A: start_region_selection (单次)
// Alt+Shift+R: start_live_translation (实时)
// Alt+Shift+S: stop_live_translation
```

### 窗口生命周期与托盘

- 关闭主窗口 → 隐藏到系统托盘（进程、实时会话、全局快捷键继续运行）；
- 托盘左键 / 菜单「显示主窗口」→ 恢复主窗口；菜单「退出」→ `app.exit(0)`；
- 单实例插件拦截第二个进程实例并恢复已有实例主窗口。

### 选区 overlay

- 选区确认（`update_live_region`）→ overlay 窗口覆盖区域所在显示器（窗口
  原点 = 显示器原点），前端 `regionOverlay` 服务定位/显示/点穿，后端同步
  显示兜底；CSS 边框 + 尺寸标签绘制在区域相对偏移处（事件
  `overlay_region_updated`）；
- 重新选区 / 取消 / 真正停止（UI 按钮或热键）→ 隐藏（事件
  `overlay_hidden`）；暂停实时会话保留方框（暂停走 `stop_live_translation`，
  后端不在停止路径隐藏，避免与前端暂停语义冲突）；
- overlay 无边框、透明、置顶、可点穿（`focusable: false` +
  `set_ignore_cursor_events(true)`），只传输 `ScreenRegion` 坐标，不跨 IPC
  传图像。

### capability 归属

`src-tauri/capabilities/default.json` 由模块 10 统一维护，清单按前端实际
调用的窗口 API 复核：`allow-available-monitors`、`allow-set-position`、
`allow-set-size`、`allow-set-ignore-cursor-events` 由 `regionOverlay` 服务
使用（常驻方框的显示器枚举/定位/点穿），其余 `allow-show`/`allow-hide`/
`allow-set-focus`/`allow-set-always-on-top`/`allow-start-dragging` 由结果
窗口与选区窗口使用；无多余权限。

**待架构确认（不阻塞 MVP）**：常驻方框目前为纯 CSS 边框，不显示区域真实
缩略图；若产品要求屏幕缩略预览，需由 vtrans-app 提供小尺寸位图事件/命令
（`CapturedImage` 不得跨 IPC）。命令命名约定维持 Tauri 默认 camelCase，
如需统一 snake_case 需前后端同步修改（见 `contracts.rs` 注释）。

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
    #[error("invalid api key: {0}")]
    InvalidApiKey(String),
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
| AppState 初始化 | 手工验证 | 依赖 Windows Graphics Capture、Credential Manager 和模型文件，无头环境不可自动化；验证步骤见 crate README「手工验证项」第 1 条 |
| save_settings | 手工验证 + 单元 | 全链路（IPC → 持久化 → 重启生效）手工验证（README 第 2 条）；配置校验与原子持久化由 vtrans-config 单测覆盖 |
| get_app_status | 手工验证 + 单元 | 全链路手工验证（README 第 3 条）；`AppStatus` 序列化契约有单测 |
| Provider 切换 | 手工验证 + 单元 | 全链路手工验证（README 第 4 条）；`validate_translation_provider_id` / `update_translation_provider_config` 有单测 |
| set_api_key | 手工验证 + 单元 | 全链路手工验证（README 第 7 条）；key 校验与凭据存储有单测，IPC 参数契约见 contracts.rs |
| get_app_config | 手工验证 + 单元 | 前端水合手工验证（README 第 8 条）；AppConfig 序列化字段契约见 contracts.rs |
| 快捷键注册 | 手工验证 | 依赖真实全局快捷键注册环境（README 第 5 条）；动作分派枚举有单测 |
| 托盘与窗口生命周期 | 手工验证 | 依赖系统托盘与真实窗口环境（README 第 9 条）；菜单 id 与事件名有单测 |
| 选区 overlay | 手工验证 + 单元 | 依赖真实显示器（README 第 10 条）；overlay 坐标换算由前端单测覆盖 |
| 单实例保护 | 手工验证 | 依赖进程级行为（README 第 11 条） |
| 错误映射 | 单元 | 各模块错误正确映射到 AppError（error.rs tests）✅ |
| 事件发送 | 单元 | PipelineEvent 正确转为前端事件（events.rs tests）✅ |

> 说明：依赖 Windows 桌面环境的 5 项已登记为手工验证，具体步骤与验证点见
> `crates/vtrans-app/README.md`「手工验证项」一节；其纯逻辑部分均有自动化测试。

### Provider id 契约

`AppStatus.translation_provider` 返回运行时 Provider 的实现 id（当前为
`"api"` / `"local-onnx"`），而 `set_translation_provider` 与配置 schema 使用
配置标识符（`"api"` / `"local"`）。两端映射：

- 后端：`validate_translation_provider_id` 维护配置标识符白名单；
- 前端：`normalizeProviderId` 把实现 id 映射回配置标识符。

**新增翻译 Provider 时**，必须同步更新后端白名单与前端 `normalizeProviderId`
映射，并在 `crates/vtrans-app/README.md` 登记，否则状态水合会显示错误引擎。

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
- 主窗口关闭默认隐藏到托盘而非退出；托盘是唯一恢复入口，托盘创建失败时
  应用启动失败而不是留下无法恢复的孤儿进程
- 合并顺序：`feat/10-app` 必须先于 `feat/11-frontend` 合并——模块 11 已调用
  `set_source_language` / `set_target_language`，若前端先合并，运行时会出现
  command 不存在错误。`set_api_key` / `get_app_config` 同理，前端
  `SettingsPanel` 已按 `{ apiKey }`（Tauri 2 默认 camelCase）参数名约定等待
  这两个命令落地；新增 command 不得添加 `rename_all = "snake_case"`，否则
  需同步修改前端 `src/services/tauri.ts` 与 `src/test/ipc.test.ts`。
