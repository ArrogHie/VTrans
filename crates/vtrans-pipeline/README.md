# vtrans-pipeline

流水线编排模块：把屏幕采集、OCR、文本标准化与翻译串成单次截屏和实时区域翻译两条链路，并统一处理帧差检测、任务取消、文本去重与背压。

## 1. 模块概述

`vtrans-pipeline` 是 VTrans 的第 3 层（编排层）：它不实现任何具体能力，只通过 `vtrans-core` 的
`CaptureSource` / `OcrProvider` / `TranslationProvider` trait 编排下游模块。

边界：

- 做：单次截屏翻译、固定区域实时翻译、帧差检测、有界通道、旧任务取消、文本指纹去重、停止清理。
- 不做：屏幕采集（`vtrans-capture`）、OCR 推理（`vtrans-ocr`）、文本清洗/切段（`vtrans-text`）、翻译请求（`vtrans-translation`）。
- 不直接依赖具体 Provider 实现：全部通过 trait 依赖注入（`PipelineDeps`）。
- 不跨 IPC 传输图像：`CapturedImage` 留在 Rust 侧，事件通道只传文本、状态与耗时。

## 2. 依赖关系

| 类型 | Crate | 用途 |
|------|-------|------|
| 上游 | `vtrans-core` | `CaptureSource` / `OcrProvider` / `TranslationProvider` trait、`CapturedImage` / `OcrResult` / `TranslationRequest` 等类型、`CaptureError` / `OcrError` / `TranslationError`、`truncate_for_log` |
| 上游 | `vtrans-text` | `TextNormalizer`（clean / merge_lines / fingerprint / split_paragraphs_default）、`japanese::normalize_punctuation`、`DEFAULT_MAX_PARAGRAPH_LEN` |
| 上游 | `vtrans-capture` / `vtrans-ocr` / `vtrans-translation` | 依赖声明（模块规格约定）；代码只引用 `vtrans-core` 中对应 trait |
| 外部 | `tokio` | 异步任务、`mpsc` 有界通道、`Notify`、超时与 sleep |
| 外部 | `tokio-util` | `CancellationToken` 协作取消 |
| 外部 | `thiserror` | `PipelineError` 错误枚举派生 |
| 外部 | `tracing` | 结构化日志（入口 `#[instrument]`、错误路径 `warn!`/`error!`） |
| 外部 | `serde` | `TranslationBox` / `BoxedTranslationResult` 的 `Serialize`/`Deserialize`（用于 IPC 传输） |
| dev | `async-trait` | 集成测试中 mock provider 的 trait 实现 |
| dev | `serde_json` | 单元测试中的序列化往返测试 |

新增外部依赖 `serde`：用于 `TranslationBox` / `BoxedTranslationResult` 的 `Serialize`/`Deserialize`，
满足跨 IPC 序列化传输需求。取消协调（`cancel::TaskSlot`）、帧差检测（`dedup::FrameDiffer`）、
文本去重（`dedup::TextDedup`）、多框管理（`multibox::MultiBoxPipeline`）均为本 crate 实现。

## 3. 公开 API 概要

```rust
pub struct PipelineConfig {
    pub mode: PipelineMode,             // SingleCapture | LiveRegion
    pub region: ScreenRegion,
    pub capture_interval_ms: u32,       // 实时模式；<16ms 会被钳制到 16ms
    pub difference_threshold: f32,      // 实时模式；像素差异比例阈值，越界值会被钳制到 0..=1
    pub ocr_options: OcrOptions,
    pub translation_request: TranslationRequest, // text 字段在每次翻译时被 OCR 结果替换
}
impl PipelineConfig {
    pub fn new(mode, region, capture_interval_ms, difference_threshold, ocr_options, translation_request) -> Self;
    pub fn single(region, ocr_options, translation_request) -> Self;
    pub fn live(region, capture_interval_ms, difference_threshold, ocr_options, translation_request) -> Self;
}

pub struct PipelineDeps {
    pub capture: Box<dyn CaptureSource>,
    pub ocr: Box<dyn OcrProvider>,
    pub translation: Box<dyn TranslationProvider>,
}

pub enum PipelineEvent {
    CaptureStarted,
    OcrStarted,
    OcrCompleted(OcrResult),        // merged_text 为清洗后的文本
    TranslationStarted,
    TranslationCompleted(TranslationResult),
    Error(PipelineError),           // 实时模式中 OCR/翻译的非致命错误
    Stopped,
}

pub struct Pipeline { /* ... */ }
impl Pipeline {
    pub fn new(config: PipelineConfig, deps: PipelineDeps) -> Self;
    pub async fn run(&self, event_tx: mpsc::Sender<PipelineEvent>) -> Result<(), PipelineError>;
    pub async fn stop(&self) -> Result<(), PipelineError>;
    pub fn status(&self) -> PipelineStatus;
    pub async fn update_region(&self, region: ScreenRegion) -> Result<(), CoreError>;
}

pub enum PipelineError {
    Capture(#[from] CaptureError),
    Ocr(#[from] OcrError),
    Translation(#[from] TranslationError),
    ChannelClosed,
    AlreadyRunning,
    NotRunning,
    Cancelled,
}

pub async fn run_single_capture(
    deps: PipelineDeps,
    config: PipelineConfig,
    event_tx: mpsc::Sender<PipelineEvent>,
) -> Result<(), PipelineError>;

// 可复用的去重/取消组件
pub use cancel::TaskSlot;            // 最多一个任务在飞，新任务到达时取消并等待旧任务
pub use dedup::{FrameDiffer, TextDedup, DEFAULT_DIFFERENCE_THRESHOLD};
```

### 多框实时翻译（MultiBoxPipeline）

```rust
pub struct TranslationBox { pub id: u32, pub region: ScreenRegion, pub color: String }
pub struct MultiBoxConfig {
    pub capture_interval_ms: u32,
    pub difference_threshold: f32,
    pub ocr_options: OcrOptions,
    pub translation_request: TranslationRequest,
    pub max_boxes: u32,                  // 默认 8
}
pub struct BoxedTranslationResult {
    pub box_id: u32, pub color: String, pub result: TranslationResult, pub timestamp: u64,
}
pub enum BoxStatus { Running, Stopped, Error(String) }

impl MultiBoxPipeline {
    pub fn new(config: MultiBoxConfig, deps: PipelineDeps) -> Self;
    pub async fn add_box(&self, box_: TranslationBox) -> Result<(), PipelineError>;
    pub async fn remove_box(&self, box_id: u32) -> Result<(), PipelineError>;
    pub async fn update_box(&self, box_id: u32, region: ScreenRegion) -> Result<(), PipelineError>;
    pub async fn start_all(&self) -> Result<(), PipelineError>;
    pub async fn stop_all(&self) -> Result<(), PipelineError>;
    pub async fn stop_box(&self, box_id: u32) -> Result<(), PipelineError>;
    pub fn subscribe_results(&self) -> mpsc::Receiver<BoxedTranslationResult>;
    pub fn box_count(&self) -> usize;
    pub fn box_status(&self, box_id: u32) -> Option<BoxStatus>;
}
```

每个翻译框作为独立 Tokio task 运行，拥有独立的 `CaptureSession`、`FrameDiffer`、
`BoxFingerprintCache` 和 `CancellationToken`。结果通过 `broadcast` channel（容量 =
`max_boxes * 2`）汇集，`subscribe_results` 返回一个由 forwarder 驱动的私有
`mpsc::Receiver`，提供 per-subscriber 背压。单框错误（如采集失败）只设置该框的
`BoxStatus::Error`，不影响其他框。`Drop` 时自动取消并清理所有 task。

新增错误变体：`BoxNotFound(u32)`、`BoxLimitExceeded(u32)`、`DuplicateBoxId(u32)`、
`InvalidConfig(String)`。

### 单次模式链路

```text
capture_once(region)
-> OcrProvider::recognize(image, 图像对齐 region, options, cancel)
-> TextNormalizer::merge_lines + clean（日文源时 normalize_punctuation）
-> 空文本直接结束；否则 translate_text（超长自动切段）
-> 依次发送 CaptureStarted / OcrStarted / OcrCompleted / TranslationStarted / TranslationCompleted / Stopped
```

### 实时模式链路

```text
capture session（区域变更时自动重建会话，不中断流水线）
-> FrameDiffer 帧差检测（未变化直接跳过，不发 OCR）
-> mpsc channel(cap=1)（满载丢弃最新帧，队列内存有界）
-> OCR worker：TaskSlot 保证最多 1 个 OCR 任务；新帧到达时取消旧任务
-> 指纹去重（TextDedup）：文本未变化时跳过翻译
-> mpsc channel(cap=1)
-> Translation worker：最多 1 个翻译任务；新任务到达时取消旧任务
-> 发送事件；stop() 通过共享 CancellationToken 在短时间内终止所有 worker
```

## 4. 行为契约

- 生命周期：同一 `Pipeline` 同时只允许一个 `run`（重复调用返回 `AlreadyRunning`）；`run` 返回后
  可以再次运行。`stop()` 在无运行会话时返回 `NotRunning`。
- 错误语义：单次模式的失败通过 `run` 的返回值上报（状态变为 `Error`）；实时模式中 OCR/翻译失败
  发送 `PipelineEvent::Error` 并继续下一帧，采集失败则终止本次运行。取消（`Cancelled`）不是错误。
- 取消语义：`stop()` 取消共享 token；单次模式在 capture/OCR/translate 各阶段间检查 token；
  实时模式各 worker 与任务都在 token 上 select，停止后所有任务在合作式取消下快速退出。
- 区域更新：`update_region` 校验区域后写入配置；实时模式下 capture loop 通过 `Notify` 被唤醒并
  用新区域重建会话（OCR/翻译 worker 不重启，不中断流水线）。
- 日志红线：不记录原文/译文全文、截图数据；引用文本一律 `truncate_for_log`（前 20 字符 + `...`）。

## 5. 已知限制

| 类型 | 限制 | 缓解方式 |
|------|------|----------|
| 设计使然 | 帧通道满载时丢弃「最新」帧（mpsc cap=1 语义） | 配合 OCR worker 的任务取消，实际效果近似「保留最新帧」；内存始终有界 |
| 设计使然 | 每次翻译只调用 `TranslationProvider::translate`，超长文本由流水线先切段、逐段翻译后合并 | 段落数有限（≤2000 字符/段），段间顺序保持 |
| 待后续 Phase | 实时模式不提供错误恢复/自动重连采集会话 | 采集失败时终止运行并由应用层提示 |
| 待后续 Phase | `PipelineConfig` 不做字段级校验（如 interval 过小） | 运行时钳制并 `warn!` 日志 |
| 设计使然 | `update_region` 在单次模式仅影响下一次运行 | 文档与示例已注明 |

## 6. 构建与测试

```powershell
cargo build -p vtrans-pipeline
cargo test -p vtrans-pipeline
cargo clippy -p vtrans-pipeline --all-targets
cargo fmt --all -- --check
```

集成测试使用 `tests/common/mod.rs` 中的脚本化 mock Provider（可编程返回值、延迟、阻塞、取消计数与
并发峰值），覆盖：单次完整链路、错误上报、取消、帧差跳过、指纹去重、旧任务取消、并发上限、
停止清理、区域更新、背压有界、会话自然结束。

`tests/pipeline_multibox.rs` 使用无状态 mock Provider（`EchoOcrProvider`、`EchoTranslationProvider`、
`GeneratingCaptureSource`）覆盖多框场景：2+ 框并发独立运行、运行时增删框、区域修改重启、停止单框、
错误隔离（一框采集失败不影响其他框）、指纹去重隔离（框间不交叉）、通道容量背压、8 框并发无 panic/
死锁基准、以及各类边界错误（重复 ID、超限、不存在、重复启停）。

## 7. 人工验证（全管线：采集屏幕 -> OCR -> 翻译 -> 输出）

`examples/pipeline_verify.rs` 是完整管线验证 CLI：用真实 `WindowsCaptureSource` 采集屏幕、
`PaddleOcrProvider` 识别、`ApiTranslationProvider` / `LocalTranslationProvider` 翻译，并把
识别文本与译文打印到终端。

前置条件：

1. 运行 `scripts/download_models.ps1` 下载 OCR 模型到 `src-tauri/resources/models`，
   目录下需要有 `manifest.json` 与对应 `.onnx` / 字典文件；
2. 需要交互式 Windows 桌面会话（远程会话 / 无桌面可能初始化失败）；
3. 屏幕目标区域里先放好要翻译的内容（如打开一个日文网页）。

> 本地翻译模型说明：本项目选定本地翻译模型采用「整图生成」ONNX 接口（速度优先），
> `vtrans-translation` 已实现该接口的推理路径，`pipeline_verify` 本地模式已验证可用
> （采集 -> OCR -> 本地整图生成翻译 -> 输出译文）。当前 manifest 声明的 `opus-mt-en-zh`
> 仅支持 `en -> zh-CN`；其他语言对请使用 **API 翻译** 路径。

单次截屏翻译（本地整图生成模型，已验证可跑通全管线）：

```powershell
cargo run -p vtrans-pipeline --example pipeline_verify -- `
  --models src-tauri/resources/models `
  --language en --target zh-CN --mode single `
  --region 100,100,800,400
```

实时翻译（Ctrl+C 停止；`--region` 不传时默认主显示器居中 800x600）：

```powershell
cargo run -p vtrans-pipeline --example pipeline_verify -- `
  --models src-tauri/resources/models `
  --language en --target zh-CN --mode live `
  --interval-ms 500 --threshold 0.02
```

其他语言对（如 `ja -> zh-CN`）走 API 翻译（OCR 仍需模型目录）：

```powershell
cargo run -p vtrans-pipeline --example pipeline_verify -- `
  --models src-tauri/resources/models `
  --api-endpoint https://api.example.com/v1/chat/completions `
  --api-model translator --api-key $env:VTRANS_API_KEY `
  --language ja --target zh-CN --mode single `
  --region 100,100,800,400
```

预期输出：`[capture]` / `[ocr]` / `--- recognized text ---` / `[translate]` / `--- translated text ---`
分阶段打印；单次模式结束时打印 `[pipeline] single capture completed`，实时模式按 Ctrl+C 后打印
`[pipeline] stopped`。若 OCR 无文本，会跳过翻译（符合指纹去重与空文本语义）。

## 8. 已知限制 / 集成说明

- 本地翻译模型采用「整图生成」ONNX 接口（输入 `input_ids/attention_mask/num_beams/
  min_length/max_length/length_penalty/repetition_penalty`，输出 `sequences`），
  `vtrans-translation` 已实现该接口（同时兼容逐 token 解码接口）；beam search 由图内实现，
  manifest `inference_params.num_beams` 直接生效。
- 翻译调用策略（管线侧）：整段文本默认**单次调用**（≤2000 字符），只有超长文本才按字符边界
  切块——最大化减少整图生成推理次数；Provider 会按自身 `max_length` 截断超长输入。
- OCR 字符表由 `vtrans-ocr` 从 ONNX 模型内嵌 `character` 元数据构建（缺失时回退 manifest
  字典文件），无需在管线层关注字典配套。
- 本地模型语言对有限：`opus-mt-en-zh` 仅支持 `en -> zh-CN`；其他语言对请走 API 翻译。
- 示例 CLI 不读取 `vtrans-security` 凭据，API Key 通过命令行/环境变量注入（应用层才使用
  CredentialManager）。

## 详细规格

参见 `docs/modules/09-pipeline.md`。
