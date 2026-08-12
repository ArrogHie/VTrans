## 模块开发说明：11-frontend — 多框实时翻译增量

### AGENT_DEV_PROMPT 参数
- MODULE_NUMBER: 11
- MODULE_NAME: frontend
- MODULE_SLUG: frontend
- CRATE_PATH: src
- SCOPE: frontend
- BRANCH_NAME: feat/multibox-frontend

### 功能上下文
- 功能目标：提供多框翻译的前端 UI，包括翻译框列表管理、翻译弹窗布局、卡顿警告、主页面精简
- 本模块承担的部分：主页面翻译框列表组件（精简版）、翻译弹窗布局（多框由上到下+彩色边框+分隔线）、主页面变更（删除结果显示和坐标显示、新增弹窗按钮）
- 上游已提供：10-app 的 Tauri Commands 和 Events（阶段 C 先定义 IPC 契约后并行开发）

### 任务要求
- 范围：仅限 src/（前端代码）；禁止修改 Rust crate
- 新增/修改组件：
  - 翻译框列表组件（主页面）：
    - 列表展示每个翻译框（颜色色块+编号+删除/编辑按钮）
    - 注意：不在列表中显示坐标/大小/形状信息（用户要求删除）
    - 新增翻译框按钮：触发选区窗口框选，完成后调用 invoke("add_translation_box", { region })
    - 删除按钮：调用 invoke("remove_translation_box", { box_id })
    - 编辑区域按钮：触发选区窗口重新框选，完成后调用 invoke("update_translation_box", { box_id, region })
    - 列表为空时显示引导提示
  - 主页面变更（用户补充需求）：
    - 删除主页面上的翻译结果显示（原文和译文均不在主页面展示）
    - 删除主页面上的翻译框坐标/大小/形状信息显示
    - 新增「打开翻译弹窗」按钮，点击调用 invoke("open_result_window")
    - 弹窗已存在时仅置顶（由后端处理，前端只调用命令）
    - 单次翻译和实时翻译均使用翻译弹窗展示结果
  - 翻译弹窗（结果窗口）布局：
    - 多框模式：结果由上到下依次排列
      - 框 1 翻译内容（用框 1 颜色的 border 包含，如 border: 2px solid #FF6B6B）
      - 分隔线（hr 或带样式的 div）
      - 框 2 翻译内容（用框 2 颜色的 border 包含）
      - 分隔线
      - ... 支持滚动（overflow-y: auto）
    - 单框模式（单次翻译）：弹窗内显示单框结果，无边框分区
    - 每个框区域内显示：原文、译文、框编号/颜色标识
    - 框删除时对应区域从弹窗移除
    - 框停止时对应区域显示已停止状态
    - 弹窗标题栏显示当前翻译状态（运行中/已停止）
  - 警告提示：
    - 监听 listen("multibox://warning") 事件，显示 toast/通知
    - 超过 warning_threshold 时在列表顶部显示持久警告条
    - 警告文案：翻译框过多可能导致卡顿，建议不超过 N 个
  - Overlay 窗口数据：
    - 监听 listen("multibox://box-added") 等事件
    - 向 overlay 窗口传递翻译框列表（颜色、区域、编号）
  - 状态指示：
    - 监听 listen("multibox://status") 事件
    - 列表中每个框显示运行/停止/错误状态
  - 启动/停止控制：
    - 调用 invoke("start_multi_realtime") / invoke("stop_multi_realtime")
    - 支持单个框停止 invoke("stop_box", { box_id })
- IPC 调用：
  - 调用 invoke("add_translation_box", { region }) 等命令
  - 调用 invoke("open_result_window") 打开/置顶翻译弹窗
  - 监听 listen("multibox://result") 多框实时结果
  - 监听 listen("translation://single-result") 单次翻译结果
  - 监听 listen("multibox://warning") 警告
  - TypeScript 类型定义与 Rust 侧 serde 表示一一对应
- 约束：
  - 不修改 Rust crate
  - invoke 调用需处理错误（try-catch，显示错误 toast）
  - 事件监听需在组件卸载时清理（unlisten）
  - 颜色展示与 Rust 侧 color hex 值一致
  - 不在 UI 文本中暴露 API Key 等敏感信息
  - 翻译弹窗中每个框区域的边框颜色必须与该框的 color hex 值一致
- 测试要求：
  - 组件单元测试（列表渲染、增删改交互）
  - 事件监听测试（模拟 multibox://result 推送）
  - 警告逻辑测试（超过阈值显示警告）
  - 弹窗布局测试（多框时由上到下排列、彩色边框、分隔线）
- 文档要求：无特殊要求（前端无 README）
- 提交规范：feat(frontend): add multi-box translation UI with popup layout

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
- [ ] 多框结果在翻译弹窗中可见（由上到下、彩色边框、分隔线）
- [ ] 警告在超过阈值时显示
- [ ] 主页面不显示翻译结果和坐标/大小信息
- [ ] 「打开翻译弹窗」按钮工作正常（创建/置顶不重复）
- [ ] 单次翻译结果也在翻译弹窗中显示

### 待确认事项
- 现有前端组件结构和状态管理方式（React Context/Zustand/其他）
- 现有 invoke/listen 的封装方式（是否有统一 API 层）
- 现有选区窗口的实现和交互流程
- 现有结果窗口的组件结构（需改为翻译弹窗）
- overlay 窗口的渲染方式（React 还是 Rust 侧绘制）
- 前端是否有 normalizeProviderId 等映射逻辑需同步
- 现有主页面是否有翻译结果显示组件需删除
- 现有主页面是否有坐标/大小显示组件需删除
