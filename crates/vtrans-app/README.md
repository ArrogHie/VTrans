# vtrans-app

VTrans 的 Rust 应用层：组装各模块的生产实现，提供 Tauri Commands/Events、应用状态生命周期和全局快捷键，是 Rust 与前端 IPC 的唯一桥梁。

## 职责

- 通过 AppState 组装 config、security、capture、OCR、translation、models 和 pipeline。
- 提供手动单次翻译、实时翻译、区域更新、语言/Provider 切换、设置持久化和状态查询命令。
- 将 vtrans_pipeline::PipelineEvent 转换为稳定的前端事件名和 JSON payload。
- 注册配置中的全局快捷键，并把快捷键动作派发到选区、实时翻译和停止流程。
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

所有依赖使用 MIT 或 Apache-2.0 兼容许可证；新增依赖仅在本 crate 的 Cargo.toml 中声明。

## 公开 API 概要

~~~rust
pub struct AppState { /* managed by Tauri */ }

impl AppState {
    pub fn new(app_data_dir: &Path) -> Result<Self, AppError>;
}

pub struct AppStatus {
    pub pipeline_status: PipelineStatus,
    pub ocr_provider: String,
    pub translation_provider: String,
    pub selected_region: Option<ScreenRegion>,
    pub live_running: bool,
    pub model_progress: Option<f32>,
}
~~~

AppState::new 从 app_data_dir/config.json 加载配置，默认从 app_data_dir/models/manifest.json 加载模型 manifest；AppConfig.model_dir 可以覆盖模型目录。

### Tauri Commands

通过 setup::builder() 注册以下 command：

~~~rust
start_region_selection() -> Result<ScreenRegion, AppError>
cancel_region_selection() -> Result<(), AppError>
capture_once(region: ScreenRegion) -> Result<OcrResult, AppError>
start_live_translation(config: LiveTranslationConfig) -> Result<(), AppError>
stop_live_translation() -> Result<(), AppError>
update_live_region(region: ScreenRegion) -> Result<(), AppError>
set_ocr_language(language: Language) -> Result<(), AppError>
set_source_language(language: Language) -> Result<(), AppError>
set_target_language(language: Language) -> Result<(), AppError>
set_translation_provider(provider_id: String) -> Result<(), AppError>
load_local_models() -> Result<VerifyReport, AppError>
save_settings(settings: AppConfig) -> Result<(), AppError>
get_app_status() -> Result<AppStatus, AppError>
~~~

`capture_once` 在流水线运行期间并发消费事件通道，把 `ocr_started`、
`translation_started` 等阶段事件推送到前端（单次捕获没有 live session，
因此不发送 `live_session_stopped`），命令本身仍只返回最终的 `OcrResult`。

`set_source_language` / `set_target_language` 与 `set_ocr_language` 语义对称：
实时会话运行中拒绝修改（`PipelineError::AlreadyRunning`），仅局部更新配置并
清除缓存的 pipeline。目标语言为 `Language::Auto` 时由配置校验拒绝
（`translation.target_language must not be "auto"`）。

LiveTranslationConfig 包含 region、capture_interval_ms 和 difference_threshold，字段可直接由前端 JSON 反序列化。

`AppStatus.translation_provider` 返回运行时 Provider 的实现 id（`"api"` /
`"local-onnx"`），与 `set_translation_provider` 接受的配置标识符（`"api"` /
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

事件只包含标准文本/状态结构，不携带截图图像数据。敏感凭据不会进入事件或日志。

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
- API key 从 CredentialManager 读取，不写入 config、事件或日志；翻译 Provider 的 upstream crate 负责 bearer token 注入。
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
   `translation_provider` 返回对应实现 id（`"api"`/`"local-onnx"`），重启后
   前端引擎开关仍显示正确。
5. **快捷键注册与触发**：依次按 Alt+Shift+A（选区）、Alt+Shift+R（实时）、
   Alt+Shift+S（停止），确认动作触发且日志无 `HotkeyFailed`；通过
   `save_settings` 修改热键后重启应用生效。
6. **源/目标语言切换**：在设置面板切换源语言（含 auto）与目标语言
   （zh-CN/ja/en），确认立即生效且 `get_app_status` 后状态正常；实时会话
   运行中切换应返回 `AlreadyRunning` 错误；重启应用确认配置持久化。

以上各项的纯逻辑部分已有自动化测试：Provider 值域校验与配置更新
（`validate_translation_provider_id` / `update_translation_provider_config`）、
`AppStatus` 序列化契约、语言配置更新与目标语言校验、错误映射与事件转换。

`AppStatus.translation_provider` 与 `set_translation_provider` 使用不同的
标识符域（实现 id `"api"`/`"local-onnx"` ↔ 配置 id `"api"`/`"local"`）。
**新增翻译 Provider 时**，必须同步更新后端 `validate_translation_provider_id`
白名单与前端 `normalizeProviderId` 映射，否则重启后前端引擎开关会错误回退
显示为 API。

## 已知限制

- AppState::new 需要可用的 Windows Graphics Capture 环境、模型 manifest 和对应模型文件；模型未部署时启动会返回 AppError::Model/AppError::Capture。
- 选区窗口的最终坐标由前端通过 update_live_region 确认；start_region_selection 会等待确认结果，Escape/关闭操作应调用 cancel_region_selection。
- 模型完整性校验通过 blocking pool 执行，避免大文件 SHA-256 计算阻塞 Tokio worker。
- 切换翻译 Provider（`set_translation_provider` / `save_settings`）会在 blocking
  pool 中重新加载本地 ONNX 模型（tokenizer + session），期间持有生命周期锁，
  其他启动/停止命令会等待切换完成。
- 全局快捷键由配置字符串解析，冲突或非法快捷键会在启动时返回 HotkeyFailed；当前没有 UI 内热键冲突编辑器。
  通过 `save_settings` 修改热键配置后需要重启应用才会重新注册。
- 单次捕获的进度事件（ocr_started 等）由 capture_once 转发，但命令契约仍只返回 OcrResult；前端如需进度提示需同时监听 ocr_started/translation_started。
- Tauri capability 文件仍由宿主项目维护；生产构建应按窗口和 command 最小化 capability。
