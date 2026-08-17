# 模块 10：vtrans-app 应用层

| 属性 | 值 |
|------|-----|
| Crate | `vtrans-app` |
| 分支 | `feat/10-app` |
| 上游依赖 | 全部 Rust crate (core, config, security, capture, ocr, text, translation, models, pipeline) |
| 层级 | 4 |
| 复杂度 | 高 |
| 阶段 | Phase 4 |

## 职责

定义 Tauri Commands 和 Events，管理 AppState 生命周期，注册全局快捷键，组装所有模块的具体实现并注入 Pipeline；并管理多框实时翻译（`MultiBoxPipeline` 生命周期、结果/状态 forwarder、翻译框配置持久化）。是 Rust 侧与前端通信的唯一桥梁。

## 公开 API

### Tauri Commands

```rust
t::generate_handler![
    start_region_selection,
    cancel_region_selection,
    capture_once,
    start_live_translation,
    stop_live_translation,
    update_live_region, // (region, mode: "single" | "live")
    set_ocr_language,
    set_source_language,
    set_target_language,
    set_translation_provider,
    load_local_models,
    save_settings,
    update_result_window_appearance, // (opacity, fontSizePx)
    update_floating_ball_appearance, // (opacity, sizePx)
    set_api_key,
    set_provider_credentials, // (providerId, apiKey?, appId?, secret?)
    get_app_config,
    get_app_status,
    add_translation_box,      // (region) -> TranslationBoxInfo
    remove_translation_box,   // (boxId)
    update_translation_box,   // (boxId, region)
    list_translation_boxes,   // () -> Vec<TranslationBoxInfo>
    start_multi_realtime,     // ()
    stop_multi_realtime,      // ()
    stop_box,                 // (boxId)
    open_result_window,       // ()
    download_translation_model,        // ()（无参数）
    cancel_translation_model_download, // ()（无参数）
    delete_translation_model,          // ()（无参数）
    get_model_status,                  // () -> ModelStatusReport
    retry_model_setup,                 // () -> ModelStatusReport
]
```

### 翻译模型下载与状态 Commands（5 个，均无参数）

`ModelStatusReport`（IPC DTO）与单条目状态
`ModelState`（serde 小写 `"ready" | "missing" | "invalid"`）：

```
ModelStatusReport {
  entries: [{ id, state, optional }], // manifest 每个条目（OCR + 翻译，optional 含）
  ocr_ready: bool,                    // 全部 OCR 模型 + 字典就位
  translation_ready: bool,            // 翻译模型 + tokenizer 就位
}
```

分类语义与 `ModelManager::verify_integrity` 一致：optional 条目缺失是
`missing`（「未安装」，非失败）；存在但校验失败是 `invalid`。

| Command | 参数 | 返回 | 行为 |
|---------|------|------|------|
| `download_translation_model` | 无 | `()` | 从 manifest `translation.model.download_url` 流式下载到 `{data}/models/translation/model.onnx.part`（已有 `.part` 用 `Range` 头续传；`206` 追加、`200` 从头重下）；节流发射 `model_download_progress`；完成后在阻塞池做 SHA-256 校验，通过则原子 rename 为 `model.onnx`，不匹配则删除 `.part`（**损坏字节绝不覆盖已装模型**）；成功后刷新模型状态并重建本地 provider。已在下载中 → `AppError::ModelDownload`（"下载已在进行中"）；无下载 URL → `ModelDownload`；manifest 不可用 → `ModelNotReady`。promise 在完成/失败/取消时结算 |
| `cancel_translation_model_download` | 无 | `()` | 取消在途下载（CancellationToken），下载 promise 以「下载已取消」结算；`.part` **保留**供下次续传；无活动下载时 no-op |
| `delete_translation_model` | 无 | `()` | 先取消在途下载并**有界等待**（5s）槽位释放；删除 `model.onnx` 与残留 `.part`（已不存在容忍）；刷新状态并重建本地 provider（配置为 `local` 时退化到「未安装」占位）。无 manifest / 未配置翻译条目时 no-op |
| `get_model_status` | 无 | `ModelStatusReport` | **严格只读**：用与 `verify_integrity` 相同的分类重跑校验（optional 缺失 = 未安装，存在但坏 = 失败），不复制、不删除、不修复；manifest 不可用时回退启动时记录的快照（含修复错误）。前端启动错误横幅由此派生 |
| `retry_model_setup` | 无 | `ModelStatusReport` | 重跑自愈配置 pass `ensure_data_models`（manifest 与缺失/损坏的**必选**文件从包内只读源重拷，optional 条目永不复制）；有修复错误时返回 `AppError::ModelNotReady`（附原因），状态快照仍可经 `get_model_status` 获取；OCR/本地 provider 若此前退化且现已就位则重建（免重启生效） |

下载中切换 provider 到 `"local"` 被双重拒绝（后端 `reject_local_switch_during_download`
+ 前端禁用选项），避免加载半写文件；`save_settings` 也走同一守卫。

### 多框实时翻译 Commands（8 个）

`TranslationBoxInfo { box_id, region, color }` 是前端面向的框信息结构
（镜像 `vtrans_pipeline::TranslationBox`，字段名用 `box_id` 对齐 IPC 契约）。

| Command | 参数（Tauri 2 默认 camelCase） | 返回 | 行为 |
|---------|-------------------------------|------|------|
| `add_translation_box` | `{ region }` | `TranslationBoxInfo` | 配置快照分配 `next_box_id` / `next_box_color`，持久化到 `translation_boxes`；懒初始化 `MultiBoxPipeline` 并注册框；发射 `multibox://box-added`；框数达到 `warning_threshold`（>0 且 count ≥ 阈值）时发射 `multibox://warning`（非阻塞） |
| `remove_translation_box` | `{ boxId }` | `()` | 从 pipeline 移除（停任务、清去重/状态；`BoxNotFound` 容忍）并删配置条目；发射 `multibox://box-removed` |
| `update_translation_box` | `{ boxId, region }` | `()` | 校验区域；pipeline `update_box`（运行中停旧任务以新区域重启）；更新配置；发射 `multibox://box-updated` |
| `list_translation_boxes` | 无 | `Vec<TranslationBoxInfo>` | 从持久化配置读取（跨重启存活，pipeline 未启动亦可列出） |
| `start_multi_realtime` | 无 | `()` | 清旧 pipeline，从当前配置重建并注册全部框；spawn forwarder task（结果转发 + 状态轮询）；`start_all`；显示 overlay 窗口；`start_all` 成功后记录 `mode = live`（失败不改 mode） |
| `stop_multi_realtime` | 无 | `()` | `stop_all`；清 pipeline + forwarder；隐藏 overlay；对每个框发射 `multibox://status`（`Stopped`）；回退 `mode = single`（单框 live 仍在运行时保持 live） |
| `stop_box` | `{ boxId }` | `()` | 停止单框任务（框保持注册）；发射该框 `Stopped` 状态 |
| `open_result_window` | 无 | `()` | 显示/聚焦预声明的 `result` 窗口（窗口已在 tauri.conf.json 声明、关闭即隐藏，不新建） |

错误映射：`add_box` 的 `BoxLimitExceeded` / `DuplicateBoxId` / `InvalidConfig`
与 `update_box` / `remove_box` 的 `BoxNotFound` 等经 `AppError::Pipeline` 传给
前端（`remove` / `update` 对 `BoxNotFound` 容忍继续）。

### 翻译 Provider 组装与凭据目标

`state.rs` 的 `build_translation_provider(config, credentials, model_manager)`
按 `config.translation.provider` 分支组装，凭据只经 `CredentialManager`
读取（不落内存副本到日志）：

| 配置 id | 运行时实现 id | 凭据目标 |
|---------|---------------|----------|
| `openai`（默认） | `openai` | `openai` |
| `deepl` | `deepl` | `deepl` |
| `google` | `google` | `google` |
| `azure` | `azure` | `azure`（区域取 `translation.region`） |
| `baidu` | `baidu` | `baidu_app_id` + `baidu_secret`（两个独立目标） |
| `local` | `local-onnx` | 无（ONNX 模型走 ModelManager） |

配置域白名单 `["openai","deepl","google","azure","baidu","local"]` 与
vtrans-config 校验一致；旧 id `"api"` 已废弃（v4→v5 迁移重命名为
`"openai"`，应用层校验拒绝）。切换 provider（`set_translation_provider` /
`save_settings`）即时重建并更新配置；live 会话运行中切换仍返回
`PipelineError::AlreadyRunning`。

`set_api_key` 按当前配置的 provider 写入对应凭据目标（baidu 写
`baidu_secret`），`set_provider_credentials` 泛化写入完整凭据集（baidu
需要 `appId` + `secret` 两个值）；写入后若目标 provider 为当前 provider
立即重建。日志引用 Key 只用 `mask_sensitive` / `mask_key` 掩码。

`update_result_window_appearance(opacity, font_size_px)` 与
`update_floating_ball_appearance(opacity, size_px)` 只持久化对应窗口的两个
外观字段（加载配置 → 修改 → `save_config` 校验 + 原子写），**不获取 live
生命周期锁、不检查 live 任务、不重建 Provider**——外观调整在实时会话运行中
仍可保存（bug 2 后端侧）。越界值由配置校验返回 `ConfigError::Validation`
并映射为 `AppError::Config`。参数名遵循 Tauri 2 默认 camelCase。

### AppState

```rust
pub struct AppState {
    config: RwLock<ConfigManager>,
    credentials: Arc<CredentialManager>,
    pipeline: RwLock<Option<Pipeline>>,          // 单框流水线
    ocr_provider: RwLock<Box<dyn OcrProvider>>,
    translation_provider: RwLock<Box<dyn TranslationProvider>>,
    capture_source: WindowsCaptureSource,
    model_manager: Option<Arc<ModelManager>>,   // {data}/models 的清单；加载失败为 None（降级启动）
    data_models_dir: PathBuf,                   // 运行时模型目录（默认 {exe}/data/models，可被 config.model_dir 覆盖）
    bundled_models_dir: Option<PathBuf>,        // 包内只读模型源 resource_dir()/resources/models
    model_status: RwLock<ModelStatusReport>,    // 模型就位快照（启动/下载/删除/重试后更新）
    model_download: RwLock<Option<CancellationToken>>, // 在途翻译模型下载槽（Some = 下载占用）
    // ── 多框实时翻译 ──
    multi_pipeline: RwLock<Option<Arc<MultiBoxPipeline>>>, // 懒初始化
    multi_forwarder: Mutex<Option<JoinHandle<()>>>,        // 结果/状态 forwarder task
    multi_box_ids: Arc<RwLock<Vec<u32>>>,                  // 注册框 id（供状态轮询）
    // ── 框选窗口可见性 ──
    selection_visibility: SelectionVisibilityState, // 框选前可见快照（首次优先，恢复即清）
}

impl AppState {
    pub fn new(app_data_dir: &Path) -> Result<Self, AppError>;
    fn build_multi_pipeline(&self) -> Result<MultiBoxPipeline, AppError>; // 复用单框同套 provider
    fn ensure_multi_pipeline(&self) -> Result<Arc<MultiBoxPipeline>, AppError>; // 懒创建
    async fn clear_multi_pipeline(&self);  // abort forwarder + 清 pipeline/ids
    async fn set_multi_forwarder(&self, task: JoinHandle<()>);
    fn add_multi_box_id(&self, box_id: u32);    // 去重登记
    fn remove_multi_box_id(&self, box_id: u32);
    fn multi_box_ids_snapshot(&self) -> Vec<u32>;
    // ── 模型下载槽（commands.rs 下载流程持有 CancellationToken）──
    fn try_start_model_download(&self, token: CancellationToken) -> bool; // 占用槽位，已在下载返回 false
    fn finish_model_download(&self);              // 下载任务结束后释放槽位
    fn model_download_active(&self) -> bool;      // 在途下载标记（provider 切换守卫用）
    fn cancel_model_download(&self);              // 请求取消在途下载
    async fn cancel_and_wait_model_download(&self, wait: Duration); // 取消并有界等待槽位释放
}
```

多框要点：

- `build_multi_pipeline` 用 `MultiBoxConfig::with_max_boxes(capture.interval_ms,
  capture.difference_threshold, ocr_options, translation_request,
  config.max_boxes)`，捕获/OCR/翻译 provider 与单框流水线**共享同一组
  `Arc`**——provider 切换对单框与多框同时生效；`multi_pipeline` 懒创建，
  配置变更在下一次 `ensure_multi_pipeline` 生效。
- forwarder task（`run_multi_forwarder`）：`subscribe_results()` 逐条发射
  `multibox://result`；每 500ms 轮询 `pipeline.box_status()`，状态与上次
  快照不同才发射 `multibox://status`（错误状态最多延迟 500ms）；结果流
  关闭（pipeline 被清）时退出。`clear_multi_pipeline` 先 abort forwarder
  再清 pipeline 与 id 列表，保证下次 `start_multi_realtime` 从最新配置重建。
- 单框与多框互不影响：`Pipeline` 与 `MultiBoxPipeline` 独立实例，共享
  provider 但各持独立 CaptureSession（可同时运行，资源消耗加倍）。

### 数据目录锚定与启动容错（发行部署 R1–R6）

- **数据根**：`paths.rs::resolve_data_root` = `{exe_dir}/data`（纯路径运算
  `resolve_data_root_for` + 启动时 `create_dir_all`）。安装版与开发版都不再
  写 `%APPDATA%`；目录创建失败仅 `warn!` 并继续（下游 `ConfigManager` 会给
  出明确错误）。数据根内布局：`config.json` / `credentials.bin` / `logs/` /
  `models/`（详见 DEVELOPMENT.md §9）。
- **旧配置一次性迁移**：`migrate_legacy_config_if_needed` 在首次启动时把
  `%APPDATA%\com.vtrans.app\config.json` 复制进便携数据根（仅当便携文件尚
  不存在），失败仅 `warn!` 不阻断启动。
- **包内模型源**：`bundled_models_dir` = `resource_dir()/resources/models`
  （安装版与 dev 均在 exe 目录旁），缺失时回退源码检出
  `src-tauri/resources/models`；两者皆无则模型自愈禁用（启动继续、状态
  快照记录错误）。
- **启动自愈**：`ensure_data_models`（model_setup.rs）每次启动与
  `retry_model_setup` 时执行——manifest 与缺失/校验失败的**必选**文件从
  包内源重拷（幂等、自愈，删除/损坏 `{data}/models` 后重启即恢复）；
  optional 条目（翻译模型）**永不复制**，只分类（missing 保持 missing）。
- **启动容错**：配置或采集初始化失败才终止启动；**模型问题永不阻断启动**：
  manifest 不可用 → `model_manager: None`；OCR 加载失败 →
  `UnavailableOcrProvider`（id `unavailable-ocr`）；配置的翻译 provider
  组装失败 → `UnavailableTranslationProvider`（id `unavailable-translation`），
  均以明确中文错误应答。翻译入口命令（`capture_once` / `start_live_translation` /
  `start_multi_realtime`）经 `ocr_ready_gate` 在 OCR 未就位时直接返回
  `AppError::ModelNotReady`（"OCR 模型未就位，请重试模型修复"），而非运行
  中静默失败；修复后（重试/重启）OCR 与本地 provider 按需重建，免重启生效。
- **DpapiFileStore 构造点**：`state.rs::build_credential_manager` 在
  `AppState::new_with_debug` 内构造 `DpapiFileStore::new(data_root/credentials.bin)`
  ；（容器尚不存在 = 本数据根首启）时调用 `migrate_windows_to_dpapi` 一次性
  迁移旧 Windows 凭据管理器条目。回退链：DPAPI 文件存储不可用 → 系统凭据
  管理器；再不可用 → 内存存储（凭据不持久化）。迁移/构造失败均不阻断启动，
  需要凭据时报明确错误。

### Events

```rust
pub fn emit_pipeline_event(app: &AppHandle, event: PipelineEvent);
pub fn emit_overlay_region(app: &AppHandle, region: &ScreenRegion);
pub fn emit_overlay_hidden(app: &AppHandle);
pub fn emit_debug_frame(app: &AppHandle, payload: DebugFramePayload);
```

### 多框与结果窗口 Events（7 个，events.rs 常量 + 发射函数）

| 事件名 | payload 形状 | 触发时机 |
|--------|-------------|----------|
| `multibox://result` | `BoxedTranslationResult`：`{ box_id, color, result: { translated_text, provider_id, elapsed_ms }, original_text, timestamp }` | forwarder 收到一条多框结果即转发 |
| `multibox://box-added` | `{ box_id, color, region }` | `add_translation_box` 成功 |
| `multibox://box-removed` | `{ box_id }` | `remove_translation_box` 成功 |
| `multibox://box-updated` | `{ box_id, region }` | `update_translation_box` 成功 |
| `multibox://status` | `{ box_id, status }`；`status` 为 `BoxStatus` 的 serde 表示：单元变体序列化为字符串 `"Running"` / `"Stopped"`，`Error` 为 `{"Error": "<msg>"}` | 状态变化（forwarder 500ms 轮询发现变化、stop 命令主动发射） |
| `multibox://warning` | `{ current_count, max_count }` | 添加框后数量达到 `warning_threshold` |
| `translation://single-result` | `{ original_text, translated_text, timestamp }` | 单次捕获完成后（OCR 与翻译均可得且原文非空），供结果窗口同时展示原文+译文 |

日志纪律：`emit_multibox_result` 只记 `box_id`，译文不落日志；
`emit_translation_single_result` 的原文/译文仅以 `truncate_for_log` 截断形式
进入 debug 日志。

### 模型下载进度 Event（1 个）

| 事件名 | payload 形状 | 触发时机 |
|--------|-------------|----------|
| `model_download_progress` | `{ bytes: u64, total: u64, fraction: f32 }`（snake_case） | `download_translation_model` 期间节流发射：至少每 500ms 或每 1MiB 一次，完成时必发（`fraction = 1.0`）。`bytes` 含续传前缀；`total` 未知时为 0、`fraction` 为 0；发射失败仅 warn（设置面板关闭不影响下载） |

下载流程日志只记录 URL 主机名（不落完整下载地址），下载完成的 sha256 校验
失败会删除 `.part` 回滚。

`original_text` 语义（与 pipeline 一致）：多框 `multibox://result` 携带清洗
后的 OCR 原文（F1/F2 落地）；翻译失败或 OCR 空文本时该字段与译文均为空串
（前端据此清除该框 overlay 残留），取消不发射。

### 全局快捷键

```rust
pub fn register_hotkeys(app: &AppHandle) -> Result<(), AppError>;
// Alt+Shift+A: start_region_selection (单次)
// Alt+Shift+R: start_live_translation (单框实时)
// Alt+Shift+S: stop_live_translation (单框实时)
```

**热键语义（用户确认的设计决策）**：Alt+Shift+R / Alt+Shift+S 始终控制
**单框**实时会话（`hotkeys.rs` 仅注册 Select / StartLive / StopLive 三个动作，
均走 `start_live_task` / `stop_live_task` / `select_region`）；多框实时翻译的
启动 / 停止**仅由 UI 按钮**调用 `start_multi_realtime` / `stop_multi_realtime`，
不注册多框热键、不复用 R/S。

### 窗口生命周期与托盘

- 关闭主窗口 → 隐藏到系统托盘（进程、实时会话、全局快捷键继续运行）；
- 托盘左键 / 菜单「显示主窗口」→ 恢复主窗口；菜单「退出」→ `app.exit(0)`；
- 单实例插件拦截第二个进程实例并恢复已有实例主窗口。

### 框选期间的窗口隐藏与恢复（Bug-005）

框选开始到后续动作完成之间，屏幕应只保留透明选区窗口（selector），
main/result/floater 不得遮挡或混入被框选内容。生命周期契约（实现于
`window_visibility.rs`，决策集中在纯函数/纯状态机，均有单测）：

- **框选开始**（`select_region` 进入时）：隐藏 main/result/floater 中当前
  可见的窗口，并记录「框选前可见集合」快照。已有未恢复快照时**保留首次
  快照、不覆盖**（避免恢复时丢失窗口），但仍隐藏此刻可见的窗口。
- **立即恢复**：框选取消（`cancel_region_selection`）、超时、selector 不可用
  等失败路径调用 `restore_app_windows_immediately`。
- **延迟恢复**：框选成功后保持隐藏，直到后续动作命令完成——`capture_once`、
  `start_live_translation`、`add_translation_box`、`update_translation_box`
  的包装函数在**成功与失败**后都调用 `restore_app_windows_after_follow_up`
  （恢复后清空快照）。`start_live_translation` 同时被「恢复/热键 R」路径
  调用：仅当存在未恢复快照时才恢复，无快照时为 no-op，不影响正常启停。
- **恢复规则**：只恢复快照中记录的窗口；floater 额外受
  `floating_ball.enabled` 配置约束（配置禁用则不恢复显示）；result / main
  仅当快照中可见才恢复（`PreSelectionVisibility::restore_plan` 纯函数）。
- **容错**：窗口可见性查询失败按「不可见」处理；hide/show 失败仅 `warn!`
  记录，不破坏框选主流程；不新增 IPC 命令，全部为 AppHandle 内部窗口调用。
- selector/overlay 显隐逻辑不受本机制影响（overlay_intent 等既有逻辑不变）。

### 选区 overlay

- 选区确认（`update_live_region(region, mode)`）→ overlay 窗口覆盖区域
  所在显示器（窗口原点 = 显示器原点），前端 `regionOverlay` 服务定位/
  显示/点穿，后端同步显示兜底；CSS 边框 + 尺寸标签绘制在区域相对偏移处
  （事件 `overlay_region_updated`）；
- **显隐规则按模式区分**（决策集中在 `overlay.rs` 的可测纯函数
  `overlay_intent` / `overlay_intent_for_stop`）：
  - 单次模式：选区确认（`mode: "single"`）**不显示**常驻方框；
    `capture_once` 完成（成功或失败）后确保隐藏，方框不残留；
  - 实时模式：`start_live_translation` 启动即显示；`update_live_region`
    （`mode: "live"`）更新区域时显示新位置；**暂停保留**方框（暂停走
    `stop_live_translation`，后端按 `Pause` 决策不隐藏）；**真正停止**
    隐藏（UI 停止按钮先隐藏再 stop、热键 Alt+Shift+S 走 `Stop` 决策由
    后端隐藏）；
- 重新选区 / 取消 → 隐藏（事件 `overlay_hidden`）；
- overlay 无边框、透明、置顶、可点穿（`focusable: false` +
  `set_ignore_cursor_events(true)`），只传输 `ScreenRegion` 坐标，不跨 IPC
  传图像。
- `AppStatus.mode`（`"single"` / `"live"`）是后端最近会话模式的权威记录
  （单次捕获与确认报告 `single`，实时会话运行或暂停均报告 `live`；多框
  实时会话运行期间 mode 报告 `live`，停止且无单框 live 会话时回退
  `single`——Bug-004 后端侧）；前端启动水合只在 `mode == "live"` 时恢复
  常驻方框，单次模式的选择区域不会在重启后显示方框。

### 窗口清单（悬浮球窗口）

VTrans 共声明 5 个窗口（`src-tauri/tauri.conf.json`，由模块 10 维护）：

| label | 角色 | 关键配置 |
|-------|------|---------|
| main | 主窗口 | 420×600，可缩放，应用入口 |
| result | 结果窗口 | 360×140（迷你条形态）、默认隐藏、置顶、`transparent: true`、无边框（`decorations: false`）、可缩放 |
| selector | 选区窗口 | 全屏、透明、无边框，默认隐藏 |
| overlay | 选区方框 | 全屏、透明、无边框、置顶、可点穿，默认隐藏 |
| floater | 悬浮球 | 48×48、透明、无边框、置顶、跳过任务栏、`focus: false`、默认隐藏 |

floater（悬浮球）：

- 由前端按 `floating_ball.enabled`（vtrans-config，默认 false）显示；拖动
  复用既有窗口 API（show/hide/start-dragging/set-position/
  set-always-on-top），**不新增 IPC Command**；
- **不点穿**：与 overlay 不同，不做 `setIgnoreCursorEvents`；
- 关闭请求被全局 hide-on-close（prevent_close + hide）覆盖，隐藏而非销毁；
- 默认 `visible: false`，启动不产生可见窗口。

**WDA 捕获排除（Bug-006）**：VTrans 用 WGC 显示器级捕获（
`CreateForMonitor`），屏幕上一切窗口都会进帧。应用启动、窗口创建完成后
（`init_app`，在 `window_exclusion.rs` 实现），对 **main/result/floater**
三个窗口调用 `SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)`
（值 0x11），被标记窗口从所有捕获表面消失、露出背景（2026-08-14 本机
Windows 11 实测：红色测试窗口在捕获中占比 398‰ → 0‰）。

- 决策集中在纯函数 `capture_exclusion_windows()`（= main/result/floater，
  单测锁定集合）。**selector 与 overlay 不设置 WDA**：selector 全屏选区
  只在框选瞬间显示、期间无捕获；overlay 方框描边已外移出捕获区域（模块
  11 `fix/11-overlay-border-outside` 配合）。
- HWND 经 Tauri 2 `WebviewWindow::hwnd()`（Windows）获取；vtrans-app 依赖
  `windows` 0.61（与锁定 tauri 2.11.5 同版本线，HWND 类型一致；仅
  `Win32_Foundation` + `Win32_UI_WindowsAndMessaging` 特性）。
- 容错：窗口缺失 / 句柄失败 / Win32 调用失败仅 `warn!` 记录 label（不
  记录窗口内容），逐窗口独立、不中断其余窗口、不影响启动；Win32 调用与
  决策分离（`apply_capture_exclusions` 可注入设置器，单测覆盖「单窗失败
  不中断」）。
- 已知副作用（用户已接受）：VTrans 窗口在一切第三方捕获（截图/录屏/
  共享）中不可见；依赖 Windows 10 2004+，换机/上线前建议
  `cargo run -p vtrans-capture --example wda_probe` 复核。

### capability 归属

`src-tauri/capabilities/default.json` 由模块 10 统一维护，`windows` 覆盖
main/result/selector/overlay/floater 五个窗口。清单按前端实际调用的窗口
API 复核：`allow-available-monitors`、`allow-set-position`、
`allow-set-size`、`allow-set-ignore-cursor-events` 由 `regionOverlay` 服务
使用（常驻方框的显示器枚举/定位/点穿）；`allow-show`/`allow-hide`/
`allow-set-focus`/`allow-set-always-on-top`/`allow-start-dragging` 由结果
窗口、选区窗口与悬浮球共用；无多余权限。

**透明度能力验证结论（feat/10-floating-ball-window）**：锁定的 Tauri
2.11.5（tauri-runtime-wry 2.11.4、@tauri-apps/api 2.11.1）不提供**运行时**
opacity 能力——Rust `WebviewWindow` 无 `set_opacity`、JS 无
`setOpacity()`、ACL 无 `core:window:allow-set-opacity`（tauri-build 实测
`Permission ... not found`，源码 grep 零命中）。透明是窗口配置属性，不
需要 ACL；result 窗口已声明 `transparent: true`（追加提交），前端可直接
用 CSS 背景 alpha 实现半透明透出桌面。Windows 透明窗口验证结论：文字
渲染/缩放重绘与 overlay/selector 同机制（overlay 已在透明窗口渲染尺寸
标签）；result 已声明 `decorations: false`（无原生标题栏，标题栏/关闭按钮
由前端 CSS + `startDragging` 提供，capability 已含 `allow-start-dragging`）；
原生阴影在分层窗口上可能不渲染，登记 README 手工验证项 14。

**待架构确认（不阻塞 MVP）**：常驻方框目前为纯 CSS 边框，不显示区域真实
缩略图；若产品要求屏幕缩略预览，需由 vtrans-app 提供小尺寸位图事件/命令
（`CapturedImage` 不得跨 IPC）。命令命名约定维持 Tauri 默认 camelCase，
如需统一 snake_case 需前后端同步修改（见 `contracts.rs` 注释）。

### Debug 模式（捕获帧预览）

- **开关**：`--debug` 或 `VTRANS_DEBUG=1`（`true` 亦可），默认关闭，不写入
  config.json；解析失败不影响启动。`AppStatus.debug_mode` 透传给前端决定
  是否渲染调试面板。
- **帧出口**：vtrans-pipeline 的 `FrameSink`（`Pipeline::with_frame_sink`）
  在捕获帧进入 OCR 前调用；关闭时无 sink、零开销。
- **编码**：`encode_debug_thumbnail` 纯函数（BGRA8/RGBA8 → ≤480px JPEG80
  Base64），在阻塞池执行；帧序号/时间戳随 payload 发送。
- **事件**：`debug_frame_updated`（仅 Debug 开启时发射），payload 含
  `image`/`region`/`frame_index`/`timestamp_ms`；≤10fps 节流、watch 最新值
  语义、编码失败 `warn!` 跳过。区域元数据：单次翻译取命令传入的区域，
  实时会话跟随 `update_live_region` 的选区；`frame_index` 按捕获帧递增，
  被节流/编码失败跳过的帧留下序号缺口，前端可据此判断丢帧。
- **隐私**：只显示不保存（不落盘、不进日志、不进 store/结果窗口）；此
  缩略图跨 IPC 是 Debug-only 显式豁免（`CapturedImage` 默认禁止序列化）。

## 错误类型

```rust
[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("state not initialized")]
    NotInitialized,
    #[error("pipeline error: {0}")]
    Pipeline(#[from] PipelineError),
    #[error("config error: {0}")]
    Config(#[from] ConfigError),
    #[error("security error: {0}")]
    Security(#[from] SecurityError),
    #[error("model error: {0}")]
    Model(#[from] ModelError),
    #[error("capture error: {0}")]
    Capture(#[from] CaptureError),
    #[error("ocr error: {0}")]
    Ocr(#[from] OcrError),
    #[error("translation error: {0}")]
    Translation(#[from] TranslationError),
    #[error("invalid api key: {0}")]
    InvalidApiKey(String),
    #[error("provider credential error: {0}")]
    ProviderCredential(String),
    #[error("tauri error: {0}")]
    Tauri(String),
    #[error("hotkey registration failed: {0}")]
    HotkeyFailed(String),
}
```

## 内部文件结构

```text
crates/vtrans-app/
├── Cargo.toml
├── README.md
└── src/
    ├── lib.rs          # re-export, init_app
    ├── state.rs         # AppState（含 DpapiFileStore 构造、启动容错、下载槽）
    ├── commands.rs      # Tauri command handlers（含 5 个模型下载/状态命令）
    ├── events.rs        # 事件发送封装（含 model_download_progress）
    ├── hotkeys.rs       # 全局快捷键注册
    ├── paths.rs         # 便携数据根解析 + 旧配置一次性迁移
    ├── model_setup.rs   # ensure_data_models 自愈配置 + 只读 ModelStatusReport
    ├── model_download.rs # 翻译模型下载编排（续传/校验/原子安装）
    └── setup.rs         # 应用启动初始化
```

## 测试计划

| 测试项 | 类型 | 说明 |
|--------|------|------|
| AppState 初始化 | 手工验证 | 依赖 Windows Graphics Capture、Credential Manager 和模型文件，无头环境不可自动化；验证步骤见 crate README「手工验证项」第 1 条 |
| save_settings | 手工验证 + 单元 | 全链路（IPC → 持久化 → 重启生效）手工验证（README 第 2 条）；配置校验与原子持久化由 vtrans-config 单测覆盖 |
| get_app_status | 手工验证 + 单元 | 全链路手工验证（README 第 3 条）；`AppStatus` 序列化契约有单测 |
| Provider 切换 | 手工验证 + 单元 | 全链路手工验证（README 第 4 条）；6 个配置 id 白名单、`update_translation_provider_config`、5 个云 Provider 组装分支有单测 |
| Provider 凭据 | 单元 | 凭据目标映射、`store_api_key` / `store_provider_credentials` 目标读写、百度双目标、缺失凭据容忍（state.rs tests）✅；IPC 参数契约见 contracts.rs |
| set_api_key / set_provider_credentials | 手工验证 + 单元 | 全链路手工验证（README 第 7 条）；key 校验与凭据存储有单测，IPC 参数契约见 contracts.rs |
| get_app_config | 手工验证 + 单元 | 前端水合手工验证（README 第 8 条）；AppConfig 序列化字段契约见 contracts.rs |
| 快捷键注册 | 手工验证 | 依赖真实全局快捷键注册环境（README 第 5 条）；动作分派枚举有单测 |
| 托盘与窗口生命周期 | 手工验证 | 依赖系统托盘与真实窗口环境（README 第 9 条）；菜单 id 与事件名有单测 |
| 选区 overlay | 手工验证 + 单元 | 依赖真实显示器（README 第 10 条）；显隐决策纯函数（单次确认不显示/实时启动显示/实时停止隐藏/暂停保留）与坐标换算有单测 |
| 框选窗口隐藏/恢复 | 手工验证 + 单元 | 真实窗口显隐登记 README 手工验证项 15；快照状态机（首次优先/立即恢复/延迟恢复）与恢复计划纯函数有单测（window_visibility.rs tests）✅ |
| 单实例保护 | 手工验证 | 依赖进程级行为（README 第 11 条） |
| Debug 模式开关 | 单元 | `parse_debug_env_value` 值域解析（setup.rs tests）✅ |
| 缩略图编码 | 单元 | 尺寸缩放/JPEG 有效/格式与缓冲校验（debug_frame.rs tests）✅ |
| Debug 帧出口 | 集成 | FrameSink 收到进入 OCR 前的帧、跳过未变化帧（pipeline 集成测试）✅ |
| Debug 面板显示 | 手工验证 | 依赖真实显示器与模型（README 第 12 条） |
| 错误映射 | 单元 | 各模块错误正确映射到 AppError（error.rs tests）✅ |
| 多框状态快照 mode | 单元 | 多框启动成功记 `live` / `start_all` 失败不改 / 停止回退 `single` / 单框 live 运行中停止保持 `live`（commands/state tests，`mode_after_multi_stop` + `MultiBoxSessionHost` 注入）✅ |
| 捕获排除（WDA） | 手工验证 + 单元 | 真实桌面截图验证登记 README 手工验证项 16；排除集合纯函数（{main,result,floater}、不含 selector/overlay）与「单窗失败不中断其余窗口」（mock 设置器）有单测（window_exclusion.rs tests）✅ |
| 事件发送 | 单元 | PipelineEvent 正确转为前端事件（events.rs tests）✅ |

> 说明：依赖 Windows 桌面环境的 5 项已登记为手工验证，具体步骤与验证点见
> `crates/vtrans-app/README.md`「手工验证项」一节；其纯逻辑部分均有自动化测试。

### Provider id 契约

`AppStatus.translation_provider` 返回运行时 Provider 的实现 id。云端
Provider 与配置 id 一致（`"openai"` / `"deepl"` / `"google"` / `"azure"` /
`"baidu"`），仅本地不同（实现 `"local-onnx"` ↔ 配置 `"local"`）。两端映射：

- 后端：`validate_translation_provider_id` 维护配置标识符白名单
  （`["openai","deepl","google","azure","baidu","local"]`）；
- 前端：`normalizeProviderId` 把 `"local-onnx"` 映射回 `"local"`，其余
  实现 id 原样透传。

**新增翻译 Provider 时**，必须同步更新后端白名单与前端 `normalizeProviderId`
映射，并在 `crates/vtrans-app/README.md` 登记，否则状态水合会显示错误引擎。

## 验收标准

- [x] 所有 Commands 可被前端调用（`invoke_handler` 注册 31 个命令——18 个
      基础命令 + 8 个多框命令 + 5 个模型下载/状态命令，前端
      `src/services/tauri.ts` 全部按 Tauri 2 camelCase 参数契约调用）
- [x] 所有 Events 正确推送到前端（`events.rs` 单测 + `tests/contracts.rs`
      固化了事件名与 payload 形状）
- [x] AppState 正确组装各模块实现（`AppState::new` / `new_with_debug`）
- [x] 快捷键可注册和触发（实现 + 动作分派单测；真实注册依赖桌面环境，
      登记 README 手工验证项 5）
- [x] 错误信息对用户友好（`AppError` 序列化为纯字符串，`error.rs` 单测）
- [x] UI 线程不被阻塞（模型校验、缩略图编码走 `spawn_blocking`）
- [x] Release 构建关闭不必要 capability（Debug 面板采用主窗口内嵌方案，
      不新增窗口/权限；capability 归属见「capability 归属」一节）
- [x] README.md 完整

### 多 Provider 验收（本任务）

- [x] `build_translation_provider` 按 `openai`/`deepl`/`google`/`azure`/
      `baidu`/`local` 六分支组装，凭据从 CredentialManager 对应目标读取
      （5 个云分支有单测，local 依赖真实模型文件登记手工验证）
- [x] OpenAI 配置 id 为 `openai`（默认 provider），旧 `"api"` 被校验拒绝
- [x] `set_api_key` 泛化为按当前 provider 写对应目标（baidu 写
      `baidu_secret`），写入后立即重建当前 provider
- [x] `set_provider_credentials` 支持 baidu 双目标（`app_id` + `secret`），
      IPC 参数契约（`{ providerId, apiKey?, appId?, secret? }`）已固化
- [x] 切换 provider 即时重建并更新配置；live 会话运行中切换仍拒绝
- [x] `AppStatus.translation_provider` 运行时 id 与前端映射对齐
      （`openai`/`deepl`/`google`/`azure`/`baidu`/`local-onnx`）
- [x] 凭据只经 CredentialManager 读写，日志仅掩码形式；百度 APP ID 与
      Secret 两个独立目标
- [x] 未修改 vtrans-core 与其它 crate；README 与本文档同步

### Debug 模式验收（本任务）

- [x] 开关：`--debug` / `VTRANS_DEBUG=1`（`true` 亦可），默认关闭、不持久化；
      解析失败不影响启动（`parse_debug_env_value` 单测）
- [x] 关闭时零开销：不挂 `FrameSink`、不注册 `debug_frame_updated`、无面板
- [x] 帧出口：pipeline `FrameSink` 在进入 OCR 前收到帧（pipeline 集成测试
      `frame_sink_observes_*`），live 模式帧差未触发时不产生调试帧
- [x] 编码：纯函数 `encode_debug_thumbnail`（≤480px JPEG80），单测覆盖
      缩放、格式转换与非法缓冲
- [x] 传输：`debug_frame_updated` 事件 payload（`image`/`region`/
      `frame_index`/`timestamp_ms`）契约有单测；节流 ≤10fps；区域元数据
      单次取命令区域、实时跟随选区；编码失败 `warn!` 跳过且不影响翻译
- [x] 日志纪律：Debug 帧不进日志，开关状态只记一行 `info!`
- [x] 隐私：只显示不保存（不落盘、不进日志、不进 store/结果窗口）；面板
      显示依赖真实桌面环境，登记 README 手工验证项 12

### overlay 生命周期验收（Bug-003）

- [x] 单次模式：选区确认（`mode: "single"`）不显示常驻方框；`capture_once`
      完成（成功或失败）后隐藏方框
- [x] 实时模式：启动显示、`update_live_region`（`mode: "live"`）更新显示、
      暂停保留（`stop_live_translation` 按 `Pause` 决策不隐藏）、真正停止
      隐藏（UI 按钮先隐藏再 stop、热键按 `Stop` 决策后端隐藏）
- [x] 决策逻辑集中为纯函数：`overlay_intent` / `overlay_intent_for_stop`
      （overlay.rs tests 覆盖四个场景）
- [x] IPC 契约已定稿：`update_live_region(region, mode)`（Tauri 2 默认
      camelCase，前端传 `{ region, mode }`）；`AppStatus.mode`（`"single"`/
      `"live"`）；contracts.rs 与前端类型/测试已同步
- [x] 启动水合：前端仅在 `snapshot.mode === "live"` 时恢复常驻方框；
      单次模式的选择区域重启后不显示方框

### 悬浮球窗口与透明度能力验收（feat/10-floating-ball-window）

- [x] `tauri.conf.json` 声明 floater 窗口：48×48、`resizable: false`、
      `decorations: false`、`transparent: true`、`alwaysOnTop: true`、
      `skipTaskbar: true`、`shadow: false`、`focus: false`、
      `visible: false`（默认隐藏，前端按配置显示）
- [x] capability `windows` 纳入 `"floater"`；悬浮球复用既有窗口权限
      （show/hide/start-dragging/set-position/set-always-on-top/
      available-monitors），无新增权限
- [x] 透明度能力验证结论已记录：Tauri 2.11.5 无运行时 opacity（Rust/JS/ACL
      均无，`core:window:allow-set-opacity` 构建期报 not found）；result
      窗口已声明 `transparent: true`，前端 CSS alpha 可直接实现半透明
- [x] 行为变更最小：floater 默认隐藏、全局 hide-on-close 覆盖；未新增
      IPC Command；未修改其他 crate 与 vtrans-core
- [x] 回归：`cargo fmt --all -- --check`、`cargo clippy -p vtrans-app
      --all-targets`、`cargo test -p vtrans-app`、`cargo check --workspace`
      全绿

### result 窗口透明化验收（方案 1，追加提交）

- [x] result 窗口声明 `transparent: true`（与 overlay/selector 相同的
      WebView2 透明机制，前端后续用 CSS 背景 alpha 实现半透明）
- [x] 初始尺寸调整为迷你条形态 360×140，保留 `resizable: true` /
      `visible: false` / `alwaysOnTop: true`
- [x] 未新增 capability 权限（透明是窗口配置属性，不需要 ACL）
- [x] 验证结论已记录：配置经 `cargo check -p vtrans` 构建期校验；文字
      渲染/缩放/阴影表现登记 README 手工验证项 14（overlay/selector 已用
      同一机制渲染文字标签）
- [x] 回归：fmt / clippy / app 单测 / workspace check 全绿；未修改其他
      crate 与 vtrans-core

### 无边框弹窗与外观命令验收（fix/10-window-appearance-commands）

- [x] result 窗口新增 `decorations: false`（无原生标题栏；`resizable` /
      `visible` / `alwaysOnTop` / `transparent` 保留）
- [x] `update_result_window_appearance(opacity, font_size_px)`：加载配置 →
      修改 `result_window` 两字段 → `save_config`（内部校验 + 原子写）；
      不获取 live 生命周期、不检查 live 任务、不重建 Provider
- [x] `update_floating_ball_appearance(opacity, size_px)`：同上，修改
      `floating_ball` 两字段
- [x] 越界值由 `save_config` 校验返回 `ConfigError::Validation`，映射为
      `AppError::Config`
- [x] 两个命令已注册进 `invoke_handler`；contracts.rs 补充 camelCase
      参数契约（`{ opacity, fontSizePx }` / `{ opacity, sizePx }`）
- [x] 测试：字段更新范围、越界拒绝且不落盘、边界值接受、live 运行中仍可
      保存（持久化路径独立于 pipeline/provider 状态）
- [x] 回归：fmt / clippy / app 单测 / workspace check 全绿；未修改其他
      crate 与 vtrans-core

### 多框实时翻译验收（本任务）

- [x] 8 个 Command 全部注册进 `invoke_handler` 并按 Tauri 2 默认 camelCase
      映射参数（`boxId` 等）；`TranslationBoxInfo` IPC 字段名为 `box_id`
      （contracts.rs 与前端 multiboxTypes.test.ts 双向断言）
- [x] 7 个 Event 常量与前端一致：`multibox://result` / `box-added` /
      `box-removed` / `box-updated` / `status` / `warning`、
      `translation://single-result`；payload 形状与 TypeScript 类型一一对应
- [x] `add_translation_box` 用配置 `next_box_id` / `next_box_color` 分配并
      持久化；达到 `warning_threshold`（>0）发射 `multibox://warning`
- [x] `start_multi_realtime` 从配置重建 pipeline、spawn forwarder、
      `start_all`、显示 overlay；`stop_multi_realtime` 停全部并发射每框
      `Stopped`；`stop_box` 单停
- [x] forwarder：结果逐条转 `multibox://result`；500ms 轮询
      `box_status()` 仅状态变化时发射 `multibox://status`
- [x] `open_result_window` 只显示/聚焦预声明 result 窗口，不新建
- [x] 热键语义按用户决策：Alt+Shift+R/S 控制单框实时；多框仅 UI 按钮
- [x] `multibox://result` 经 F1/F2 携带 `original_text`（OCR 清洗原文）；
      空文本/翻译失败降级为空串（前端清除残留）
- [x] 单框链路（capture_once / live / 热键）与设置、provider、托盘、悬浮球
      回归无变化
- [x] 未修改 vtrans-core 与其它 crate；README 与本文档同步

### 框选窗口隐藏/恢复验收（Bug-005）

- [x] 框选开始（`select_region` 进入）隐藏 main/result/floater 中当前可见
      窗口并记录「框选前可见集合」快照；已有未恢复快照时保留首次快照
- [x] 立即恢复：取消（`cancel_region_selection`）、超时、selector 不可用
      等失败路径调用 `restore_app_windows_immediately`
- [x] 延迟恢复：`capture_once` / `start_live_translation` /
      `add_translation_box` / `update_translation_box` 包装函数在成功与失败
      后均调用 `restore_app_windows_after_follow_up`；恢复后清空快照
- [x] `start_live_translation` 被「恢复/热键 R」路径调用时仅在有未恢复
      快照时才恢复，不影响正常启停（无快照 no-op）
- [x] 恢复规则：只恢复快照中窗口；floater 受 `floating_ball.enabled`
      约束；result/main 仅当快照可见才恢复（`restore_plan` 纯函数）
- [x] 决策与窗口 API 分离：纯状态机 `SelectionVisibilityState` +
      `restore_plan`（window_visibility.rs tests 覆盖五个要求场景）；窗口
      操作失败容忍并 `warn!`，不破坏框选主流程
- [x] selector/overlay 显隐逻辑保持不变；不新增 IPC 命令；窗口操作均为
      AppHandle 内部调用
- [x] 真实桌面显隐登记 README 手工验证项 15；回归：fmt / clippy / app
      单测 / workspace check 全绿；未修改其他 crate 与 vtrans-core

### 多框状态快照同步验收（Bug-004）

- [x] `start_multi_realtime` 在 `start_all` 成功后调用
      `set_current_mode(LiveRegion)`，多框会话运行期间 `get_app_status`
      报告 `mode: "live"`；`start_all` 失败（错误路径 `warn!`）不改 mode
- [x] `stop_multi_realtime` 完成停止后回退 `mode = single`；此刻单框
      live task 仍在运行/暂停中时保持 `LiveRegion` 不覆盖并发单框会话
      （决策集中在 `mode_after_multi_stop` 纯函数）
- [x] 编排与状态访问分离：`start_multi_session` / `stop_multi_session`
      泛化在 `MultiBoxSessionHost` 可注入接口上，AppState 仅做薄委托；
      窗口/任务相关副作用（overlay 显隐、状态事件、forwarder spawn）留在
      command 包装层
- [x] 单测覆盖四个场景（commands/state tests：`multi_start_records_*` /
      `multi_stop_falls_back_*` / `multi_stop_preserves_*` /
      `failed_multi_start_leaves_*` + `mode_after_multi_stop` 两个纯函数
      用例），用 mock host + 桩 provider 构建真实 `MultiBoxPipeline`，
      无 Windows 采集环境依赖
- [x] `AppStatus` serde 形状不变、无新增字段、无新增/修改 IPC 命令与
      事件（contracts.rs 无改动）
- [x] 回归：fmt / clippy / app 单测 / workspace check 全绿；未修改其他
      crate 与 vtrans-core；README 与本文档同步

### 窗口捕获排除验收（Bug-006）

- [x] 启动、窗口创建完成后（`init_app`）对 main/result/floater 调用
      `SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)`（0x11）；
      selector/overlay 不设置 WDA（selector 框选瞬间无捕获、overlay 描边
      已外移，模块 11 配合）
- [x] HWND 经 Tauri 2 `WebviewWindow::hwnd()` 获取；`windows` 0.61 依赖
      （与锁定 tauri 2.11.5 同版本线）仅启用 `Win32_Foundation` +
      `Win32_UI_WindowsAndMessaging`，仅在 vtrans-app 的 Cargo.toml 声明
- [x] 容错：窗口缺失/句柄失败/Win32 调用失败仅 `warn!` 记录 label（不
      记录窗口内容），不中断其余窗口、不影响启动；unsafe 块带 SAFETY 注释
- [x] 决策可测：`capture_exclusion_windows()` 纯函数单测锁定集合
      {main, result, floater}（不含 selector/overlay）；可注入设置器
      `apply_capture_exclusions` 单测覆盖「单窗失败不中断其余窗口」
- [x] 不新增 IPC 命令与事件；未修改 capture crate、vtrans-core 与其他
      crate；README（手工验证项 16 + 已知限制）与本文档同步
- [x] 回归：fmt / clippy / app 单测 / workspace check 全绿

### 发行部署验收（本任务：便携数据根 + 模型下载）

- [x] 数据根锚定 `{exe}/data`（`resolve_data_root` / `resolve_data_root_for`
      纯路径算术）；旧 `%APPDATA%\com.vtrans.app\config.json` 一次性迁移
      （`migrate_legacy_config`，仅目标缺失时复制）
- [x] 凭据本地化：`DpapiFileStore` 构造于 `data_root/credentials.bin`，
      首启执行 `migrate_windows_to_dpapi`；回退链 DPAPI → Windows
      Credential Manager → 内存存储，全部失败不阻断启动
- [x] 启动自愈：`ensure_data_models` 每次启动/重试从包内只读源修复必选
      文件，optional 条目（翻译模型）永不复制
- [x] 启动容错：模型问题不阻断启动（占位 OCR/翻译 provider + 状态快照）；
      `ocr_ready_gate` 使翻译入口在 OCR 未就位时返回明确错误
- [x] 5 个新命令注册进 `invoke_handler` 且均无参数：
      `download_translation_model` / `cancel_translation_model_download` /
      `delete_translation_model` / `get_model_status` / `retry_model_setup`
- [x] `model_download_progress` 事件 payload `{ bytes, total, fraction }`
      （snake_case，500ms/1MiB 节流，完成必发）；下载 .part 续传 +
      sha256 校验失败回滚 + 原子 rename 安装
- [x] 下载中切换 `local` 被后端守卫拒绝（`reject_local_switch_during_download`，
      与前端禁用选项双重保险）
- [x] 日志纪律：下载只记 URL 主机名；`{data}/logs/` 小时轮转（setup.rs）

## 开发注意事项

- AppState 使用 RwLock 保护可变状态
- Commands 通过 tauri::State 访问 AppState
- Pipeline 事件通过 app.emit 转发到前端
- 快捷键冲突时允许用户修改（配置中定义）
- 所有 Command 返回 Result<T, AppError>
- AppError 实现 Serialize 用于前端错误展示
- `ocr.language` 与 `translation.source_language` 是联动字段
  （vtrans-config `validate_language_linkage` 要求二者恒等）。
  `set_ocr_language` 与 `set_source_language` 各自同时写入两个字段，
  任一命令执行后两字段恒相等，`ConfigManager::save` 校验不会因联动不一致
  而拒绝。`set_target_language` 只写 `translation.target_language`。
- src-tauri/main.rs 只调用 vtrans-app::init_app，保持薄层
- 主窗口关闭默认隐藏到托盘而非退出；托盘是唯一恢复入口，托盘创建失败时
  应用启动失败而不是留下无法恢复的孤儿进程
- 合并顺序：`feat/10-app` 必须先于 `feat/11-frontend` 合并——模块 11 已调用
  `set_source_language` / `set_target_language`，若前端先合并，运行时会出现
  command 不存在错误。`set_api_key` / `get_app_config` 同理，前端
  `SettingsPanel` 已按 `{ apiKey }`（Tauri 2 默认 camelCase）参数名约定等待
  这两个命令落地；新增 command 不得添加 `rename_all = "snake_case"`，否则
  需同步修改前端 `src/services/tauri.ts` 与 `src/test/ipc.test.ts`。
- 多 Provider 前端同步（模块 11）：`AppStatus.translation_provider` 现在
  返回 `"openai"`/`"deepl"`/`"google"`/`"azure"`/`"baidu"`/`"local-onnx"`。
  前端需把 `normalizeProviderId` 中的 `"api" -> "api"` 映射改为
  `"openai" -> "openai"`（并删除 `"api"` 分支）、扩展 `ProviderId` 类型与
  凭据表单（baidu 需要 APP ID + Secret 两个输入，通过
  `set_provider_credentials` 提交）；`set_api_key` 仍按当前 provider 写
  对应目标，单 key 输入流程可保留。
