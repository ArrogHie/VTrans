# vtrans-pipeline

流水线编排模块：把屏幕采集、OCR、文本标准化与翻译串成单次截屏和实时区域翻译两条链路，并统一处理帧差检测、任务取消、文本去重与背压。

## 1. 模块概述

`vtrans-pipeline` 是 VTrans 的第 3 层（编排层）：它不实现任何具体能力，只通过 `vtrans-core` 的
`CaptureSource` / `OcrProvider` / `TranslationProvider` trait 编排下游模块，并在翻译前解析
实际源语言（OCR 检测 → Unicode 启发式）、按语言预算做标点感知分块。

边界：

- 做：单次截屏翻译、固定区域实时翻译、auto 源语言路由、标点感知分块、帧差检测、有界通道、
  旧任务取消、文本指纹去重、停止清理。
- 不做：屏幕采集（`vtrans-capture`）、OCR 推理（`vtrans-ocr`）、文本清洗/切段（`vtrans-text`）、翻译请求（`vtrans-translation`）。
- 不直接依赖具体 Provider 实现：全部通过 trait 依赖注入（`PipelineDeps`）。
- 不跨 IPC 传输图像：`CapturedImage` 留在 Rust 侧，事件通道只传文本、状态与耗时。

## 2. 依赖关系

| 类型 | Crate | 用途 |
|------|-------|------|
| 上游 | `vtrans-core` | `CaptureSource` / `OcrProvider` / `TranslationProvider` trait、`CapturedImage` / `OcrResult` / `TranslationRequest` 等类型、`CaptureError` / `OcrError` / `TranslationError`、`truncate_for_log` |
| 上游 | `vtrans-text` | `TextNormalizer`（clean / merge_lines / fingerprint）、`japanese::normalize_punctuation` |
| 上游 | `vtrans-capture` / `vtrans-ocr` / `vtrans-translation` | 依赖声明（模块规格约定）；代码只引用 `vtrans-core` 中对应 trait |
| 外部 | `tokio` | 异步任务、`mpsc` 有界通道、`Notify`、超时与 sleep |
| 外部 | `tokio-util` | `CancellationToken` 协作取消 |
| 外部 | `thiserror` | `PipelineError` 错误枚举派生 |
| 外部 | `tracing` | 结构化日志（入口 `#[instrument]`、错误路径 `warn!`/`error!`） |
| dev | `async-trait` | 集成测试中 mock provider 的 trait 实现 |

无新增外部依赖：取消协调（`cancel::TaskSlot`）、帧差检测（`dedup::FrameDiffer`）、文本去重
（`dedup::TextDedup`）均为本 crate 实现，避免引入第三方依赖。

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

// Auto 源语言路由（纯函数，可单测）
pub fn resolve_translation_source(detected: Option<Language>, configured: Language) -> Language;
pub fn heuristic_detect_language(text: &str) -> Option<Language>;

// 分块预算常量（字符级；单测锁定）
pub const MAX_TRANSLATION_CHUNK_CHARS: usize = 2000; // 无专项预算语言（zh / 未解析 Auto）的上限
pub const JA_CHUNK_CHARS: usize = 512;               // 日文单块预算（对齐 max_input_tokens=256）
pub const EN_CHUNK_CHARS: usize = 1024;              // 英文单块预算

// 可复用的去重/取消组件
pub use cancel::TaskSlot;            // 最多一个任务在飞，新任务到达时取消并等待旧任务
pub use dedup::{FrameDiffer, TextDedup, DEFAULT_DIFFERENCE_THRESHOLD};
```

### 单次模式链路

```text
capture_once(region)
-> OcrProvider::recognize(image, 图像对齐 region, options, cancel)
-> 解析实际源语言：配置语言 -> OCR detected_language -> Unicode 启发式 -> Auto
-> TextNormalizer::merge_lines + clean（解析后源为日文时 normalize_punctuation）
-> 空文本直接结束；否则 translate_text（按源语言预算做标点感知切段）
-> 依次发送 CaptureStarted / OcrStarted / OcrCompleted / TranslationStarted / TranslationCompleted / Stopped
```

### 实时模式链路

```text
capture session（区域变更时自动重建会话，不中断流水线）
-> FrameDiffer 帧差检测（未变化直接跳过，不发 OCR）
-> mpsc channel(cap=1)（满载丢弃最新帧，队列内存有界）
-> OCR worker：TaskSlot 保证最多 1 个 OCR 任务；新帧到达时取消旧任务
-> 指纹去重（TextDedup）：文本未变化时跳过翻译
-> 每帧解析实际源语言（同单次模式顺序）
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
- 源语言路由：`Auto` 的解析顺序为「配置的具体语言 → OCR `detected_language`（仅 en/ja/zh-CN）→
  Unicode 启发式（假名 → 日文；拉丁字母为主 → 英文）→ 保持 `Auto`」。解析结果以 `debug!` 记录
  （如 `configured=auto resolved=ja`），解析后的源语言同时用于日文标点归一化与翻译请求。
- 分块：文本不超过源语言预算时单次调用（保留换行）；超长时先按换行分段落，段内优先在句子边界
  （`。！？.!?`）切分，其次逗号/分号（`，、,;`），再其次空白，最后硬切（绝不拆开 Unicode 标量）。
  日文预算 512 字符、英文 1024 字符，其他语言保持 2000 字符上限；块间顺序保持，译文以 `\n` 合并。

## 5. 已知限制

| 类型 | 限制 | 缓解方式 |
|------|------|----------|
| 设计使然 | 帧通道满载时丢弃「最新」帧（mpsc cap=1 语义） | 配合 OCR worker 的任务取消，实际效果近似「保留最新帧」；内存始终有界 |
| 设计使然 | 每次翻译只调用 `TranslationProvider::translate`，超长文本由流水线先切段、逐段翻译后合并 | 块间顺序保持，译文以 `\n` 合并；块大小受源语言预算约束 |
| 设计使然 | Unicode 启发式不是语言识别器：纯汉字短文本无法区分中/日文，可能保持 `Auto` 并被 Provider 拒绝 | 引导用户在 OCR 语言/源语言上显式选择（游戏/漫画场景推荐） |
| 待优化 | 分块预算是字符级近似，不精确对应 token 数 | Provider 侧 `max_input_tokens=256` 截断兜底，不做 tokenizer 依赖 |
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

## 7. 人工验证（全管线：采集屏幕 -> OCR -> 翻译 -> 输出）

`examples/pipeline_verify.rs` 是完整管线验证 CLI：用真实 `WindowsCaptureSource` 采集屏幕、
`PaddleOcrProvider` 识别、`ApiTranslationProvider` / `NativeTranslationProvider` 翻译，并把
识别文本与译文打印到终端。

前置条件：

1. 运行 `scripts/download_models.ps1` 下载 OCR 模型到 `src-tauri/resources/models`，
   目录下需要有 `manifest.json` 与对应 `.onnx` / 字典文件；
2. 本地翻译路径需要 `native/translation_bridge` 构建产物（`translation_bridge.dll`，运行
   `native/translation_bridge/build.ps1`）与 `scripts/translation/setup_translation_models.ps1`
   部署的 manifest v2 模型文件；未构建时本地模式会以 `ModelLoad` 失败并提示改用 API；
3. 需要交互式 Windows 桌面会话（远程会话 / 无桌面可能初始化失败）；
4. 屏幕目标区域里先放好要翻译的内容（如打开一个日文网页）。

> 本地翻译模型说明：本地翻译走 `vtrans-translation` 的双引擎原生路径（`Bergamot` en→zh-CN +
> `CTranslate2` ja→zh-CN，provider id `local-native`），仅支持 `en -> zh-CN` 与 `ja -> zh-CN`；
> 其他语言对请使用 **API 翻译** 路径。

单次截屏翻译（本地双引擎模型）：

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

其他语言对或未构建本地桥时走 API 翻译（OCR 仍需模型目录）：

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
`--language auto` 时流水线会在 OCR 后解析源语言（检测 → 启发式），本地模式无法判定时由
Provider 返回 `UnsupportedPair` 错误事件。

## 8. 已知限制 / 集成说明

- 本地翻译采用 `vtrans-translation` 的双引擎原生 Provider（`Bergamot` / `CTranslate2`，
  provider id `local-native`），只接受具体源语言；`Auto` 由本模块在 OCR 后解析。
- 翻译调用策略（管线侧）：整段文本默认**单次调用**（日文 ≤512、英文 ≤1024、其他 ≤2000
  字符），超长文本按换行 + 句子/逗号/空白边界切块——最大化减少 Provider 调用次数，同时保持
  句子完整；Provider 会按自身 `max_input_tokens` 截断超长输入。
- OCR 字符表由 `vtrans-ocr` 从 ONNX 模型内嵌 `character` 元数据构建（缺失时回退 manifest
  字典文件），无需在管线层关注字典配套。
- 本地模型语言对有限：仅 `en -> zh-CN` / `ja -> zh-CN`；其他语言对请走 API 翻译。
- 示例 CLI 不读取 `vtrans-security` 凭据，API Key 通过命令行/环境变量注入（应用层才使用
  CredentialManager）。

## 详细规格

参见 `docs/modules/09-pipeline.md`。
