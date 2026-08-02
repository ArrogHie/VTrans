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
- serde/serde_json：IPC payload 和错误序列化。
- thiserror：错误枚举和错误链。
- tracing：结构化生命周期和错误日志。

所有依赖使用 MIT 或 Apache-2.0 兼容许可证；本 crate 没有新增 workspace 根依赖。

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

事件只包含标准文本/状态结构，不携带截图图像数据。敏感凭据不会进入事件或日志。

### Tauri bootstrap

~~~rust
pub fn init_app(app: &mut tauri::App<tauri::Wry>) -> Result<(), AppError>
pub fn builder() -> tauri::Builder<tauri::Wry>
~~~

桌面入口只需要在 Tauri Builder 中安装本 crate 的 builder/setup，或使用 builder() 的 command、plugin 和 setup 配置。

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

## 日志与安全

- 所有 command、初始化入口和事件转发入口使用 tracing instrumentation。
- 错误路径记录 warn! 或 error!，正常生命周期记录 info!。
- API key 从 CredentialManager 读取，不写入 config、事件或日志；翻译 Provider 的 upstream crate 负责 bearer token 注入。
- 前端事件不传递 CapturedImage，避免截图通过 JSON/Base64 跨越 IPC。

## 已知限制

- AppState::new 需要可用的 Windows Graphics Capture 环境、模型 manifest 和对应模型文件；模型未部署时启动会返回 AppError::Model/AppError::Capture。
- 选区窗口的最终坐标由前端确认并通过 update_live_region 写回；在确认前 start_region_selection 返回 NotInitialized，不会伪造坐标。
- 模型完整性校验目前是同步文件校验，命令本身是 async 但校验仍会占用运行时 worker；后续可迁移到专用 blocking pool。
- 全局快捷键由配置字符串解析，冲突或非法快捷键会在启动时返回 HotkeyFailed；当前没有 UI 内热键冲突编辑器。
- 当前应用层未修改 src-tauri 的 capability 文件；生产构建仍应在 Tauri 配置中按窗口和 command 最小化 capability。
