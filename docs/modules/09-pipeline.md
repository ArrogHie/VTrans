# 模块 09：vtrans-pipeline 流水线编排

| 属性 | 值 |
|------|-----|
| Crate | `vtrans-pipeline` |
| 分支 | `feat/09-pipeline` |
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
-> TextNormalizer::merge_lines + clean
-> TranslationProvider::translate(request, cancel)
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
│   └── cancel.rs        # 任务取消协调
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

## 验收标准

- [ ] 单次模式：capture -> OCR -> translate -> emit 完整链路
- [ ] 实时模式：帧差检测正确跳过
- [ ] 最多 1 个 OCR 和 1 个翻译任务同时运行
- [ ] 新任务到达时取消旧任务
- [ ] 文本指纹不变时不重复翻译
- [ ] stop 能在短时间内终止所有 worker
- [ ] 无限队列和内存增长
- [ ] README.md 完整

## 开发注意事项

- 使用 tokio::sync::mpsc channel(cap=1) 连接 capture -> OCR -> translation
- CancellationToken 每次新任务创建新的，cancel 旧的
- 帧差检测：计算像素差异比例，超过 threshold 才触发 OCR
- 空闲时 sleep(interval_ms)，不忙等待
- Pipeline 通过依赖注入接收 trait 对象，不直接依赖具体实现
- 日志记录每步耗时和事件发送
