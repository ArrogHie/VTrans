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
    update_live_region, // (region, mode: "single" | "live")
    set_ocr_language,
    set_source_language,
    set_target_language,
    set_translation_provider,
    load_local_models,
    save_settings,
    update_result_window_appearance, // (opacity, fontSizePx)
    update_floating_ball_appearance, // (opacity, sizePx)
    set_api_key,
    set_provider_credentials, // (providerId, apiKey?, appId?, secret?)
    get_app_config,
    get_app_status,
]
```

### 翻译 Provider 组装与凭据目标

`state.rs` 的 `build_translation_provider(config, credentials, model_manager)`
按 `config.translation.provider` 分支组装，凭据只经 `CredentialManager`
读取（不落内存副本到日志）：

| 配置 id | 运行时实现 id | 凭据目标 |
|---------|---------------|----------|
| `openai`（默认） | `openai` | `openai` |
| `deepl` | `deepl` | `deepl` |
| `google` | `google` | `google` |
| `azure` | `azure` | `azure`（区域取 `translation.region`） |
| `baidu` | `baidu` | `baidu_app_id` + `baidu_secret`（两个独立目标） |
| `local` | `local-onnx` | 无（ONNX 模型走 ModelManager） |

配置域白名单 `["openai","deepl","google","azure","baidu","local"]` 与
vtrans-config 校验一致；旧 id `"api"` 已废弃（v4→v5 迁移重命名为
`"openai"`，应用层校验拒绝）。切换 provider（`set_translation_provider` /
`save_settings`）即时重建并更新配置；live 会话运行中切换仍返回
`PipelineError::AlreadyRunning`。

`set_api_key` 按当前配置的 provider 写入对应凭据目标（baidu 写
`baidu_secret`），`set_provider_credentials` 泛化写入完整凭据集（baidu
需要 `appId` + `secret` 两个值）；写入后若目标 provider 为当前 provider
立即重建。日志引用 Key 只用 `mask_sensitive` / `mask_key` 掩码。

`update_result_window_appearance(opacity, font_size_px)` 与
`update_floating_ball_appearance(opacity, size_px)` 只持久化对应窗口的两个
外观字段（加载配置 → 修改 → `save_config` 校验 + 原子写），**不获取 live
生命周期锁、不检查 live 任务、不重建 Provider**——外观调整在实时会话运行中
仍可保存（bug 2 后端侧）。越界值由配置校验返回 `ConfigError::Validation`
并映射为 `AppError::Config`。参数名遵循 Tauri 2 默认 camelCase。

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
pub fn emit_debug_frame(app: &AppHandle, payload: DebugFramePayload);
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

- 选区确认（`update_live_region(region, mode)`）→ overlay 窗口覆盖区域
  所在显示器（窗口原点 = 显示器原点），前端 `regionOverlay` 服务定位/
  显示/点穿，后端同步显示兜底；CSS 边框 + 尺寸标签绘制在区域相对偏移处
  （事件 `overlay_region_updated`）；
- **显隐规则按模式区分**（决策集中在 `overlay.rs` 的可测纯函数
  `overlay_intent` / `overlay_intent_for_stop`）：
  - 单次模式：选区确认（`mode: "single"`）**不显示**常驻方框；
    `capture_once` 完成（成功或失败）后确保隐藏，方框不残留；
  - 实时模式：`start_live_translation` 启动即显示；`update_live_region`
    （`mode: "live"`）更新区域时显示新位置；**暂停保留**方框（暂停走
    `stop_live_translation`，后端按 `Pause` 决策不隐藏）；**真正停止**
    隐藏（UI 停止按钮先隐藏再 stop、热键 Alt+Shift+S 走 `Stop` 决策由
    后端隐藏）；
- 重新选区 / 取消 → 隐藏（事件 `overlay_hidden`）；
- overlay 无边框、透明、置顶、可点穿（`focusable: false` +
  `set_ignore_cursor_events(true)`），只传输 `ScreenRegion` 坐标，不跨 IPC
  传图像。
- `AppStatus.mode`（`"single"` / `"live"`）是后端最近会话模式的权威记录
  （单次捕获与确认报告 `single`，实时会话运行或暂停均报告 `live`）；前端
  启动水合只在 `mode == "live"` 时恢复常驻方框，单次模式的选择区域不会
  在重启后显示方框。

### 窗口清单（悬浮球窗口）

VTrans 共声明 5 个窗口（`src-tauri/tauri.conf.json`，由模块 10 维护）：

| label | 角色 | 关键配置 |
|-------|------|---------|
| main | 主窗口 | 420×600，可缩放，应用入口 |
| result | 结果窗口 | 360×140（迷你条形态）、默认隐藏、置顶、`transparent: true`、无边框（`decorations: false`）、可缩放 |
| selector | 选区窗口 | 全屏、透明、无边框，默认隐藏 |
| overlay | 选区方框 | 全屏、透明、无边框、置顶、可点穿，默认隐藏 |
| floater | 悬浮球 | 48×48、透明、无边框、置顶、跳过任务栏、`focus: false`、默认隐藏 |

floater（悬浮球）：

- 由前端按 `floating_ball.enabled`（vtrans-config，默认 false）显示；拖动
  复用既有窗口 API（show/hide/start-dragging/set-position/
  set-always-on-top），**不新增 IPC Command**；
- **不点穿**：与 overlay 不同，不做 `setIgnoreCursorEvents`；
- 关闭请求被全局 hide-on-close（prevent_close + hide）覆盖，隐藏而非销毁；
- 默认 `visible: false`，启动不产生可见窗口。

### capability 归属

`src-tauri/capabilities/default.json` 由模块 10 统一维护，`windows` 覆盖
main/result/selector/overlay/floater 五个窗口。清单按前端实际调用的窗口
API 复核：`allow-available-monitors`、`allow-set-position`、
`allow-set-size`、`allow-set-ignore-cursor-events` 由 `regionOverlay` 服务
使用（常驻方框的显示器枚举/定位/点穿）；`allow-show`/`allow-hide`/
`allow-set-focus`/`allow-set-always-on-top`/`allow-start-dragging` 由结果
窗口、选区窗口与悬浮球共用；无多余权限。

**透明度能力验证结论（feat/10-floating-ball-window）**：锁定的 Tauri
2.11.5（tauri-runtime-wry 2.11.4、@tauri-apps/api 2.11.1）不提供**运行时**
opacity 能力——Rust `WebviewWindow` 无 `set_opacity`、JS 无
`setOpacity()`、ACL 无 `core:window:allow-set-opacity`（tauri-build 实测
`Permission ... not found`，源码 grep 零命中）。透明是窗口配置属性，不
需要 ACL；result 窗口已声明 `transparent: true`（追加提交），前端可直接
用 CSS 背景 alpha 实现半透明透出桌面。Windows 透明窗口验证结论：文字
渲染/缩放重绘与 overlay/selector 同机制（overlay 已在透明窗口渲染尺寸
标签）；result 已声明 `decorations: false`（无原生标题栏，标题栏/关闭按钮
由前端 CSS + `startDragging` 提供，capability 已含 `allow-start-dragging`）；
原生阴影在分层窗口上可能不渲染，登记 README 手工验证项 14。

**待架构确认（不阻塞 MVP）**：常驻方框目前为纯 CSS 边框，不显示区域真实
缩略图；若产品要求屏幕缩略预览，需由 vtrans-app 提供小尺寸位图事件/命令
（`CapturedImage` 不得跨 IPC）。命令命名约定维持 Tauri 默认 camelCase，
如需统一 snake_case 需前后端同步修改（见 `contracts.rs` 注释）。

### Debug 模式（捕获帧预览）

- **开关**：`--debug` 或 `VTRANS_DEBUG=1`（`true` 亦可），默认关闭，不写入
  config.json；解析失败不影响启动。`AppStatus.debug_mode` 透传给前端决定
  是否渲染调试面板。
- **帧出口**：vtrans-pipeline 的 `FrameSink`（`Pipeline::with_frame_sink`）
  在捕获帧进入 OCR 前调用；关闭时无 sink、零开销。
- **编码**：`encode_debug_thumbnail` 纯函数（BGRA8/RGBA8 → ≤480px JPEG80
  Base64），在阻塞池执行；帧序号/时间戳随 payload 发送。
- **事件**：`debug_frame_updated`（仅 Debug 开启时发射），payload 含
  `image`/`region`/`frame_index`/`timestamp_ms`；≤10fps 节流、watch 最新值
  语义、编码失败 `warn!` 跳过。区域元数据：单次翻译取命令传入的区域，
  实时会话跟随 `update_live_region` 的选区；`frame_index` 按捕获帧递增，
  被节流/编码失败跳过的帧留下序号缺口，前端可据此判断丢帧。
- **隐私**：只显示不保存（不落盘、不进日志、不进 store/结果窗口）；此
  缩略图跨 IPC 是 Debug-only 显式豁免（`CapturedImage` 默认禁止序列化）。

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
    #[error("provider credential error: {0}")]
    ProviderCredential(String),
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
| Provider 切换 | 手工验证 + 单元 | 全链路手工验证（README 第 4 条）；6 个配置 id 白名单、`update_translation_provider_config`、5 个云 Provider 组装分支有单测 |
| Provider 凭据 | 单元 | 凭据目标映射、`store_api_key` / `store_provider_credentials` 目标读写、百度双目标、缺失凭据容忍（state.rs tests）✅；IPC 参数契约见 contracts.rs |
| set_api_key / set_provider_credentials | 手工验证 + 单元 | 全链路手工验证（README 第 7 条）；key 校验与凭据存储有单测，IPC 参数契约见 contracts.rs |
| get_app_config | 手工验证 + 单元 | 前端水合手工验证（README 第 8 条）；AppConfig 序列化字段契约见 contracts.rs |
| 快捷键注册 | 手工验证 | 依赖真实全局快捷键注册环境（README 第 5 条）；动作分派枚举有单测 |
| 托盘与窗口生命周期 | 手工验证 | 依赖系统托盘与真实窗口环境（README 第 9 条）；菜单 id 与事件名有单测 |
| 选区 overlay | 手工验证 + 单元 | 依赖真实显示器（README 第 10 条）；显隐决策纯函数（单次确认不显示/实时启动显示/实时停止隐藏/暂停保留）与坐标换算有单测 |
| 单实例保护 | 手工验证 | 依赖进程级行为（README 第 11 条） |
| Debug 模式开关 | 单元 | `parse_debug_env_value` 值域解析（setup.rs tests）✅ |
| 缩略图编码 | 单元 | 尺寸缩放/JPEG 有效/格式与缓冲校验（debug_frame.rs tests）✅ |
| Debug 帧出口 | 集成 | FrameSink 收到进入 OCR 前的帧、跳过未变化帧（pipeline 集成测试）✅ |
| Debug 面板显示 | 手工验证 | 依赖真实显示器与模型（README 第 12 条） |
| 错误映射 | 单元 | 各模块错误正确映射到 AppError（error.rs tests）✅ |
| 事件发送 | 单元 | PipelineEvent 正确转为前端事件（events.rs tests）✅ |

> 说明：依赖 Windows 桌面环境的 5 项已登记为手工验证，具体步骤与验证点见
> `crates/vtrans-app/README.md`「手工验证项」一节；其纯逻辑部分均有自动化测试。

### Provider id 契约

`AppStatus.translation_provider` 返回运行时 Provider 的实现 id。云端
Provider 与配置 id 一致（`"openai"` / `"deepl"` / `"google"` / `"azure"` /
`"baidu"`），仅本地不同（实现 `"local-onnx"` ↔ 配置 `"local"`）。两端映射：

- 后端：`validate_translation_provider_id` 维护配置标识符白名单
  （`["openai","deepl","google","azure","baidu","local"]`）；
- 前端：`normalizeProviderId` 把 `"local-onnx"` 映射回 `"local"`，其余
  实现 id 原样透传。

**新增翻译 Provider 时**，必须同步更新后端白名单与前端 `normalizeProviderId`
映射，并在 `crates/vtrans-app/README.md` 登记，否则状态水合会显示错误引擎。

## 验收标准

- [x] 所有 Commands 可被前端调用（`invoke_handler` 注册 16 个命令，前端
      `src/services/tauri.ts` 全部按 Tauri 2 camelCase 参数契约调用）
- [x] 所有 Events 正确推送到前端（`events.rs` 单测 + `tests/contracts.rs`
      固化了事件名与 payload 形状）
- [x] AppState 正确组装各模块实现（`AppState::new` / `new_with_debug`）
- [x] 快捷键可注册和触发（实现 + 动作分派单测；真实注册依赖桌面环境，
      登记 README 手工验证项 5）
- [x] 错误信息对用户友好（`AppError` 序列化为纯字符串，`error.rs` 单测）
- [x] UI 线程不被阻塞（模型校验、缩略图编码走 `spawn_blocking`）
- [x] Release 构建关闭不必要 capability（Debug 面板采用主窗口内嵌方案，
      不新增窗口/权限；capability 归属见「capability 归属」一节）
- [x] README.md 完整

### 多 Provider 验收（本任务）

- [x] `build_translation_provider` 按 `openai`/`deepl`/`google`/`azure`/
      `baidu`/`local` 六分支组装，凭据从 CredentialManager 对应目标读取
      （5 个云分支有单测，local 依赖真实模型文件登记手工验证）
- [x] OpenAI 配置 id 为 `openai`（默认 provider），旧 `"api"` 被校验拒绝
- [x] `set_api_key` 泛化为按当前 provider 写对应目标（baidu 写
      `baidu_secret`），写入后立即重建当前 provider
- [x] `set_provider_credentials` 支持 baidu 双目标（`app_id` + `secret`），
      IPC 参数契约（`{ providerId, apiKey?, appId?, secret? }`）已固化
- [x] 切换 provider 即时重建并更新配置；live 会话运行中切换仍拒绝
- [x] `AppStatus.translation_provider` 运行时 id 与前端映射对齐
      （`openai`/`deepl`/`google`/`azure`/`baidu`/`local-onnx`）
- [x] 凭据只经 CredentialManager 读写，日志仅掩码形式；百度 APP ID 与
      Secret 两个独立目标
- [x] 未修改 vtrans-core 与其它 crate；README 与本文档同步

### Debug 模式验收（本任务）

- [x] 开关：`--debug` / `VTRANS_DEBUG=1`（`true` 亦可），默认关闭、不持久化；
      解析失败不影响启动（`parse_debug_env_value` 单测）
- [x] 关闭时零开销：不挂 `FrameSink`、不注册 `debug_frame_updated`、无面板
- [x] 帧出口：pipeline `FrameSink` 在进入 OCR 前收到帧（pipeline 集成测试
      `frame_sink_observes_*`），live 模式帧差未触发时不产生调试帧
- [x] 编码：纯函数 `encode_debug_thumbnail`（≤480px JPEG80），单测覆盖
      缩放、格式转换与非法缓冲
- [x] 传输：`debug_frame_updated` 事件 payload（`image`/`region`/
      `frame_index`/`timestamp_ms`）契约有单测；节流 ≤10fps；区域元数据
      单次取命令区域、实时跟随选区；编码失败 `warn!` 跳过且不影响翻译
- [x] 日志纪律：Debug 帧不进日志，开关状态只记一行 `info!`
- [x] 隐私：只显示不保存（不落盘、不进日志、不进 store/结果窗口）；面板
      显示依赖真实桌面环境，登记 README 手工验证项 12

### overlay 生命周期验收（Bug-003）

- [x] 单次模式：选区确认（`mode: "single"`）不显示常驻方框；`capture_once`
      完成（成功或失败）后隐藏方框
- [x] 实时模式：启动显示、`update_live_region`（`mode: "live"`）更新显示、
      暂停保留（`stop_live_translation` 按 `Pause` 决策不隐藏）、真正停止
      隐藏（UI 按钮先隐藏再 stop、热键按 `Stop` 决策后端隐藏）
- [x] 决策逻辑集中为纯函数：`overlay_intent` / `overlay_intent_for_stop`
      （overlay.rs tests 覆盖四个场景）
- [x] IPC 契约已定稿：`update_live_region(region, mode)`（Tauri 2 默认
      camelCase，前端传 `{ region, mode }`）；`AppStatus.mode`（`"single"`/
      `"live"`）；contracts.rs 与前端类型/测试已同步
- [x] 启动水合：前端仅在 `snapshot.mode === "live"` 时恢复常驻方框；
      单次模式的选择区域重启后不显示方框

### 悬浮球窗口与透明度能力验收（feat/10-floating-ball-window）

- [x] `tauri.conf.json` 声明 floater 窗口：48×48、`resizable: false`、
      `decorations: false`、`transparent: true`、`alwaysOnTop: true`、
      `skipTaskbar: true`、`shadow: false`、`focus: false`、
      `visible: false`（默认隐藏，前端按配置显示）
- [x] capability `windows` 纳入 `"floater"`；悬浮球复用既有窗口权限
      （show/hide/start-dragging/set-position/set-always-on-top/
      available-monitors），无新增权限
- [x] 透明度能力验证结论已记录：Tauri 2.11.5 无运行时 opacity（Rust/JS/ACL
      均无，`core:window:allow-set-opacity` 构建期报 not found）；result
      窗口已声明 `transparent: true`，前端 CSS alpha 可直接实现半透明
- [x] 行为变更最小：floater 默认隐藏、全局 hide-on-close 覆盖；未新增
      IPC Command；未修改其他 crate 与 vtrans-core
- [x] 回归：`cargo fmt --all -- --check`、`cargo clippy -p vtrans-app
      --all-targets`、`cargo test -p vtrans-app`、`cargo check --workspace`
      全绿

### result 窗口透明化验收（方案 1，追加提交）

- [x] result 窗口声明 `transparent: true`（与 overlay/selector 相同的
      WebView2 透明机制，前端后续用 CSS 背景 alpha 实现半透明）
- [x] 初始尺寸调整为迷你条形态 360×140，保留 `resizable: true` /
      `visible: false` / `alwaysOnTop: true`
- [x] 未新增 capability 权限（透明是窗口配置属性，不需要 ACL）
- [x] 验证结论已记录：配置经 `cargo check -p vtrans` 构建期校验；文字
      渲染/缩放/阴影表现登记 README 手工验证项 14（overlay/selector 已用
      同一机制渲染文字标签）
- [x] 回归：fmt / clippy / app 单测 / workspace check 全绿；未修改其他
      crate 与 vtrans-core

### 无边框弹窗与外观命令验收（fix/10-window-appearance-commands）

- [x] result 窗口新增 `decorations: false`（无原生标题栏；`resizable` /
      `visible` / `alwaysOnTop` / `transparent` 保留）
- [x] `update_result_window_appearance(opacity, font_size_px)`：加载配置 →
      修改 `result_window` 两字段 → `save_config`（内部校验 + 原子写）；
      不获取 live 生命周期、不检查 live 任务、不重建 Provider
- [x] `update_floating_ball_appearance(opacity, size_px)`：同上，修改
      `floating_ball` 两字段
- [x] 越界值由 `save_config` 校验返回 `ConfigError::Validation`，映射为
      `AppError::Config`
- [x] 两个命令已注册进 `invoke_handler`；contracts.rs 补充 camelCase
      参数契约（`{ opacity, fontSizePx }` / `{ opacity, sizePx }`）
- [x] 测试：字段更新范围、越界拒绝且不落盘、边界值接受、live 运行中仍可
      保存（持久化路径独立于 pipeline/provider 状态）
- [x] 回归：fmt / clippy / app 单测 / workspace check 全绿；未修改其他
      crate 与 vtrans-core

## 开发注意事项

- AppState 使用 RwLock 保护可变状态
- Commands 通过 tauri::State 访问 AppState
- Pipeline 事件通过 app.emit 转发到前端
- 快捷键冲突时允许用户修改（配置中定义）
- 所有 Command 返回 Result<T, AppError>
- AppError 实现 Serialize 用于前端错误展示
- `ocr.language` 与 `translation.source_language` 是联动字段
  （vtrans-config `validate_language_linkage` 要求二者恒等）。
  `set_ocr_language` 与 `set_source_language` 各自同时写入两个字段，
  任一命令执行后两字段恒相等，`ConfigManager::save` 校验不会因联动不一致
  而拒绝。`set_target_language` 只写 `translation.target_language`。
- src-tauri/main.rs 只调用 vtrans-app::init_app，保持薄层
- 主窗口关闭默认隐藏到托盘而非退出；托盘是唯一恢复入口，托盘创建失败时
  应用启动失败而不是留下无法恢复的孤儿进程
- 合并顺序：`feat/10-app` 必须先于 `feat/11-frontend` 合并——模块 11 已调用
  `set_source_language` / `set_target_language`，若前端先合并，运行时会出现
  command 不存在错误。`set_api_key` / `get_app_config` 同理，前端
  `SettingsPanel` 已按 `{ apiKey }`（Tauri 2 默认 camelCase）参数名约定等待
  这两个命令落地；新增 command 不得添加 `rename_all = "snake_case"`，否则
  需同步修改前端 `src/services/tauri.ts` 与 `src/test/ipc.test.ts`。
- 多 Provider 前端同步（模块 11）：`AppStatus.translation_provider` 现在
  返回 `"openai"`/`"deepl"`/`"google"`/`"azure"`/`"baidu"`/`"local-onnx"`。
  前端需把 `normalizeProviderId` 中的 `"api" -> "api"` 映射改为
  `"openai" -> "openai"`（并删除 `"api"` 分支）、扩展 `ProviderId` 类型与
  凭据表单（baidu 需要 APP ID + Secret 两个输入，通过
  `set_provider_credentials` 提交）；`set_api_key` 仍按当前 provider 写
  对应目标，单 key 输入流程可保留。
