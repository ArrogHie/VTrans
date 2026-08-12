## 模块开发说明：09-pipeline — 多框实时翻译增量（主任务）

### AGENT_DEV_PROMPT 参数
- MODULE_NUMBER: 09
- MODULE_NAME: vtrans-pipeline
- MODULE_SLUG: pipeline
- CRATE_PATH: crates/vtrans-pipeline
- SCOPE: pipeline
- BRANCH_NAME: feat/multibox-pipeline

### 功能上下文
- 功能目标：在实时翻译模式下支持多框同时采集-OCR-翻译编排
- 本模块承担的部分：定义 TranslationBox 类型、多框 pipeline 管理、Tokio 多任务编排、per-box 去重/帧差/取消
- 上游已提供：
  - vtrans-core: ScreenRegion（不变）、CaptureSource/CaptureSession trait（Send+Sync）、OcrProvider、TranslationProvider、CancellationToken
  - vtrans-config: TranslationBoxConfig、MultiBoxConfig（阶段 A 完成后从 main 拉分支）
  - vtrans-text: BoxFingerprintCache（如 06-text 任务完成）或 pipeline 自行实现

### 任务要求
- 范围：仅限 crates/vtrans-pipeline；禁止修改其他 crate；禁止修改 vtrans-core
- 新增公开 API：
  - `TranslationBox` struct：`id: u32`、`region: ScreenRegion`、`color: String`
  - `MultiBoxPipeline` struct（或扩展现有 Pipeline）：
    - `new(config) -> Self`：初始化，传入多框配置
    - `add_box(box: TranslationBox) -> Result<()>`：运行时新增翻译框
    - `remove_box(box_id: u32) -> Result<()>`：运行时删除翻译框（停止对应 task）
    - `update_box(box_id: u32, region: ScreenRegion) -> Result<()>`：修改区域（需重启对应 task）
    - `start_all() -> Result<()>`：启动所有翻译框的实时翻译
    - `stop_all() -> Result<()>`：停止所有翻译框
    - `stop_box(box_id: u32) -> Result<()>`：停止单个翻译框
    - `subscribe_results() -> Receiver<BoxedTranslationResult>`：订阅多框结果流
    - `box_count() -> usize`：当前翻译框数量
  - `BoxedTranslationResult` struct：`box_id: u32`、`color: String`、`result: TranslationResult`、`timestamp: u64`
  - `BoxStatus` enum：`Running`、`Stopped`、`Error(String)`
- 行为变更：
  - 现有单框 Pipeline 保持不变，新增 MultiBoxPipeline 或在 Pipeline 上扩展多框模式
  - 每个翻译框作为独立 Tokio task 运行：独立 CaptureSession、独立 OCR、独立翻译、独立 CancellationToken
  - 帧差检测：每个框独立维护上一帧，检测变化才触发 OCR
  - 指纹去重：每个框独立维护去重缓存（使用 BoxFingerprintCache 或 HashMap<u32, Set>）
  - 有界通道：使用 mpsc channel 汇集所有框的结果，容量 = max_boxes * 2
  - task 取消：删除框或停止框时通过 CancellationToken 取消对应 task
  - 区域修改：先停止旧 task，再用新区域启动新 task
  - 错误隔离：单个框出错不影响其他框，通过 BoxStatus::Error 上报
- 约束：
  - 不修改 vtrans-core 的任何类型、trait、serde 表示
  - 必须通过 trait 调用 capture/ocr/translation（不绕过 Provider trait）
  - 图像不跨 IPC（pipeline 内部传递 CapturedImage，不序列化）
  - 错误归属：使用 PipelineError（本 crate 定义），不引入 core 错误
  - TranslationBox、BoxedTranslationResult 需实现 Serialize/Deserialize（用于 IPC 传输）
  - 多框并发安全：MultiBoxPipeline 需 Send + Sync
  - 性能约束：单框 pipeline 延迟不因多框引入而显著增加（目标：单框延迟增量 < 10%）
- 测试要求：
  - 多框并发测试：启动 2+ 框，验证各自独立运行
  - 框增删测试：运行时 add_box/remove_box，验证 task 正确启停
  - 区域修改测试：update_box 后新区域生效
  - 取消测试：stop_box 后对应 task 终止
  - 错误隔离测试：一个框出错不影响其他框
  - 去重隔离测试：框间指纹不交叉
  - 通道容量测试：结果不丢失（或有背压处理）
- 文档要求：API 变化同步本 crate README；新增类型需 rustdoc 注释
- 提交规范：`feat(pipeline): add multi-box real-time translation support`

### 横切标准提醒
- 日志：使用 tracing；每个框的 task 用 box_id 作为 tracing field；不记录完整原文/译文（用 truncate_for_log）；不记录图像数据
- 错误：使用 thiserror；错误归属 PipelineError；`#[from]` 错误链；单框错误不传播为 pipeline 致命错误
- 测试与风格：fmt/clippy 通过；并发测试覆盖；无 todo!()/unimplemented!()
- SAFETY：如使用 unsafe（如 Graphics Capture 互操作），需 SAFETY 注释

### 完成定义（DoD）
- [ ] cargo fmt --all -- --check 通过
- [ ] cargo clippy -p vtrans-pipeline --all-targets 通过
- [ ] cargo test -p vtrans-pipeline 通过
- [ ] 未修改其他 crate 与 vtrans-core
- [ ] PR 描述含实现说明、多线程方案、测试覆盖、验收 checklist
- [ ] 多框并发性能基准：8 框同时运行时无 panic、无死锁

### 待确认事项
- 现有 Pipeline struct 的公开 API 和方法签名（开发 Agent 需阅读 crates/vtrans-pipeline/src/ 确认）
- PipelineMode 的 2 个变体名称（探测未找到，需确认实际名称）
- 现有单框实时翻译的实现方式（帧差检测、去重、通道的具体实现）
- CaptureSession 是否支持多实例同时运行（需验证 Graphics Capture API 多实例可行性）
- 是否需要复用现有 Pipeline 还是新建 MultiBoxPipeline（建议新建，保持单框逻辑不变）
