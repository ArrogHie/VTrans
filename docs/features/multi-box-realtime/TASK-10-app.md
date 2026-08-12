## 模块开发说明：10-app — 多框实时翻译增量

### AGENT_DEV_PROMPT 参数
- MODULE_NUMBER: 10
- MODULE_NAME: vtrans-app
- MODULE_SLUG: app
- CRATE_PATH: src-tauri
- SCOPE: app
- BRANCH_NAME: feat/multibox-app

### 功能上下文
- 功能目标：在应用层提供多框实时翻译的 Tauri Commands/Events、AppState 管理、overlay 多框渲染、结果窗口多框展示
- 本模块承担的部分：新增 Tauri 命令和事件、AppState 管理翻译框列表和 MultiBoxPipeline 生命周期、overlay 窗口渲染多个彩色方框、结果窗口推送多框结果
- 上游已提供：
  - vtrans-pipeline: MultiBoxPipeline、TranslationBox、BoxedTranslationResult、BoxStatus（阶段 B 完成后从 main 拉分支）
  - vtrans-config: TranslationBoxConfig、max_boxes、warning_threshold

### 任务要求
- 范围：仅限 src-tauri/src/；禁止修改其他 crate；禁止修改 vtrans-core
- 新增 Tauri Commands（需 #[tauri::command] 或项目现有命令宏，待确认实际用法）：
  - `add_translation_box(region: ScreenRegion) -> Result<TranslationBoxInfo, String>`：调用 pipeline.add_box，返回含 id 和颜色
  - `remove_translation_box(box_id: u32) -> Result<(), String>`：调用 pipeline.remove_box
  - `update_translation_box(box_id: u32, region: ScreenRegion) -> Result<(), String>`：调用 pipeline.update_box
  - `list_translation_boxes() -> Result<Vec<TranslationBoxInfo>, String>`：返回当前翻译框列表
  - `start_multi_realtime() -> Result<(), String>`：调用 pipeline.start_all
  - `stop_multi_realtime() -> Result<(), String>`：调用 pipeline.stop_all
- `stop_box(box_id: u32) -> Result<(), String>`：调用 pipeline.stop_box
  - `open_result_window() -> Result<(), String>`：打开翻译弹窗（如不存在则创建，已存在则 set_focus 置顶，不重复创建）
- 新增 Tauri Events（通过 app.emit 或 window.emit 推送，待确认现有 emit 方式）：
  - `multibox://result`：payload 为 BoxedTranslationResult 的 serde 序列化
  - `multibox://box-added`：payload 含 box_id、color、region
  - `multibox://box-removed`：payload 含 box_id
  - `multibox://box-updated`：payload 含 box_id、region
  - `multibox://status`：payload 含 box_id、status（Running/Stopped/Error）
  - `multibox://warning`：payload 含 current_count、max_count（超过 warning_threshold 时推送）
  - `translation://single-result`：单次翻译结果推送（替代在主页面显示结果），payload 含原文、译文
- AppState 变更：
  - 新增 `multi_pipeline: Option<MultiBoxPipeline>` 字段
  - 新增 `box_configs: Vec<TranslationBoxConfig>` 用于持久化（或直接从 config 加载）
  - 启动实时翻译时从 config 加载翻译框列表，初始化 MultiBoxPipeline
  - 结果流订阅：spawn 一个 task 从 pipeline.subscribe_results() 接收结果，通过 emit 推送前端
  - 状态变更：监听 pipeline 的 box 状态变更，通过 multibox://status 推送
  - 警告逻辑：add_translation_box 时检查 box_count >= warning_threshold，推送 multibox://warning
- Overlay 窗口：
  - 修改 overlay 窗口渲染逻辑，支持渲染多个彩色方框
  - 每个方框使用 TranslationBox 的 color 渲染边框
  - 方框可显示编号标签（box_id）
  - 删除框时从 overlay 移除对应方框
  - 修改框区域时更新 overlay 方框位置
- 结果窗口：
  - 修改结果窗口的事件监听，接收 multibox://result 和 translation://single-result 事件
  - 结果窗口（翻译弹窗）由 open_result_window 命令控制：不存在时创建，已存在时 set_focus
  - 前端负责具体布局（多框由上到下、彩色边框、分隔线），app 层只负责推送数据
- 主页面变更（用户补充需求）：
  - 删除主页面上的翻译结果显示（原文和译文不再在主页面展示）
  - 删除主页面上的翻译框坐标/大小/形状信息显示
  - 新增「打开翻译弹窗」按钮，调用 open_result_window 命令
  - 单次翻译结果通过 translation://single-result 事件推送到翻译弹窗（不在主页面显示）
  - 实时翻译结果通过 multibox://result 事件推送到翻译弹窗
- 约束：
  - 不修改 vtrans-core 的任何类型
  - 必须通过 pipeline 的 API 调用（不绕过 pipeline 直接操作 capture/ocr）
  - 图像不跨 IPC（结果只含文本和 box_id/color）
  - 错误归属：使用 AppError（本 crate 定义）
  - 翻译框过多警告不得阻塞用户操作（仅提示）
  - 敏感数据红线：emit 的结果中不包含图像；日志中原文/译文用 truncate_for_log
- 测试要求：
  - 命令测试：add/remove/update/list 正确调用 pipeline
  - 事件推送测试：结果和状态变更正确 emit
  - 警告逻辑测试：超过阈值时推送警告
  - AppState 并发安全测试
- 文档要求：API 变化同步 README；新增命令/事件需在文档中列出
- 提交规范：`feat(app): add multi-box translation commands and events`

### 横切标准提醒
- 日志：使用 tracing；命令调用记录 info（含 box_id，不含完整文本）；emit 记录 debug（含 event 名和 box_id）
- 错误：使用 thiserror 或现有 AppError；错误返回前端时不泄露内部细节
- 测试与风格：fmt/clippy 通过；无 todo!()/unimplemented!()
- 热键：不修改现有热键（Alt+Shift+A/R/S），多框使用现有 start/stop 热键触发

### 完成定义（DoD）
- [ ] cargo fmt --all -- --check 通过
- [ ] cargo clippy -p vtrans-app --all-targets 通过（或项目现有 clippy 配置）
- [ ] cargo test -p vtrans-app 通过（或项目现有 test 配置）
- [ ] 未修改其他 crate 与 vtrans-core
- [ ] PR 描述含实现说明、命令/事件清单、测试覆盖
- [ ] overlay 多框渲染在 cargo tauri dev 下可见
- [ ] 翻译弹窗 open_result_window 行为正确（创建/置顶不重复）
- [ ] 主页面不再显示翻译结果和坐标/大小信息

### 待确认事项
- 现有 Tauri 命令的宏用法（探测未找到 #[tauri::command]，可能使用 #[command] 或其他方式）
- 现有事件 emit 的方式（app.emit vs window.emit）
- AppState 的现有结构和方法
- overlay 窗口的现有渲染实现
- 结果窗口的现有事件监听方式
- src-tauri 的 Cargo.toml 中 crate 名称（vtrans-app 还是其他）

*** Add File: D:\~~~rust\VTrans\docs\features\multi-box-realtime\TASK-11-frontend.md
## 模块开发说明：11-frontend — 多框实时翻译增量

### AGENT_DEV_PROMPT 参数
- MODULE_NUMBER: 11
- MODULE_NAME: frontend
- MODULE_SLUG: frontend
- CRATE_PATH: src
- SCOPE: frontend
- BRANCH_NAME: feat/multibox-frontend

### 功能上下文
- 功能目标：提供多框翻译的前端 UI，包括翻译框列表管理、多色 overlay、多框结果展示、卡顿警告
- 本模块承担的部分：主页面翻译框列表组件、选区窗口多色支持、结果窗口多框分区展示、overlay 多框渲染数据、警告 UI
- 上游已提供：10-app 的 Tauri Commands 和 Events（阶段 C 先定义 IPC 契约后并行开发）

### 任务要求
- 范围：仅限 src/（前端代码）；禁止修改 Rust crate
- 新增/修改组件：
  - **翻译框列表组件（主页面）**：
    - 列表展示每个翻译框（颜色色块 + 编号 + 区域信息 + 删除/编辑按钮）
    - 「新增翻译框」按钮：触发选区窗口框选，选区完成后调用 add_translation_box
    - 「删除」按钮：调用 remove_translation_box
    - 「编辑区域」按钮：触发选区窗口重新框选，完成后调用 update_translation_box
    - 列表为空时显示引导提示
  - **警告提示**：
    - 监听 multibox://warning 事件，显示 toast/通知
    - 翻译框数量超过 warning_threshold 时在列表顶部显示持久警告条
    - 警告文案：「翻译框过多可能导致卡顿，建议不超过 N 个」
  - **结果窗口多框展示**：
    - 监听 multibox://result 事件
    - 按 box_id 分区展示，每区头部显示颜色色块 + 编号
    - 每区显示原文和译文，可折叠/展开
    - 框删除时对应区域从结果窗口移除
    - 框停止时对应区域显示「已停止」状态
  - **Overlay 窗口数据**：
    - 监听 multibox://box-added/removed/updated 事件
    - 向 overlay 窗口传递翻译框列表（颜色、区域、编号）
    - overlay 窗口渲染多个彩色方框（如 overlay 为 Rust 侧渲染则 app 层处理）
  - **状态指示**：
    - 监听 multibox://status 事件
    - 列表中每个框显示运行/停止/错误状态
  - **启动/停止控制**：
    - 调用 start_multi_realtime / stop_multi_realtime
    - 启动后列表中每个框显示运行状态
    - 支持单个框停止（stop_box）
- IPC 调用：
  - 调用 invoke("add_translation_box", { region }) 等 Tauri 命令
  - 监听 listen("multibox://result", callback) 等事件
  - TypeScript 类型定义与 Rust 侧 serde 表示一一对应（见 PLAN.md IPC 契约部分）
- 约束：
  - 不修改 Rust crate
  - invoke 调用需处理错误（try-catch，显示错误 toast）
  - 事件监听需在组件卸载时清理（unlisten）
  - 颜色展示与 Rust 侧 color hex 值一致
  - 不在 UI 文本中暴露 API Key、Bearer Token 等敏感信息
- 测试要求：
  - 组件单元测试（列表渲染、增删改交互）
  - 事件监听测试（模拟 multibox://result 推送）
  - 警告逻辑测试（超过阈值显示警告）
- 文档要求：无特殊要求（前端无 README）
- 提交规范：`feat(frontend): add multi-box translation UI`

### 横切标准提醒
- 无日志要求（前端不写 Rust 日志）
- 错误处理：invoke 失败时显示 toast/通知，不崩溃
- 测试与风格：pnpm test 通过；tsc --noEmit 通过；无 console.log 残留

### 完成定义（DoD）
- [ ] pnpm test 通过
- [ ] pnpm exec tsc --noEmit 通过
- [ ] 未修改 Rust crate
- [ ] PR 描述含组件说明、IPC 调用清单、测试覆盖
- [ ] 翻译框列表在 cargo tauri dev 下可交互
- [ ] 多框结果在结果窗口中可见
- [ ] 警告在超过阈值时显示

### 待确认事项
- 现有前端组件结构和状态管理方式（React Context/Zustand/其他）
- 现有 invoke/listen 的封装方式（是否有统一 API 层）
- 现有选区窗口的实现和交互流程
- 现有结果窗口的组件结构
- overlay 窗口的渲染方式（React 还是 Rust 侧绘制）
- 前端是否有 normalizeProviderId 等映射逻辑需同步
