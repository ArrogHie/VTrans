# 模块 09：vtrans-pipeline 流水线编排

| 属性 | 值 |
|------|-----|
| Crate | `vtrans-pipeline` |
| 分支 | `feat/09-new-translate-model`（翻译模型升级增量） |
| 上游依赖 | `vtrans-core`, `vtrans-capture`, `vtrans-ocr`, `vtrans-text`, `vtrans-translation` |
| 层级 | 3 |
| 复杂度 | 高 |
| 阶段 | Phase 3 |

## 职责

编排屏幕采集、OCR、文本标准化和翻译的完整流水线。支持单次截屏翻译和固定区域实时翻译。实现帧差检测、有界通道、任务取消和文本去重。

## 公开 API

```rust
pub struct PipelineConfig {
    pub mode: PipelineMode,
    pub region: ScreenRegion,
    pub capture_interval_ms: u32,
    pub difference_threshold: f32,
    pub ocr_options: OcrOptions,
    pub translation_request: TranslationRequest,
}

pub enum PipelineEvent {
    CaptureStarted,
    OcrStarted,
    OcrCompleted(OcrResult),
    TranslationStarted,
    TranslationCompleted(TranslationResult),
    Error(PipelineError),
    Stopped,
}

/// 流水线依赖注入参数
pub struct PipelineDeps {
    pub capture: Box<dyn CaptureSource>,
    pub ocr: Box<dyn OcrProvider>,
    pub translation: Box<dyn TranslationProvider>,
}

pub struct Pipeline { /* ... */ }

impl Pipeline {
    pub fn new(config: PipelineConfig, deps: PipelineDeps) -> Self;
    pub async fn run(&self, event_tx: mpsc::Sender<PipelineEvent>);
    pub async fn stop(&self);
    pub fn status(&self) -> PipelineStatus;
    pub async fn update_region(&self, region: ScreenRegion);
}
```

### 源语言路由（翻译模型升级增量）

```rust
/// 解析实际翻译源语言：配置为具体语言时原样返回；配置为 Auto 时优先用
/// OCR detected_language（仅 en/ja/zh-CN）；无检测结果时返回 Auto。
/// （文本的 Unicode heuristic 兜底由流水线在翻译前组合
/// `heuristic_detect_language` 完成，见「流水线流程」。）
pub fn resolve_translation_source(detected: Option<Language>, configured: Language) -> Language;

/// Unicode heuristic：存在平假名/片假名/半角片假名 -> Japanese；
/// 否则以拉丁字母为主 -> English；其余 None。
pub fn heuristic_detect_language(text: &str) -> Option<Language>;

pub const MAX_TRANSLATION_CHUNK_CHARS: usize = 2000; // 无专项预算语言（zh / 未解析 Auto）的上限
pub const JA_CHUNK_CHARS: usize = 512;               // 日文单块预算（对齐 max_input_tokens=256）
pub const EN_CHUNK_CHARS: usize = 1024;              // 英文单块预算
```

`Auto` 源语言的解析顺序（OCR 完成后、normalize 与翻译之前）：

```text
配置的具体语言（en / ja / zh-CN）直接使用
  -> OCR detected_language（仅 en / ja / zh-CN）
  -> Unicode heuristic（假名 -> ja；拉丁字母为主 -> en）
  -> 保持 Auto（Provider 返回 UnsupportedPair，UI 提示用户显式选择）
```

解析后的源语言同时用于 `normalize_result` 的日文标点归一化与翻译请求；
解析结果以 `debug!` 记录（如 `configured=auto resolved=ja`）。

### 分块规则（翻译模型升级增量）

文本不超过源语言预算时单次调用（保留换行）；超长时先按换行分段落，段内依次优先：

1. 句子边界：`。！？`（日文）/ `.!?`（英文），并集 `。！？.!?`；
2. 逗号/分号：`，、,;`；
3. 空白；
4. 硬切（绝不拆开 Unicode 标量）。

预算：日文 `JA_CHUNK_CHARS = 512`、英文 `EN_CHUNK_CHARS = 1024`（对齐新模型
`max_input_tokens=256` 的保守字符/token 估算），其余语言保持
`MAX_TRANSLATION_CHUNK_CHARS = 2000`。块间顺序保持，译文以 `\n` 合并。

## 错误类型

> **定义位置**：`PipelineError` 定义在本 crate（`vtrans-pipeline`）中，不在 vtrans-core。通过 `#[from]` 从 `CaptureError`、`OcrError`、`TranslationError`（均在 vtrans-core 中定义）自动转换。

```rust
[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error("capture error: {0}")]
    Capture(#[from] CaptureError),
    #[error("ocr error: {0}")]
    Ocr(#[from] OcrError),
    #[error("translation error: {0}")]
    Translation(#[from] TranslationError),
    #[error("channel closed")]
    ChannelClosed,
    #[error("session already running")]
    AlreadyRunning,
    #[error("session not running")]
    NotRunning,
    #[error("cancelled")]
    Cancelled,
}
```

## 流水线流程

### 单次模式

```text
capture_once(region)
-> OcrProvider::recognize(image, region, options, cancel)
-> 解析实际源语言（配置 -> OCR 检测 -> Unicode heuristic -> Auto）
-> TextNormalizer::merge_lines + clean（解析后源为日文时 normalize_punctuation）
-> TranslationProvider::translate(request, cancel)（请求使用解析后的源语言）
-> emit PipelineEvent::TranslationCompleted
```

### 实时模式

```text
loop {
  start_session(region)
  next_frame -> crop
  frame_difference_check (skip if unchanged)
  -> bounded channel(cap=1, only latest frame)
  -> OCR worker (cancel previous if running)
  -> 每帧解析实际源语言（同单次模式顺序）
  -> text fingerprint check (skip if same)
  -> Translation worker (cancel previous if running)
  -> emit events
  sleep(capture_interval_ms)
}
```

## 内部文件结构

```text
crates/vtrans-pipeline/
├── Cargo.toml
├── README.md
├── src/
│   ├── lib.rs          # re-export, Pipeline
│   ├── live.rs          # 实时模式实现
│   ├── single.rs        # 单次模式实现
│   ├── dedup.rs         # 帧差检测 + 文本指纹去重
│   ├── cancel.rs        # 任务取消协调
│   └── language.rs      # auto 源语言路由 + Unicode heuristic（纯函数）
└── tests/
```

## 测试计划

| 测试项 | 类型 | 说明 |
|--------|------|------|
| 单次流水线 | 集成 | mock capture/ocr/translation, 事件正确发送 |
| 帧差检测 | 单元 | 相同帧返回 false，不同帧返回 true |
| 文本指纹去重 | 单元 | 相同文本跳过翻译 |
| 通道容量 | 单元 | cap=1 只保留最新帧 |
| 旧任务取消 | 集成 | 新帧到达时旧 OCR/翻译被取消 |
| 停止清理 | 集成 | stop 后所有 worker 退出 |
| 区域更新 | 集成 | update_region 不中断会话 |
| 空闲 CPU | 集成 | 无变化时不触发 OCR/翻译 |
| 源语言路由（具体语言直通） | 单元 | resolve_translation_source：配置具体语言时忽略检测结果 |
| 源语言路由（Auto + 检测） | 单元 | Auto + Some(en/ja/zh-CN) 各分支 |
| 源语言路由（Auto + 启发式） | 单元 | Auto + None -> heuristic（假名/拉丁/混合/空文本）；无法判定 -> Auto |
| 分块预算 | 单元 | ja 512 / en 1024 / 其余 2000；常量单测锁定 |
| 分块边界 | 单元 | 句子优先、逗号兜底、空白兜底、硬切兜底、Unicode 标量不拆分、短文本单块 |
| auto 路由集成 | 集成 | 单次/实时模式下翻译请求使用解析后的源语言 |

## 验收标准

- [ ] 单次模式：capture -> OCR -> translate -> emit 完整链路
- [ ] auto 源语言路由：配置语言 -> OCR 检测 -> Unicode heuristic -> Auto，解析结果用于
      翻译请求与日文标点归一化
- [ ] 分块规则升级：句子/逗号/空白优先、硬切兜底；ja 512 / en 1024 / 其余 2000 字符预算
- [ ] 实时模式：帧差检测正确跳过
- [ ] 最多 1 个 OCR 和 1 个翻译任务同时运行
- [ ] 新任务到达时取消旧任务
- [ ] 文本指纹不变时不重复翻译
- [ ] stop 能在短时间内终止所有 worker
- [ ] 无限队列和内存增长
- [ ] README.md 完整
- [ ] examples/pipeline_verify.rs 适配 07（NativeTranslationProvider + supported_pairs trait 方法）

## 开发注意事项

- 使用 tokio::sync::mpsc channel(cap=1) 连接 capture -> OCR -> translation
- CancellationToken 每次新任务创建新的，cancel 旧的
- 帧差检测：计算像素差异比例，超过 threshold 才触发 OCR
- 空闲时 sleep(interval_ms)，不忙等待
- Pipeline 通过依赖注入接收 trait 对象，不直接依赖具体实现
- 日志记录每步耗时和事件发送
- 不引入 tokenizer 依赖：分块是字符/标点级近似，token 精确性由 Provider 侧
  `max_input_tokens` 截断兜底
- `zh-CN` 检测结果保持原样：本地 Provider 不支持中文源，由 Provider 返回
  `UnsupportedPair`（UI 已提示）
