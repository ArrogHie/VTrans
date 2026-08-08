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
- vtrans-translation：API Provider 与本地 Native 双引擎 Provider
  （Bergamot en→zh + CTranslate2 ja→zh，经 `translation_bridge.dll` FFI）。
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
get_app_config() -> Result<AppConfig, AppError>
get_app_status() -> Result<AppStatus, AppError>
~~~

`capture_once` 在流水线运行期间并发消费事件通道，把 `ocr_started`、
`translation_started` 等阶段事件推送到前端（单次捕获没有 live session，
因此不发送 `live_session_stopped`），命令本身仍只返回最终的 `OcrResult`。

`set_ocr_language` 与 `set_source_language` 是**联动设置**：任一命令都会把
`config.ocr.language` 与 `config.translation.source_language` 同步为同一个
值（后端权威联动，`apply_ocr_language` / `apply_source_language` 纯函数）。
实时会话运行中拒绝修改（`PipelineError::AlreadyRunning`）；保存后清除
缓存的 pipeline。`set_target_language` 只改目标语言，`Language::Auto` 由
配置校验拒绝（`translation.target_language must not be "auto"`）。整包
`save_settings` 若提交两字段不一致的配置，由 `vtrans-config` v4 跨字段
校验拒绝并返回 `AppError::Config`。

`set_api_key` 把翻译 API Key 写入 Windows Credential Manager（target 固定为
`"translation"`，与 `load_api_key` 读取的 target 一致），Key 不进入
`config.json`、前端 store、事件或日志。写入通过 `spawn_blocking` 在阻塞池
执行；当 `config.translation.provider == "api"` 时，保存后用新 Key 重建 API
provider，无需重启即生效。空串/纯空白或超过 4096 字符的 Key 返回
`AppError::InvalidApiKey`。前端参数名为 `{ apiKey }`（Tauri 2 默认
camelCase，命令未加 `rename_all`）。

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

`AppStatus.translation_provider` 返回运行时 Provider 的实现 id（`"api"` /
`"local-native"`），与 `set_translation_provider` 接受的配置标识符（`"api"` /
`"local"`）值域不同；前端 `normalizeProviderId` 负责把实现 id 映射回配置标识符。

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
- API key 通过 `set_api_key` 写入 CredentialManager（target `"translation"`），
  从 CredentialManager 读取，不写入 config、事件或日志；日志引用 Key 时仅
  记录 `vtrans_core::mask_sensitive` 掩码值；翻译 Provider 的 upstream crate
  负责 bearer token 注入。
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
4. **Provider 切换全链路**：切换 api/local 后调用 `get_app_status`，确认
   `translation_provider` 返回对应实现 id（`"api"`/`"local-native"`），重启后
   前端引擎开关仍显示正确。
5. **快捷键注册与触发**：依次按 Alt+Shift+A（选区）、Alt+Shift+R（实时）、
   Alt+Shift+S（停止），确认动作触发且日志无 `HotkeyFailed`；通过
   `save_settings` 修改热键后重启应用生效。
6. **语言联动与目标语言切换**：在设置面板切换源语言（含 auto）后，确认
   OCR 语言同步变为同一值（反之亦然），`get_app_status` 状态正常且
   config.json 两字段一致；目标语言（zh-CN/ja/en）独立切换不受影响；实时
   会话运行中切换应返回 `AlreadyRunning` 错误；重启应用确认配置持久化。
   手工构造两字段不一致的 `config.json` 后整包保存，确认被
   `vtrans-config` 校验拒绝。
7. **set_api_key 全链路**：在设置面板输入 API Key 保存，确认日志只有掩码
   形式（`sk-****1234`）；重启应用后 Key 仍在（Credential Manager），且
   provider 为 `"api"` 时翻译请求携带新 Key 生效；输入空串/超长 Key 确认
   前端展示校验错误且不写入凭据。
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
15. **本地 Native 双引擎加载**：部署 manifest v2 翻译模型（`en-zh`/
    `ja-zh`）与 `translation_bridge.dll`（构建机产出，位于
    `src-tauri/resources/native/`）后，把 provider 切换为 local：确认
    启动/切换日志出现 `native translation provider loaded`，
    `get_app_status` 返回 `"local-native"`；框选日文区域实时翻译，确认
    ja→zh-CN 由 CTranslate2 引擎翻译、en→zh-CN 由 Bergamot 引擎翻译；
    把 `translation.quality` 改为 `"balanced"` 后重新加载 Provider，确认
    日志与翻译结果反映更高 beam 档位。

以上各项的纯逻辑部分已有自动化测试：Provider 值域校验与配置更新
（`validate_translation_provider_id` / `update_translation_provider_config`）、
`AppStatus` 序列化契约（含 A2 运行时 id `"local-native"`）、语言联动纯函数
（`apply_ocr_language` / `apply_source_language` 双向同步）、quality 解析
（`fast`/`balanced` 接受、非法值拒绝）、错误映射与事件转换。

`AppStatus.translation_provider` 与 `set_translation_provider` 使用不同的
标识符域（实现 id `"api"`/`"local-native"` ↔ 配置 id `"api"`/`"local"`）。
**新增翻译 Provider 时**，必须同步更新后端 `validate_translation_provider_id`
白名单与前端 `normalizeProviderId` 映射，否则重启后前端引擎开关会错误回退
显示为 API。

## 已知限制

- AppState::new 需要可用的 Windows Graphics Capture 环境、模型 manifest 和对应模型文件；模型未部署时启动会返回 AppError::Model/AppError::Capture。
- 选区窗口的最终坐标由前端通过 update_live_region（携带当前模式
  `"single"` / `"live"`）确认；start_region_selection 会等待确认结果，
  Escape/关闭操作应调用 cancel_region_selection。
- 模型完整性校验通过 blocking pool 执行，避免大文件 SHA-256 计算阻塞 Tokio worker。
- 切换翻译 Provider（`set_translation_provider` / `save_settings`）会在 blocking
  pool 中重新加载本地 Native 双引擎（Bergamot + CTranslate2，含
  `translation_bridge.dll` 动态加载），期间持有生命周期锁，其他启动/停止
  命令会等待切换完成。
- 本地 Native Provider 需要 `translation_bridge.dll`（由 07 的构建机产出，
  声明于 `src-tauri/tauri.conf.json` 的 `bundle.resources`，打包后位于
  安装目录 `resources/native/`）与 manifest v2 翻译模型；缺失时切换 local
  会返回 `AppError::Translation`，不影响 API Provider 路径。
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
