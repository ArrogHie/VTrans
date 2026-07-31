# vtrans-core

VTrans 核心类型与接口模块：定义全项目共享的数据结构、Provider trait、错误类型和日志初始化工具，是所有其他 crate 的依赖基础。

## 模块职责

- 核心数据结构：`Language`、`ScreenRegion`、`CapturedImage`、`OcrResult`、`TranslationRequest`、`TranslationResult`、`PipelineMode`、`PipelineStatus` 等
- Provider trait：`OcrProvider`、`TranslationProvider`、`CaptureSource`、`CaptureSession`
- 错误类型：`CoreError`，以及 trait 引用的 `CaptureError`、`OcrError`、`TranslationError`
- 日志初始化：`init_logging()` 控制台 + 按小时轮转文件输出，保留 5 个文件

## 依赖关系

- 上游 crate：无（层级 0）
- 外部 crate：`serde`、`serde_json`、`async-trait`、`thiserror`、`tracing`、`tracing-subscriber`、`tracing-appender`、`tokio-util`
- 下游 crate：所有其他 `vtrans-*` crate 均依赖本 crate，必须从此处导入共享类型，禁止重复定义

## 公开 API 概要

### 类型（`vtrans_core::types`）

```rust
pub enum Language { Auto, ChineseSimplified, Japanese, English }
impl Language {
    pub const fn code(self) -> &'static str;
    pub fn from_code(code: &str) -> Option<Self>;
    pub const fn is_auto(self) -> bool;
    pub const fn display_name(self) -> &'static str;
    pub const fn all_concrete() -> &'static [Self];
}

pub struct ScreenRegion { pub monitor_id: String, pub x: i32, pub y: i32, pub width: u32, pub height: u32 }
impl ScreenRegion {
    pub fn new(monitor_id: impl Into<String>, x: i32, y: i32, width: u32, height: u32) -> Self;
    pub fn validate(&self) -> Result<(), CoreError>;
    pub fn is_valid(&self) -> bool;
}

pub enum PixelFormat { Rgba8, Bgra8 }

pub struct CapturedImage { pub width: u32, pub height: u32, pub format: PixelFormat, pub data: Vec<u8> }
impl CapturedImage {
    pub fn new(width: u32, height: u32, format: PixelFormat, data: Vec<u8>) -> Result<Self, CoreError>;
    pub fn check_format(&self, expected: PixelFormat) -> Result<(), CoreError>;
    pub fn validate(&self) -> Result<(), CoreError>;
}

pub struct OcrLine { pub text: String, pub confidence: f32, pub polygon: [[f32; 2]; 4], pub reading_order: usize }
pub struct OcrResult { pub lines: Vec<OcrLine>, pub merged_text: String, pub detected_language: Option<Language>, pub elapsed_ms: u64 }
pub struct OcrOptions { pub language: Language, pub min_confidence: f32, pub detect_vertical: bool }
pub struct TranslationRequest { pub text: String, pub source: Language, pub target: Language }
pub struct TranslationResult { pub translated_text: String, pub provider_id: String, pub elapsed_ms: u64 }
pub enum PipelineMode { SingleCapture, LiveRegion }
pub enum PipelineStatus { Idle, Capturing, OcrInProgress, Translating, Completed, Error(String) }
```

所有类型派生 `Debug`、`Clone`、`Serialize`、`Deserialize`，除 `CapturedImage` 仅派生 `Clone`（防止图像数据被序列化到 JSON/IPC）。

### Trait（`vtrans_core::traits`）

```rust
#[async_trait]
pub trait OcrProvider: Send + Sync {
    fn id(&self) -> &'static str;
    async fn recognize(&self, image: &CapturedImage, region: &ScreenRegion,
        options: &OcrOptions, cancel: CancellationToken) -> Result<OcrResult, OcrError>;
    fn supported_languages(&self) -> &[Language];
}

#[async_trait]
pub trait TranslationProvider: Send + Sync {
    fn id(&self) -> &'static str;
    async fn translate(&self, request: &TranslationRequest,
        cancel: CancellationToken) -> Result<TranslationResult, TranslationError>;
    fn supported_pairs(&self) -> &[(Language, Language)];
}

#[async_trait]
pub trait CaptureSource: Send + Sync {
    async fn capture_once(&self, region: &ScreenRegion) -> Result<CapturedImage, CaptureError>;
    async fn start_session(&self, region: &ScreenRegion) -> Result<Box<dyn CaptureSession>, CaptureError>;
}

#[async_trait]
pub trait CaptureSession: Send {
    async fn next_frame(&mut self) -> Result<Option<CapturedImage>, CaptureError>;
    async fn stop(&mut self) -> Result<(), CaptureError>;
}
```

### 错误类型（`vtrans_core::error`）

`CoreError`（类型校验/序列化）、`CaptureError`、`OcrError`、`TranslationError`。所有错误均派生 `thiserror::Error`，trait 相关错误由各下游实现 crate 直接导入，不重复定义。

### 日志工具（`vtrans_core::logging`）

```rust
pub fn init_logging(log_dir: &Path, level: &str) -> Result<WorkerGuard, std::io::Error>;
pub fn mask_sensitive(s: &str) -> String;      // sk-****1234
pub fn truncate_for_log(s: &str) -> String;    // 前 20 字符 + ...
```

## 构建与测试

```powershell
cargo check -p vtrans-core
cargo test -p vtrans-core
cargo clippy -p vtrans-core --all-targets
cargo fmt -p vtrans-core -- --check
```

## 已知限制

- `CapturedImage` 不实现 `Serialize`，因此无法直接通过 Tauri IPC 传输图像；图像应留在 Rust 侧，前端只接收文本和状态。
- `tracing-appender` 不支持按大小轮转，日志按小时轮转，保留 5 个文件（与规格中"10MB 轮转"的原始要求最接近的可行替代）。
- `init_logging` 只能调用一次；重复调用会触发 `tracing_subscriber` 全局注册断言。
- `mask_sensitive` 按字节长度截断，仅适用于 ASCII 为主的密钥；非 ASCII 敏感串建议先经外部编码处理。

## 详细规格

参见 `docs/modules/01-core.md`。