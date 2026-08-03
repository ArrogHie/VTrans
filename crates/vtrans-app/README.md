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
- vtrans-text：由 pipeline 间接使用。
- vtrans-translation：API 和本地 Provider。
- vtrans-models：manifest、模型路径和完整性报告。
- vtrans-pipeline：捕获、OCR、标准化和翻译编排。

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
set_translation_provider(provider_id: String) -> Result<(), AppError>
load_local_models() -> Result<VerifyReport, AppError>
save_settings(settings: AppConfig) -> Result<(), AppError>
get_app_status() -> Result<AppStatus, AppError>
~~~

`capture_once` 在流水线运行期间并发消费事件通道，把 `ocr_started`、
`translation_started` 等阶段事件推送到前端（单次捕获没有 live session，
因此不发送 `live_session_stopped`），命令本身仍只返回最终的 `OcrResult`。

LiveTranslationConfig 包含 region、capture_interval_ms 和 difference_threshold，字段可直接由前端 JSON 反序列化。

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

## 已知限制

- AppState::new 需要可用的 Windows Graphics Capture 环境、模型 manifest 和对应模型文件；模型未部署时启动会返回 AppError::Model/AppError::Capture。
- 选区窗口的最终坐标由前端通过 update_live_region 确认；start_region_selection 会等待确认结果，Escape/关闭操作应调用 cancel_region_selection。
- 模型完整性校验通过 blocking pool 执行，避免大文件 SHA-256 计算阻塞 Tokio worker。
- 全局快捷键由配置字符串解析，冲突或非法快捷键会在启动时返回 HotkeyFailed；当前没有 UI 内热键冲突编辑器。
- 单次捕获的进度事件（ocr_started 等）由 capture_once 转发，但命令契约仍只返回 OcrResult；前端如需进度提示需同时监听 ocr_started/translation_started。
- Tauri capability 文件仍由宿主项目维护；生产构建应按窗口和 command 最小化 capability。
