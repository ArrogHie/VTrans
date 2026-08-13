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

编排屏幕采集、OCR、文本标准化和翻译的完整流水线。支持单次截屏翻译、固定区域实时翻译，以及多框实时翻译（每框独立采集-OCR-翻译任务、独立取消、结果统一广播）。实现帧差检测、有界通道、任务取消和文本去重。

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

### 多框实时翻译（`src/multibox.rs`，crate 根 re-export）

```rust
/// 单个翻译框（Serialize/Deserialize，经 IPC 传给前端）
pub struct TranslationBox {
    pub id: u32,             // 唯一 id（对应 TranslationBoxConfig.id）
    pub region: ScreenRegion, // 该框捕获/翻译的屏幕区域
    pub color: String,       // hex 颜色（如 "#FF6B6B"），前端据此区分框
}
impl TranslationBox { pub fn new(id, region, color) -> Self; }

/// 多框流水线共享配置（每框的 region/color 在 TranslationBox 上）
pub struct MultiBoxConfig {
    pub capture_interval_ms: u32,    // 每框捕获间隔（<16ms 被钳制）
    pub difference_threshold: f32,   // 触发 OCR 的像素差异比例 0.0..=1.0
    pub ocr_options: OcrOptions,     // 所有框共享
    pub translation_request: TranslationRequest, // 模板；OCR 文本替换 text
    pub max_boxes: u32,              // 并发框上限，默认 8
}
impl MultiBoxConfig {
    pub fn new(interval, threshold, ocr, request) -> Self;      // max_boxes=8
    pub fn with_max_boxes(interval, threshold, ocr, request, max) -> Self;
}
impl Default for MultiBoxConfig;  // 250ms / 默认阈值 / OcrOptions::default / Auto→ChineseSimplified / 8

/// 带框标签的翻译结果（Serialize/Deserialize，经 IPC 传给前端）
pub struct BoxedTranslationResult {
    pub box_id: u32,
    pub color: String,
    pub result: TranslationResult,  // translated_text / provider_id / elapsed_ms
    pub original_text: String,      // 已清洗的 OCR 原文（发送给翻译 provider 的同一份文本）
    pub timestamp: u64,             // Unix 毫秒
}
impl BoxedTranslationResult {
    pub fn new(box_id, color, result) -> Self;              // original_text 为空
    pub fn with_original_text(self, text) -> Self;          // 配对 OCR 原文
}

/// 单框运行时状态（Serialize/Deserialize；单元变体序列化为字符串）
pub enum BoxStatus {
    Running,
    Stopped,
    Error(String),
}

/// 多框流水线编排器（Send + Sync；框注册表由 RwLock 保护）
pub struct MultiBoxPipeline { /* ... */ }

impl MultiBoxPipeline {
    pub fn new(config: MultiBoxConfig, deps: PipelineDeps) -> Self;
    pub fn max_boxes(&self) -> u32;
    pub fn box_count(&self) -> usize;                     // 已注册框数（含停用）
    pub fn box_status(&self, box_id: u32) -> Option<BoxStatus>;

    /// 注册框；流水线运行中则立即启动其任务
    pub async fn add_box(&self, box_: TranslationBox) -> Result<(), PipelineError>;
    /// 先停任务再移除框（清理去重与状态）
    pub async fn remove_box(&self, box_id: u32) -> Result<(), PipelineError>;
    /// 更新框区域；运行中则停旧任务并以新区域重启
    pub async fn update_box(&self, box_id: u32, region: ScreenRegion) -> Result<(), PipelineError>;
    /// 启动全部未运行框的任务
    pub async fn start_all(&self) -> Result<(), PipelineError>;
    /// 停止全部框任务（框保持注册，可再次 start_all）
    pub async fn stop_all(&self) -> Result<(), PipelineError>;
    /// 停止单框任务（框保持注册）
    pub async fn stop_box(&self, box_id: u32) -> Result<(), PipelineError>;
    /// 订阅结果流：返回私有 mpsc::Receiver<BoxedTranslationResult>
    pub fn subscribe_results(&self) -> mpsc::Receiver<BoxedTranslationResult>;
}
```

`BoxedTranslationResult.original_text` 语义（F1/F2 落地）：

- **成功路径**：携带清洗后的 OCR 文本（与发送给翻译 provider 的文本同源，
  `normalize_result` 产物），供弹窗同时展示原文+译文；
- **降级路径**：OCR 产出空文本（跳过 provider 调用）或翻译失败时，仍发布
  一条 `translated_text` 与 `original_text` 均为**空串**的结果——让 overlay
  清除过期内容而非残留旧译文；取消（stop/被新任务取代/provider 取消）不是
  失败，不发布任何结果。

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
    // ── 多框变体 ──
    #[error("box not found: {0}")]
    BoxNotFound(u32),
    #[error("box limit exceeded: max {0}")]
    BoxLimitExceeded(u32),
    #[error("duplicate box id: {0}")]
    DuplicateBoxId(u32),
    #[error("invalid config: {0}")]
    InvalidConfig(String),
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

### 多框实时模式（每框一条独立流水线）

```text
MultiBoxPipeline
├─ box 0 ── tokio task（独立 CancellationToken #0）
│    ├─ capture loop：自有 CaptureSession + FrameDiffer（阈值触发）
│    │      -> frames channel (cap=1，满则丢帧背压)
│    ├─ OCR worker：TaskSlot 保证同框最多 1 次 OCR
│    │      -> normalize -> BoxFingerprintCache.is_duplicate(box_id)
│    │      -> jobs channel (cap=1，满则丢旧任务)
│    └─ translation worker：TaskSlot 保证同框最多 1 次翻译
│           -> 成功：BoxedTranslationResult{ original_text = OCR 清洗文本 }
│           -> OCR 空文本 / 翻译失败：发布空译文 + 空 original_text（清除 overlay）
│           -> 取消：不发布
├─ box 1 ── tokio task（独立 CancellationToken #1）……同上
└─ 结果广播：broadcast::Sender（容量 max_boxes*2，至少 1）
      -> 每订阅者经 forwarder task 转私有 mpsc（按订阅者背压，慢订阅 lag 丢旧结果并 warn）
```

关键模型：

- **每框独立任务**：`add_box` / `start_all` 为每框 spawn 一个 tokio task（内含
  上述 3 个子任务）；每框持独立 `CancellationToken`，`stop_box` /
  `remove_box` / `stop_all` 在有限时间内终止该框，不影响其它框。
- **帧差 + 指纹双去重**：帧差（`FrameDiffer`，按框独立实例）过滤无变化帧；
  OCR 清洗后经 `BoxFingerprintCache` 按 `box_id` 隔离去重（见模块 06）。
- **错误隔离**：某框捕获失败等错误只把该框 `BoxStatus` 置为
  `Error(String)`（发布状态），其它框继续运行；单框翻译失败发布空结果清
  除 overlay，不终止框任务。
- **有界内存**：每框两段 cap-1 通道 + 结果 broadcast（`max_boxes*2`）。
- **运行时增删改**：`update_box` 停旧任务、换新区域重启（重启前
  `clear_box` 重置该框去重）；`remove_box` 停任务、清去重与状态；运行中
  `add_box` 立即启动。

## 内部文件结构

```text
crates/vtrans-pipeline/
├── Cargo.toml
├── README.md
├── src/
│   ├── lib.rs          # re-export, Pipeline
│   ├── live.rs          # 实时模式实现
│   ├── single.rs        # 单次模式实现
│   ├── multibox.rs      # 多框实时翻译（MultiBoxPipeline / TranslationBox / BoxedTranslationResult / BoxStatus）
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
| 多框增删改 | 集成 | add_box 校验区域/上限/重复 id；remove_box 停任务并清理；update_box 停旧启新 |
| 多框启停 | 集成 | start_all 只启动未运行框；重复 start/stop 返回 AlreadyRunning/NotRunning；stop_box 单停 |
| 多框结果 | 集成 | 结果含 box_id/color/original_text/timestamp；空 OCR 文本与翻译失败发布空结果；取消不发布 |
| 框状态与隔离 | 集成 | 单框错误置 Error(String) 不影响其它框；box_status/box_count 正确 |
| 广播与背压 | 单元/集成 | 订阅者私有 mpsc；慢订阅 lag 丢旧结果；容量 = max_boxes*2（至少 1） |

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
- 多框：`MultiBoxPipeline` 为 `Send + Sync`；框注册表用 `std::sync::RwLock`
  且仅在快速 map 操作期间持锁，锁在任何 `.await` 前释放；结果走
  `tokio::sync::broadcast`（容量 `max_boxes*2`），每订阅者经 forwarder task
  转私有 `mpsc` 实现按订阅者背压
- 多框：OCR/翻译各用 `TaskSlot` 保证同框同一时刻至多一个进行中任务；
  `capture_interval_ms` 钳制到 ≥16ms、`difference_threshold` 钳制到合法区间
  （复用 `live.rs` 的 clamp 函数）
- 多框：`original_text` 必须与发送给 provider 的清洗文本配对（
  `with_original_text`）；降级结果（空 OCR 文本 / 翻译失败）发布空译文 +
  空原文以清除 overlay，取消不发布

## 增量记录

### 多框实时翻译增量（分支 `feat/multibox-pipeline`）

对应功能计划 `docs/features/multi-box-realtime/PLAN.md`。

- 新增 `src/multibox.rs`：`TranslationBox` / `MultiBoxConfig` /
  `BoxedTranslationResult` / `BoxStatus` / `MultiBoxPipeline`，crate 根
  re-export；`PipelineError` 新增 `BoxNotFound` / `BoxLimitExceeded` /
  `DuplicateBoxId` / `InvalidConfig` 四变体。
- `MultiBoxPipeline` 公开 API：`new` / `max_boxes` / `box_count` /
  `box_status` / `add_box` / `remove_box` / `update_box` / `start_all` /
  `stop_all` / `stop_box` / `subscribe_results`。
- 每框独立 tokio task（capture loop + OCR worker + translation worker）与
  独立 `CancellationToken`；帧差（`FrameDiffer`）+ per-box 指纹去重
  （`BoxFingerprintCache`）；错误隔离（单框错误置 `Error(String)`）。
- 结果经 `broadcast` 通道（容量 `max_boxes*2`）统一分发，订阅者私有
  `mpsc` + forwarder task 提供按订阅者背压。

### F1 增量：多框结果携带原文（分支 `feat/multibox-original-text`）

- `BoxedTranslationResult` 新增 `original_text: String`，经
  `with_original_text` 与翻译结果配对（清洗后的 OCR 文本）。
- 降级语义：OCR 空文本（跳过 provider 调用）与翻译失败均发布空译文 +
  空 `original_text` 的结果，清除 overlay 残留；取消不发布。
- 向后兼容：`new` 构造 `original_text` 为空串；serde 往返含新字段。
