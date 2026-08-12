# 功能开发计划：多框实时翻译（Multi-Box Realtime Translation）

## 概述
- 需求来源：用户提出新功能需求——多框实时翻译
- 功能目标：在实时翻译模式下支持多个矩形框同时进行翻译，每个框独立采集、OCR、翻译并展示结果，用不同颜色区分
- 使用场景：用户在主页面「翻译框」列表中新增/删除/修改翻译框（矩形区域），启动实时翻译后所有框同时工作；单次翻译仍仅支持单框
- 优先级：P1
- 状态：待拆解

## 验收标准（用户可验证）
- [ ] 主页面有「翻译框」列表，可新增、删除、修改每个框的区域
- [ ] 新增翻译框时，通过选区窗口框选区域，系统自动分配颜色
- [ ] 每个翻译框用不同颜色区分（overlay 窗口渲染对应颜色的方框）
- [ ] 启动实时翻译后，所有翻译框同时采集并翻译
- [ ] 翻译弹窗内多框结果由上到下依次排列，框间用分隔线隔开
- [ ] 每个框的翻译内容用与翻译框相同颜色的边框包含，便于区分
- [ ] 翻译框过多时（超过阈值）提示用户可能导致卡顿
- [ ] 单次翻译仍为单框，但结果也通过翻译弹窗展示（不在主页面显示）
- [ ] 可单独停止某个翻译框，也可一键停止全部
- [ ] 修改翻译框区域后实时生效（无需重启实时翻译）
- [ ] 删除翻译框后该框的翻译结果从结果窗口移除
- [ ] 主页面不显示翻译结果（原文和译文），改为显示「打开翻译弹窗」按钮
- [ ] 点击「打开翻译弹窗」按钮弹出翻译弹窗；如弹窗已存在则仅置顶，不重复弹出
- [ ] 主页面不再显示翻译框的坐标、大小、形状信息（该信息仅用于内部配置）
- [ ] 单次翻译和实时翻译均使用翻译弹窗展示结果

## 涉及模块与顺序

| 序号 | 模块 | 任务类型 | 依赖 | 建议分支 | 状态 |
|------|------|----------|------|----------|------|
| 1 | 02-config | 修改 | — | feat/multibox-config | 待分配 |
| 2 | 06-text | 修改（条件性） | — | feat/multibox-text | 待分配 |
| 3 | 09-pipeline | 新增+修改 | 依赖 1, 2 | feat/multibox-pipeline | 待分配 |
| 4 | 10-app | 新增+修改 | 依赖 3 | feat/multibox-app | 待分配 |
| 5 | 11-frontend | 新增+修改 | 依赖 4（IPC 契约） | feat/multibox-frontend | 待分配 |

### 阶段安排
- **阶段 A（并行）**：02-config + 06-text（层级 1，互不依赖）
- **阶段 B**：09-pipeline（层级 3，依赖 config 和 text 的上游 API）
- **阶段 C**：10-app + 11-frontend（层级 4，先定义 IPC 契约再并行开发）

## 契约变更

### 冻结契约（vtrans-core）
- **不涉及**。本功能不修改 vtrans-core 的任何类型、trait 或 serde 表示。
- **设计决策**：`TranslationBox`（包含 `id`、`region: ScreenRegion`、`color`）定义在 `vtrans-pipeline`（层级 3），不引入 core 新类型。`ScreenRegion` 保持现有字段（x, y, width, height）不变。
- `BoxId` 使用 `u32`，在 pipeline 层定义；跨 IPC 传输时作为普通数字序列化。
- OCR/翻译结果与框的关联在 pipeline 层完成：pipeline 输出 (BoxId, TranslationResult) 配对，core 类型不变。

### IPC 契约（10-app 与 11-frontend）
新增以下 Tauri Commands 和 Events（两端一起改，先 app 后 frontend）：

**Commands（Rust 侧定义，前端调用）：**
- `add_translation_box(region) -> TranslationBoxInfo`：新增翻译框，返回含 id 和分配颜色
- `remove_translation_box(box_id) -> ()`：删除指定翻译框
- `update_translation_box(box_id, region) -> ()`：修改翻译框区域
- `list_translation_boxes() -> Vec<TranslationBoxInfo>`：列出所有翻译框
- `start_multi_realtime() -> ()`：启动多框实时翻译
- `stop_multi_realtime() -> ()`：停止所有多框实时翻译
- `stop_box(box_id) -> ()`：停止单个翻译框

**Events（Rust 侧推送，前端监听）：**
- `multibox://result`：单框翻译结果，payload 含 box_id、color、原文、译文
- `multibox://box-added`：翻译框新增通知
- `multibox://box-removed`：翻译框删除通知
- `multibox://box-updated`：翻译框更新通知
- `multibox://status`：翻译框状态变更（运行/停止/错误）
- `multibox://warning`：翻译框过多警告

**TypeScript 类型（前端定义，与 Rust 侧 serde 表示一一对应）：**
```typescript
interface TranslationBoxInfo {
  box_id: number;
  region: { x: number; y: number; width: number; height: number };
  color: string; // hex color, e.g. "#FF6B6B"
}

interface MultiBoxResult {
  box_id: number;
  color: string;
  original_text: string;
  translated_text: string;
  timestamp: number;
}
```

### 配置变更
- AppConfig 新增 `translation_boxes: Vec<TranslationBoxConfig>` 字段（含 id、region、color）
- 新增 `max_boxes: u32`（默认 8）、`warning_threshold: u32`（默认 4）
- 需编写迁移逻辑（从无多框配置的旧版本迁移）

## 多线程决策

**方案：多线程（Tokio 异步任务）**（用户已确认接受）

理由：
1. 现有架构基于 Tokio，pipeline 已使用 async/await
2. core 类型和 Provider trait 均 Send + Sync，天然支持跨线程
3. 每个翻译框作为独立 Tokio task，拥有独立 CaptureSession 和 CancellationToken
4. 结果通过 mpsc channel 汇集到 app 层，再通过 Tauri Event 推送前端
5. 改动量可控：pipeline 新增多框管理逻辑，不改变单框 pipeline 核心流程

## 风险与假设

### 假设
- 用户已确认：P1 优先级、多线程方案、max_boxes=8/warning_threshold=4、复用现有 Alt+Shift+R/S 热键
- PipelineMode 现有 2 个变体（探测确认），分别对应单次翻译和实时翻译。多框在实时模式下扩展，不新增变体。
- AppConfig 现有字段不含多框列表（探测确认未找到 region 相关字段名）。
- 现有结果窗口和 overlay 窗口为单框设计，需修改为支持多框。
- CaptureSource/CaptureSession trait 可为每个翻译框独立创建实例。

### 风险
- 性能风险：多框同时采集+OCR+翻译可能消耗大量资源。设最大框数限制（默认8），超阈值（默认4）时警告。
- 采集冲突风险：多个 CaptureSession 同时运行 Graphics Capture API 可能存在资源竞争。若不可行则降级为单线程轮询。
- 前端渲染风险：多个 overlay 方框同时渲染可能影响 UI 响应性。
- 结果窗口布局风险：多框结果分区展示可能空间不足。

### 已知限制排除
- 修改快捷键需重启生效：多框不涉及快捷键修改
- 本地模型仅 en -> zh-CN：多框使用相同限制
- 大图像不跨 IPC：多框仅传文本和缩略图

## 主页面与翻译弹窗设计（用户补充需求）

### 主页面变更
- 删除主页面上的翻译结果显示（原文和译文均不在主页面展示）
- 删除主页面上的翻译框坐标/大小/形状信息显示
- 新增「打开翻译弹窗」按钮，点击后弹出翻译弹窗窗口
- 弹窗已存在时仅置顶（set_focus），不重复创建
- 单次翻译和实时翻译均适用此变更

### 翻译弹窗（结果窗口）布局
- 多框模式：结果由上到下依次排列
  - 框 1 翻译内容（用框 1 颜色的边框包含）
  - 分隔线
  - 框 2 翻译内容（用框 2 颜色的边框包含）
  - 分隔线
  - ...
  - 支持滚动
- 单框模式（单次翻译）：弹窗内显示单框结果，无边框分区
- 每个框的区域内显示：原文、译文、框编号/颜色标识
- 框删除时对应区域从弹窗移除
- 框停止时对应区域显示「已停止」状态

### IPC 契约补充
- 新增 Command：`open_result_window() -> ()`：打开/置顶翻译弹窗（如不存在则创建，已存在则 set_focus）
- 新增 Event：`translation://single-result`：单次翻译结果推送（替代在主页面显示）
## 环境说明

本计划基于项目架构文档和代码探测结果编写。因沙箱环境限制，部分详细文档未能直接读取（docs/modules/NN-*.md 等），标注「待补充」处需开发 Agent 对照源码核实。

### 探测确认的代码事实
- ScreenRegion 为 struct，字段 x/y/width/height，无 id 无 color
- PipelineMode 为 enum，2 个变体（变体名待确认）
- PipelineStatus 为 enum，至少有 Idle 变体
- CaptureSource/CaptureSession trait 存在于 vtrans-core
- AppConfig struct 存在于 vtrans-config，有若干字段
- Pipeline struct 存在于 vtrans-pipeline，有若干公开函数
- CapturedImage struct 存在于 vtrans-core
