# vtrans-app

VTrans 的 Rust 应用层：组装各模块的生产实现，提供 Tauri Commands/Events、应用状态生命周期和全局快捷键，是 Rust 与前端 IPC 的唯一桥梁。

## 职责

- 通过 AppState 组装 config、security、capture、OCR、translation、models 和 pipeline。
- 提供手动单次翻译、实时翻译、区域更新、语言/Provider 切换、设置持久化和状态查询命令。
- 将 vtrans_pipeline::PipelineEvent 转换为稳定的前端事件名和 JSON payload。
- 注册配置中的全局快捷键，并把快捷键动作派发到选区、实时翻译和停止流程。
- 管理系统托盘（关闭主窗口隐藏到托盘、托盘菜单恢复/退出）与单实例保护。
- 维护常驻选区 overlay 窗口，在屏幕上持续显示当前捕获区域边界。
- Debug 模式下把进入 OCR 前的捕获帧以缩略图形式实时推送到前端面板。
- 使用 AppError 将底层错误映射为可序列化、可展示的用户错误信息。

## 依赖关系

### 上游 crate

- vtrans-core：共享类型、Provider trait 和 trait 错误。
- vtrans-config：配置 schema 和原子持久化。
- vtrans-security：Windows Credential Manager 凭据访问。
- vtrans-capture：WindowsCaptureSource。
- vtrans-ocr：PaddleOcrProvider。
- vtrans-translation：API 和本地 Provider。
- vtrans-models：manifest、模型路径和完整性报告。
- vtrans-pipeline：捕获、OCR、标准化和翻译编排。

> vtrans-text 只作为 vtrans-pipeline 的传递依赖参与 OCR 文本标准化；本 crate
> 不直接引用它，因此不在 Cargo.toml 中声明。

### 外部 crate

- tauri 2：commands、managed state、events 和窗口操作。
- tauri-plugin-global-shortcut 2：全局快捷键注册。
- tokio：异步命令、pipeline task 和有界事件通道。
- async-trait：为共享 Provider adapter 实现 core trait。
- serde/serde_json：IPC payload 和错误序列化。
- thiserror：错误枚举和错误链。
- tracing：结构化生命周期和错误日志。
- tracing-appender：持有 `WorkerGuard`，在应用生命周期内刷新非阻塞日志写入器。
- tauri-plugin-single-instance：阻止多实例并存，避免全局快捷键冲突。
- image：捕获帧缩放与 JPEG 编码（Debug 模式）。
- base64：调试缩略图 Base64 编码（跨 IPC 事件 payload）。

所有依赖使用 MIT 或 Apache-2.0 兼容许可证；新增依赖仅在本 crate 的 Cargo.toml 中声明。

## 公开 API 概要

~~~rust
pub struct AppState { /* managed by Tauri */ }

impl AppState {
    pub fn new(app_data_dir: &Path) -> Result<Self, AppError>;
    pub fn new_with_debug(app_data_dir: &Path, debug_mode: bool) -> Result<Self, AppError>;
}

pub struct AppStatus {
    pub mode: PipelineMode,
    pub pipeline_status: PipelineStatus,
    pub ocr_provider: String,
    pub translation_provider: String,
    pub selected_region: Option<ScreenRegion>,
    pub live_running: bool,
    pub model_progress: Option<f32>,
    pub debug_mode: bool,
}
~~~

AppState::new 从 app_data_dir/config.json 加载配置，默认从 app_data_dir/models/manifest.json 加载模型 manifest；AppConfig.model_dir 可以覆盖模型目录。
AppState::new_with_debug 额外接收 Debug 模式开关（仅本次运行有效，不持久化）。

### Tauri Commands

通过 setup::builder() 注册以下 command：

~~~rust
start_region_selection() -> Result<ScreenRegion, AppError>
cancel_region_selection() -> Result<(), AppError>
capture_once(region: ScreenRegion) -> Result<OcrResult, AppError>
start_live_translation(config: LiveTranslationConfig) -> Result<(), AppError>
stop_live_translation() -> Result<(), AppError>
update_live_region(region: ScreenRegion, mode: PipelineMode) -> Result<(), AppError>
set_ocr_language(language: Language) -> Result<(), AppError>
set_source_language(language: Language) -> Result<(), AppError>
set_target_language(language: Language) -> Result<(), AppError>
set_translation_provider(provider_id: String) -> Result<(), AppError>
load_local_models() -> Result<VerifyReport, AppError>
save_settings(settings: AppConfig) -> Result<(), AppError>
update_result_window_appearance(opacity: f64, font_size_px: u32) -> Result<(), AppError>
update_floating_ball_appearance(opacity: f64, size_px: u32) -> Result<(), AppError>
set_api_key(api_key: String) -> Result<(), AppError>
set_provider_credentials(provider_id: String, api_key: Option<String>, app_id: Option<String>, secret: Option<String>) -> Result<(), AppError>
get_app_config() -> Result<AppConfig, AppError>
get_app_status() -> Result<AppStatus, AppError>
add_translation_box(region: ScreenRegion) -> Result<TranslationBoxInfo, AppError>
remove_translation_box(box_id: u32) -> Result<(), AppError>
update_translation_box(box_id: u32, region: ScreenRegion) -> Result<(), AppError>
list_translation_boxes() -> Result<Vec<TranslationBoxInfo>, AppError>
start_multi_realtime() -> Result<(), AppError>
stop_multi_realtime() -> Result<(), AppError>
stop_box(box_id: u32) -> Result<(), AppError>
open_result_window() -> Result<(), AppError>
~~~

`capture_once` 在流水线运行期间并发消费事件通道，把 `ocr_started`、
`translation_started` 等阶段事件推送到前端（单次捕获没有 live session，
因此不发送 `live_session_stopped`），命令本身仍只返回最终的 `OcrResult`。
单次翻译完成后还通过 `translation://single-result` 事件把原文和译文推送
到翻译弹窗，不再在主页面显示结果。

### 多框翻译命令（Multi-Box）

多框实时翻译围绕 `MultiBoxPipeline`（vtrans-pipeline）构建：

- `add_translation_box(region)`：分配下一个 id 和调色板颜色，加入 pipeline
  （惰性创建），持久化到 config，发射 `multibox://box-added`，框数达到
  `warning_threshold` 时发射 `multibox://warning`（非阻塞）。
- `remove_translation_box(box_id)`：从 pipeline 和 config 移除，发射
  `multibox://box-removed`。
- `update_translation_box(box_id, region)`：更新区域，发射
  `multibox://box-updated`。
- `list_translation_boxes()`：从 config 读取当前翻译框列表。
- `start_multi_realtime()`：清空旧 pipeline/forwarder，从 config 加载框列表，
  创建 `MultiBoxPipeline`，spawn 结果转发+状态轮询 task，调用
  `pipeline.start_all()`，显示 overlay。
- `stop_multi_realtime()`：`pipeline.stop_all()`，清空 pipeline/forwarder，
  隐藏 overlay，为每个框发射 `Stopped` 状态。
- `stop_box(box_id)`：`pipeline.stop_box()`，发射 `Stopped` 状态。
- `open_result_window()`：显示并聚焦 result 窗口（已存在则仅置顶不重复创建）。

`TranslationBoxInfo` 使用 `box_id` 字段名（与前端 TypeScript 契约一致），
非 pipeline `TranslationBox.id`。IPC 参数名遵循 Tauri 2 默认 camelCase
（`{ boxId, region }`，见 `tests/contracts.rs`）。

`set_source_language` / `set_target_language` 与 `set_ocr_language` 语义对称：
实时会话运行中拒绝修改（`PipelineError::AlreadyRunning`），仅局部更新配置并
清除缓存的 pipeline。目标语言为 `Language::Auto` 时由配置校验拒绝
（`translation.target_language must not be "auto"`）。

`ocr.language` 与 `translation.source_language` 是**联动字段**
（vtrans-config 的 `validate_language_linkage` 要求二者恒等）：
`set_ocr_language` 与 `set_source_language` 各自同时写入两个字段，任一命令
执行后两字段恒相等，`ConfigManager::save` 的校验不会因联动不一致而拒绝。

`set_api_key` 把 API Key 写入 Windows Credential Manager 中**当前配置
provider** 对应的凭据目标（openai/deepl/google/azure 各一个目标；baidu
写 `baidu_secret`，APP ID 由 `set_provider_credentials` 单独写入），Key
不进入 `config.json`、前端 store、事件或日志。写入通过 `spawn_blocking`
在阻塞池执行；写入成功后立即用新凭据重建当前 provider，无需重启即生效。
空串/纯空白或超过 4096 字符的 Key 返回 `AppError::InvalidApiKey`；`local`
provider 不接受凭据，返回 `AppError::ProviderCredential`。前端参数名为
`{ apiKey }`（Tauri 2 默认 camelCase，命令未加 `rename_all`）。

`set_provider_credentials` 泛化地写入一个云端 provider 的完整凭据集：
OpenAI/DeepL/Google/Azure 传 `apiKey`；Baidu 必须同时传 `appId` 与
`secret`（分别写入 `baidu_app_id` / `baidu_secret` 两个独立目标）。写入后
若目标 provider 就是当前配置的 provider，立即重建。前端参数名为
`{ providerId, apiKey?, appId?, secret? }`（契约见 `tests/contracts.rs`）。

`get_app_config` 返回当前配置的完整快照（clone，不长时间持有锁），前端
挂载时用它水合设置面板，避免整包 `save_settings` 用前端默认值覆盖后端
其它字段（OCR 语言、日志级别、模型目录等）。

`update_result_window_appearance` / `update_floating_ball_appearance` 只
持久化对应窗口的两个外观字段（`result_window.opacity`/`font_size_px` 与
`floating_ball.opacity`/`size_px`）：加载配置 → 修改字段 → `save_config`
（内部校验 + 原子写）。与 `save_settings` 不同，它们**不获取 live 生命周期
锁、不检查 live 任务是否在运行、不重建 Provider**，因此外观调整在实时
会话运行中也能保存（bug 2 后端侧修复）。越界值由配置校验返回
`ConfigError::Validation`，经 `#[from]` 映射为 `AppError::Config`。前端
参数名遵循 Tauri 2 默认 camelCase：`{ opacity, fontSizePx }` 与
`{ opacity, sizePx }`（契约见 `tests/contracts.rs`）。窗口样式本身由前端
按持久化字段应用，命令不触碰窗口 API。

LiveTranslationConfig 包含 region、capture_interval_ms 和 difference_threshold，字段可直接由前端 JSON 反序列化。

`AppStatus.translation_provider` 返回运行时 Provider 的实现 id：云端
Provider 与配置域一致（`"openai"` / `"deepl"` / `"google"` / `"azure"` /
`"baidu"`），本地 Provider 为 `"local-onnx"`（配置域 `"local"`）。前端
`normalizeProviderId` 只需把 `"local-onnx"` 映射回 `"local"`，其余 id
原样透传。

### 翻译 Provider 组装与凭据目标

`state.rs` 的 `build_translation_provider` 按 `config.translation.provider`
分支组装：

| 配置 id | 运行时实现 | 凭据目标（CredentialManager） |
|---------|------------|-------------------------------|
| `openai`（默认） | `OpenAiProvider` | `openai` |
| `deepl` | `DeepLProvider` | `deepl` |
| `google` | `GoogleV2Provider` | `google` |
| `azure` | `AzureTranslatorProvider` | `azure`（区域取 `translation.region`） |
| `baidu` | `BaiduProvider` | `baidu_app_id` + `baidu_secret`（两个独立目标） |
| `local` | `LocalTranslationProvider` | 无（ONNX 模型走 ModelManager） |

配置域白名单为 `["openai","deepl","google","azure","baidu","local"]`（与
vtrans-config 校验一致），默认 `openai`。旧 id `"api"` 已废弃：迁移
（v4→v5）会将其重命名为 `"openai"`，应用层校验拒绝 `"api"`。

凭据只经 `CredentialManager` 读写，不落内存副本到日志；日志引用 Key 时
只用 `vtrans_core::mask_sensitive` / `vtrans_security::mask_key` 掩码值。
切换 provider（`set_translation_provider` / `save_settings`）立即重建并在
配置持久化后生效；live 会话运行中切换仍返回 `PipelineError::AlreadyRunning`。
本地 Provider 加载一次后缓存，再次切到 `local` 近瞬时命中缓存；切换期间发射
`model_loading_progress`（`model_id="translation"`，0.0 → 1.0）。

### Events

emit_pipeline_event 转发以下事件：

- capture_status_changed
- ocr_started
- ocr_completed
- translation_started
- translation_completed
- pipeline_error
- live_session_stopped
- model_loading_progress
- region_selected
- overlay_region_updated
- overlay_hidden
- debug_frame_updated（仅 Debug 模式开启时发射）

#### Multi-box events

- `multibox://result`：单框翻译结果，payload 为 `BoxedTranslationResult`
  的 serde 序列化（含 `box_id`、`color`、`result.translated_text`、`timestamp`）。
- `multibox://box-added`：翻译框新增，payload 含 `box_id`、`color`、`region`。
- `multibox://box-removed`：翻译框删除，payload 含 `box_id`。
- `multibox://box-updated`：翻译框区域更新，payload 含 `box_id`、`region`。
- `multibox://status`：翻译框状态变更，payload 含 `box_id`、`status`
  （`Running` / `Stopped` / `Error(string)`）。
- `multibox://warning`：翻译框过多警告，payload 含 `current_count`、`max_count`。
- `translation://single-result`：单次翻译结果，payload 含 `original_text`、
  `translated_text`、`timestamp`。替代在主页面显示结果，结果推送到翻译弹窗。

Multi-box 事件只包含文本和状态数据，不携带截图图像数据。结果转发 task
（`run_multi_forwarder`）订阅 `pipeline.subscribe_results()` 并以 500ms 轮询
box 状态变更；日志只记录 `box_id` 和事件名，不记录原文/译文完整内容
（原文/译文在 `emit_translation_single_result` 中用 `truncate_for_log` 截断）。

事件只包含标准文本/状态结构，不携带截图图像数据。敏感凭据不会进入事件或日志。

### Debug 模式（捕获帧预览）

Debug 模式是**默认关闭、显式开启**的运行期开关，用于排查「OCR 识别文字与
选区方框内实际文字不符」：开启后，进入 OCR 之前的捕获帧会以缩略图形式
实时显示在主窗口调试面板。

- **开关**：`--debug` 命令行参数或 `VTRANS_DEBUG=1` 环境变量（`true` 亦可，
  大小写不敏感）；解析失败不影响正常启动，只记一行 `info!`（`debug_mode`
  布尔）。开关状态**不写入 config.json**。
- **帧出口**：`vtrans_pipeline::FrameSink` 在捕获帧通过帧差检测（live）或
  捕获完成（single）后、进入 OCR 前调用；关闭时 pipeline 不挂 sink，
  整条调试链路不存在、零开销。
- **编码**：纯函数 `encode_debug_thumbnail` 把 BGRA8/RGBA8 帧缩放到最长边
  ≤ 480px（整数等比缩放，不放大），JPEG 质量 80；在阻塞池执行。
- **传输**：事件 `debug_frame_updated`，payload 为 Base64 JPEG + 区域 +
  帧序号 + 时间戳；只走事件不走命令返回值。节流 ≤ 10fps，watch 通道
  保证最新值语义（旧帧被覆盖），编码失败只 `warn!` 并跳过该帧。
  区域元数据：单次翻译使用命令传入的区域（含显示器坐标），实时会话跟随
  `update_live_region` 的最新选区；帧序号按捕获帧递增，节流或编码失败
  跳过的帧会在序号上留下缺口，便于前端判断丢帧。
- **隐私**：只显示、不保存——不落盘、不进日志、不进 store、不进结果窗口；
  面板只保留内存中最新一帧，Debug 退出后不保留任何帧缓存。
- **红线豁免**：`CapturedImage` 默认禁止跨 IPC；缩略图是 Debug-only 的
  显式豁免，仅在 Debug 开启时发射，生产 Release 默认不启用。

### 窗口生命周期与托盘

关闭主窗口（点 X）不会退出进程：窗口隐藏到系统托盘，实时会话与全局快捷键
继续运行。托盘图标左键单击或菜单「显示主窗口」恢复主窗口；菜单「退出」是
唯一的主动退出路径，进程退出时释放全部快捷键。第二个进程实例启动时会被
单实例插件拦截，并恢复已有实例的主窗口。

### 窗口清单

VTrans 共声明 5 个窗口（`src-tauri/tauri.conf.json`）：

| label | 角色 | 关键配置 |
|-------|------|---------|
| main | 主窗口 | 420×600，可缩放，应用入口 |
| result | 结果窗口 | 360×140（迷你条形态）、默认隐藏、置顶、`transparent: true`、无边框（`decorations: false`）、可缩放 |
| selector | 选区窗口 | 全屏、透明、无边框，默认隐藏 |
| overlay | 选区方框 | 全屏、透明、无边框、置顶、可点穿，默认隐藏 |
| floater | 悬浮球 | 48×48、透明、无边框、置顶、跳过任务栏、不抢焦点（`focus: false`），默认隐藏 |

floater 由前端按配置（`floating_ball.enabled`，vtrans-config 默认 false）
显示：拖动/定位复用既有窗口 API（`startDragging`/`setPosition`/
`show`/`hide`/`setAlwaysOnTop`），**不新增 IPC Command**；悬浮球**不点穿**
（与 overlay 不同，不做 `setIgnoreCursorEvents`）。关闭请求被全局
hide-on-close 策略（prevent_close + hide）覆盖，窗口隐藏而非销毁。

### 选区 overlay

一个无边框、透明、置顶、可点穿的全屏 overlay 窗口覆盖在区域所在显示器上
（窗口原点 = 显示器原点，尺寸 = 显示器尺寸），用纯 CSS 边框在区域相对
偏移处持续标出捕获区域（含尺寸标签）。显示/定位由前端 `regionOverlay`
服务驱动（`availableMonitors` + `setPosition`/`setSize` +
`setIgnoreCursorEvents`），后端同步显示兜底。

**显隐规则按模式区分**（决策集中在 `overlay.rs` 的纯函数
`overlay_intent` / `overlay_intent_for_stop`，均有单元测试）：

- **单次模式**：选区确认（`update_live_region` 传 `mode: "single"`）不显示
  常驻方框；`capture_once` 完成（成功或失败）后确保隐藏，方框不残留。
- **实时模式**：`start_live_translation` 启动即显示；`update_live_region`
  （`mode: "live"`）更新区域时显示新位置；**暂停保留**方框（暂停走
  `stop_live_translation`，后端按 `Pause` 决策不隐藏，恢复后无需重新
  定位）；**真正停止**隐藏（UI 停止按钮先隐藏再 stop，热键 Alt+Shift+S
  走 `Stop` 决策由后端隐藏）。
- 重新选区 / 取消 → 隐藏（事件 `overlay_hidden`）。

`AppStatus.mode`（`"single"` / `"live"`）是后端最近会话模式的权威记录：
单次捕获与确认报告 `single`，实时会话运行或暂停均报告 `live`。前端启动
水合只在 `mode == "live"` 时恢复常驻方框，单次模式的选择区域不会在重启
后显示方框。overlay 窗口不接收鼠标事件，也不传输任何图像数据（只传
`ScreenRegion` 坐标）。

### capability 归属

`src-tauri/capabilities/default.json` 由模块 10 统一维护，`windows` 覆盖
main/result/selector/overlay/floater 五个窗口。清单按前端实际调用的窗口
API 复核：`allow-available-monitors`、`allow-set-position`、
`allow-set-size`、`allow-set-ignore-cursor-events` 由 `regionOverlay`
服务使用（常驻方框的显示器枚举/定位/点穿）；`allow-show`/`allow-hide`/
`allow-set-focus`/`allow-set-always-on-top`/`allow-start-dragging` 由结果
窗口、选区窗口与悬浮球共用（悬浮球的显示/隐藏/置顶/拖动/定位复用既有
权限）；无多余权限。

**透明度能力验证结论（feat/10-floating-ball-window）**：锁定的 Tauri
2.11.5（tauri-runtime-wry 2.11.4、@tauri-apps/api 2.11.1）不提供**运行时**
opacity 能力——Rust 侧 `WebviewWindow` 无 `set_opacity`，JS 侧
`getCurrentWindow().setOpacity()` 不存在，ACL 中也没有
`core:window:allow-set-opacity` 权限（tauri-build 编译期实测报
`Permission core:window:allow-set-opacity not found`；tauri/tauri-runtime/
tauri-runtime-wry/tao/wry 全量源码 grep 无 opacity 命中）。因此 capability
**不含** opacity 权限（透明是窗口配置属性，不需要 ACL）。result 窗口已
声明 `transparent: true`（追加提交，与 overlay/selector 相同的 WebView2
透明机制），前端可直接用 CSS 背景 alpha 实现半透明透出桌面；运行时透明度
调节（setOpacity）不可用，需要时由前端按 `result_window.opacity` 配置在
CSS 层应用。

### Tauri bootstrap

~~~rust
pub fn init_app(app: &mut tauri::App<tauri::Wry>) -> Result<(), AppError>
pub fn builder() -> tauri::Builder<tauri::Wry>
~~~

src-tauri/src/main.rs 使用 builder()、generate_context!() 和 run() 完成宿主启动；capability 仍由宿主项目维护。

init_app 在解析 app_data_dir 之后、创建 AppState 之前，通过
`vtrans_core::init_logging` 初始化 tracing：日志同时输出到控制台和
`app_data_dir/logs`（按小时轮转，保留 5 个文件），级别取配置中的
`log_level`（`RUST_LOG` 环境变量优先）。返回的 `WorkerGuard` 存入 Tauri
管理的 `LoggingGuard`，确保非阻塞写入器在应用退出前完成刷新。

## 构建与测试

在仓库根目录执行：

~~~powershell
cargo fmt --all -- --check
cargo clippy -p vtrans-app --all-targets
cargo test -p vtrans-app
~~~

仅编译应用 crate：

~~~powershell
cargo check -p vtrans-app --all-targets
~~~

编译宿主 Tauri：

~~~powershell
pnpm build
cargo check -p vtrans
~~~

## 日志与安全

- 所有 command、初始化入口和事件转发入口使用 tracing instrumentation。
- 日志在 setup::init_app 中初始化；若 tracing 已被宿主或测试环境初始化，
  初始化失败会降级为不记录滚动文件，应用仍可启动。
- 错误路径记录 warn! 或 error!，正常生命周期记录 info!。
- 凭据通过 `set_api_key` / `set_provider_credentials` 写入 CredentialManager
  的 provider 目标（openai/deepl/google/azure/baidu_app_id/baidu_secret），
  组装时从同一目标读取，不写入 config、事件或日志；日志引用 Key 时仅记录
  `vtrans_core::mask_sensitive` / `vtrans_security::mask_key` 掩码值；
  翻译 Provider 的 upstream crate 负责鉴权注入。
- 前端事件不传递 CapturedImage，避免截图通过 JSON/Base64 跨越 IPC。

## 手工验证项

模块测试计划中的以下项依赖 Windows 桌面环境（Graphics Capture、Credential
Manager、模型文件），无法在无头环境自动化，登记为手工验证：

1. **AppState 初始化**：部署模型文件到 `app_data_dir/models` 后启动应用；确认
   启动日志出现 `application state initialized`，首次运行自动生成 `config.json`。
2. **save_settings 全链路**：修改捕获间隔并保存，重启应用确认配置持久化；
   API 提供者需先在凭据管理器配置 key。
3. **get_app_status 全链路**：启动后前端状态栏显示正确的 Provider、区域和
   pipeline 状态；启动/停止实时会话后轮询结果同步变化。
4. **Provider 切换全链路**：依次切换 openai/deepl/google/azure/baidu/local
   后调用 `get_app_status`，确认 `translation_provider` 返回对应实现 id
   （云端与配置 id 一致，本地为 `"local-onnx"`）；重启后前端引擎开关仍
   显示正确。
5. **快捷键注册与触发**：依次按 Alt+Shift+A（选区）、Alt+Shift+R（实时）、
   Alt+Shift+S（停止），确认动作触发且日志无 `HotkeyFailed`；通过
   `save_settings` 修改热键后重启应用生效。
6. **源/目标语言切换**：在设置面板切换源语言（含 auto）与目标语言
   （zh-CN/ja/en），确认立即生效且 `get_app_status` 后状态正常；实时会话
   运行中切换应返回 `AlreadyRunning` 错误；重启应用确认配置持久化。
7. **凭据全链路**：在设置面板为 openai/deepl/google/azure 输入 API Key、
   为 baidu 输入 APP ID + Secret 保存，确认日志只有掩码形式
   （`sk-****1234`）；重启应用后凭据仍在（Credential Manager），翻译请求
   携带新凭据生效；输入空串/超长值确认前端展示校验错误且不写入凭据。
8. **get_app_config 水合**：在设置面板保存配置后修改配置文件中其它字段
   （如 OCR 语言），重启应用打开设置面板，确认显示后端真实值而非前端默认
   值；整包保存后其它字段不被覆盖。
9. **托盘与窗口生命周期**：点击主窗口关闭按钮，确认主窗口隐藏、进程仍在、
   托盘出现图标；左键单击托盘恢复主窗口；托盘菜单「退出」后进程结束且
   全局快捷键不再占用（再次按下 Alt+Shift+A/R/S 不触发本应用）。
10. **选区 overlay**：单次模式框选确认后**不出现**常驻边框，翻译完成
    （成功或失败）后仍无残留；切到实时模式启动后出现与选区对齐的常驻
    边框（含尺寸标签），实时运行中更新区域边框同步移动/缩放；暂停保留
    边框，停止实时或重新选区时边框消失；框选区域内的鼠标操作（点击、
    拖动）不受 overlay 影响。
11. **单实例**：应用运行中再次启动 vtrans.exe，确认不会出现第二个进程，
    已有实例的主窗口被恢复显示。
12. **Debug 模式**：以 `VTRANS_DEBUG=1` 启动应用，主窗口出现「调试：捕获
    帧」面板；实时翻译或单次翻译运行时面板显示进入 OCR 前的区域缩略图
    （≤480px），帧号递增且刷新率 ≤10fps；确认面板图像与 OCR 输出一致。
    关闭 Debug 正常启动时无面板、无 `debug_frame_updated` 事件。
13. **悬浮球窗口**：启动应用确认无悬浮球（默认隐藏）；开启
    `floating_ball.enabled` 后出现 48×48 透明置顶悬浮球，可拖动且不点穿
    （悬浮球区域仍能接收鼠标）；点击悬浮球唤起主窗口。
14. **结果窗口无边框透明（方案 1 迷你条）**：确认 result 窗口以 360×140
    迷你条形态启动（默认隐藏），无原生标题栏（`decorations: false`），
    拖动区域与关闭按钮由前端提供（`data-tauri-drag-region` /
    `startDragging`，capability 已含 `allow-start-dragging`）；模块 11 合并
    前前端背景仍不透明，无可见回归；前端按 `result_window.opacity` 应用
    CSS 背景 alpha 后，半透明内容透出桌面（无需 setOpacity，窗口已声明
    `transparent: true`）。同时检查 Windows 下透明窗口的文字渲染清晰、
    缩放重绘无残留、原生阴影表现正常（若有不完善处见「已知限制」建议）。
15. **框选期间窗口隐藏与恢复（Bug-005）**：开启 `floating_ball.enabled`
    并显示悬浮球，再打开结果窗口（单次翻译后结果弹窗保持可见），然后按
    Alt+Shift+A 或点击主窗口/悬浮球「框选翻译」：
    - 框选开始的瞬间，主窗口、结果窗口、悬浮球全部隐藏，屏幕上只剩透明
      选区窗口，被框选内容不被遮挡；
    - 框选确认并完成单次翻译后（成功与失败都要检查），三个窗口恢复为
      **框选前**的可见状态（框选前不可见的窗口保持不可见，例如结果窗口
      若框选前没打开，恢复后仍然隐藏）；
    - 按 Esc 取消、等待超时或选区窗口异常时，窗口**立即**恢复；
    - 实时模式框选（选后启动实时）与多框添加/编辑框路径同样在动作命令
      完成后恢复；
    - 关闭 `floating_ball.enabled` 后再框选：框选前悬浮球即使可见（前端
      仍显示），恢复后悬浮球保持隐藏；
    - 连续快速框选（例如上次翻译失败后立即重试）时，恢复集合以**首次**
      框选的快照为准，不丢失窗口；
    - 框选结束后按 Alt+Shift+R 恢复实时（非框选后续路径）不应影响悬浮球
      等窗口的正常启停。

以上各项的纯逻辑部分已有自动化测试：Provider 值域校验与配置更新
（`validate_translation_provider_id` / `update_translation_provider_config`）、
`AppStatus` 序列化契约、语言配置更新与目标语言校验、错误映射与事件转换。

云端 Provider 的运行时 id 与配置 id 一致（`"openai"`/`"deepl"`/
`"google"`/`"azure"`/`"baidu"`）；仅本地不同（实现 `"local-onnx"` ↔ 配置
`"local"`）。**新增翻译 Provider 时**，必须同步更新后端
`validate_translation_provider_id` 白名单、凭据目标映射与前端
`normalizeProviderId` 映射，否则重启后前端引擎开关会错误回退。

## 已知限制

- AppState::new 需要可用的 Windows Graphics Capture 环境、模型 manifest 和对应模型文件；模型未部署时启动会返回 AppError::Model/AppError::Capture。
- 选区窗口的最终坐标由前端通过 update_live_region（携带当前模式
  `"single"` / `"live"`）确认；start_region_selection 会等待确认结果，
  Escape/关闭操作应调用 cancel_region_selection。
- 模型完整性校验通过 blocking pool 执行，避免大文件 SHA-256 计算阻塞 Tokio worker。
- 切换翻译 Provider（`set_translation_provider` / `save_settings`）会在 blocking
  pool 中加载本地 ONNX 模型（tokenizer + session），期间持有生命周期锁，
  其他启动/停止命令会等待切换完成。已加载的本地 Provider 会被缓存：首次切到
  `local` 走重路径（SHA-256 校验 + ONNX session commit + 全图优化），之后切回
  云端再切到 `local` 命中缓存、近瞬时生效。切换路径发射 `model_loading_progress`
  事件（`model_id="translation"`，0.0 → 1.0），命中缓存时近瞬时发 1.0。模型目录
  变更（`AppConfig.model_dir`）会失效缓存并重新加载。
- 全局快捷键由配置字符串解析，冲突或非法快捷键会在启动时返回 HotkeyFailed；当前没有 UI 内热键冲突编辑器。
  通过 `save_settings` 修改热键配置后需要重启应用才会重新注册。
- 单次捕获的进度事件（ocr_started 等）由 capture_once 转发，但命令契约仍只返回 OcrResult；前端如需进度提示需同时监听 ocr_started/translation_started。
- 关闭主窗口是「隐藏到托盘」而非退出；只有托盘菜单「退出」或任务管理器
  能结束进程，首次使用时需注意托盘图标的存在。
- overlay 仅覆盖区域所在显示器（selector 全屏所在的显示器）；选区窗口目前
  不支持跨显示器拖拽，因此框选区域始终位于该显示器内。
- overlay 边框会被 Windows Graphics Capture 截入画面（位于选区边缘，2px），
  对 OCR 文本行检测的影响可忽略；这是"屏幕常驻标记"的固有代价。
- Debug 缩略图为 JPEG（有损）且最长边 ≤ 480px，仅用于核对 OCR 输入，不
  保证与原始帧逐像素一致；帧差未触发时（画面无变化）不产生新调试帧。
- 常驻方框为纯 CSS 边框，不显示区域真实缩略图；如需屏幕缩略预览，需架构
  确认后由 vtrans-app 提供小尺寸位图事件/命令（`CapturedImage` 不得跨
  IPC），前端方框已兜底，不阻塞 MVP。
- capability 清单由模块 10 统一维护（见「capability 归属」一节）；生产构建
  应按窗口和 command 最小化 capability。
- 锁定的 Tauri 2.11.5 不提供**运行时**窗口 opacity 能力（Rust/JS/ACL 均
  无，`core:window:allow-set-opacity` 不存在，添加会导致构建失败）；result
  窗口已声明 `transparent: true`（窗口级透明属性，无需 ACL），前端用 CSS
  背景 alpha 实现半透明。Windows 透明窗口的已知注意事项：result 已声明
  `decorations: false`（无原生标题栏），标题栏/边框由前端 CSS 提供，原生
  拖动与关闭按钮不可用（前端用 `data-tauri-drag-region` /
  `startDragging` 与自定义关闭按钮兜底，capability 已含
  `allow-start-dragging`）；原生阴影在分层窗口（WS_EX_LAYERED）上可能
  不渲染或呈矩形，需要时建议 `shadow: false` + CSS box-shadow；文字渲染
  与缩放重绘依赖 WebView2 Evergreen 运行时，overlay/selector 已用同一
  机制渲染文字标签，表现一致。
- 悬浮球窗口默认隐藏（`visible: false`），由前端按 `floating_ball.enabled`
  配置显示；全局 hide-on-close 策略对 floater 同样生效（关闭隐藏不销毁）。
  悬浮球不点穿，未配置 `setIgnoreCursorEvents`。
- 多框实时翻译的 `multibox://result` 事件 payload 为
  `BoxedTranslationResult`：含 `result.translated_text` 与 `original_text`
  （F1/F2 落地）。`original_text` 为清洗后的 OCR 原文——与发送给翻译
  provider 的文本同源，供弹窗每框同时显示原文+译文。降级语义：OCR 产出
  空文本（跳过 provider 调用）或翻译失败时，仍发布译文与原文均为空串的
  结果，前端据此清除该框 overlay 残留而非保留旧译文；取消（stop/被新
  任务取代）不发布任何结果。单次翻译的 `translation://single-result`
  同样携带 `original_text` / `translated_text` / `timestamp`。
- 多框 overlay 渲染由前端负责：app 层显示 overlay 窗口并发射
  `multibox://box-added` 等事件，彩色方框的实际绘制由前端 CSS 完成。
  app 层不跨 IPC 传输图像，仅传 `box_id`、`color`、`region` 坐标。
- 多框状态轮询间隔为 500ms：forwarder task 通过 `pipeline.box_status()`
  轮询，运行中新增的框在下一个轮询周期纳入监控；错误状态变更最多有
  500ms 延迟。
- 多框与单框实时翻译互不影响：`MultiBoxPipeline` 与 `Pipeline` 是独立实例，
  共享相同的 provider 但拥有独立的 CaptureSession。用户可同时运行单框
  实时翻译和多框实时翻译（但资源消耗加倍）。
- `add_translation_box` 使用 `load_config` + `save_config`（非原子）持久化
  翻译框，因 `add_box_config` 需返回分配的 id/color。其他命令使用
  `update_config`（原子加载-修改-保存）。桌面应用场景下并发写入概率极低。
- 框选期间的窗口隐藏/恢复基于**框选前可见集合快照**（Bug-005）：框选开始
  时记录 main/result/floater 的可见性，恢复只重新显示快照中可见的窗口，
  floater 额外受 `floating_ball.enabled` 约束。快照「首次优先、恢复即清」：
  若框选成功后前端未调用 `capture_once` / `start_live_translation` /
  `add_translation_box` / `update_translation_box` 之一，窗口保持隐藏，直到
  下一个这些命令完成（无论成败）或下一次框选被取消/超时。窗口可见性查询
  失败时按「不可见」处理（不隐藏也不恢复该窗口），保证不会把窗口弄丢。
