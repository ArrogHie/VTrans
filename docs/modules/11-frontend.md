# 模块 11：frontend 前端 UI

| 属性 | 值 |
|------|-----|
| 路径 | `src/` |
| 分支 | `feat/11-frontend` |
| 上游依赖 | `vtrans-app` (通过 Tauri IPC) |
| 层级 | 4 |
| 复杂度 | 中 |
| 阶段 | Phase 4 |

## 职责

实现五个 Tauri 窗口的 React UI：主窗口（控制面板）、选区窗口（透明全屏选区）、结果窗口（翻译迷你条）、常驻选区方框窗口与悬浮球。管理前端状态、通过 IPC 调用 Rust 后端、监听后端事件。

## 技术选型

- React 18 + TypeScript + Vite
- 状态管理：Zustand（轻量，适合 Tauri 应用）
- 样式：Tailwind CSS（工具优先，快速迭代）
- 图标：Lucide React
- Tauri IPC：@tauri-apps/api v2

## 五窗口设计

### 主窗口 (MainWindow.tsx)

控制面板，包含：
- 模式切换：单次翻译 / 实时翻译
- 操作按钮：选择区域、开始、暂停、停止
- 识别语言选择：自动、日语、英语、简体中文（同时作为 OCR 识别语言与翻译源语言，二者经后端联动恒相等）
- 目标语言选择：中文、日语、英语
- 翻译引擎切换（`ProviderSelect`）：OpenAI / DeepL / Google / Azure / 百度 /
  本地（切换即经 `set_translation_provider` 保存；`AppStatus.translation_provider`
  返回的运行时 id 经 `normalizeProviderId` 映射到前端配置标识符域）。
  **本地引擎禁用联动**：`localProviderBlockReason` 由 `get_model_status` +
  下载在途标记推导——翻译模型缺失/校验失败/下载中时**仅禁用 local 选项**
  （云端选项不受影响）并显示原因提示（「请先在设置中下载本地翻译模型」等），
  下载中提示始终可见；后端切换守卫是第二道保险
- 当前状态与简要错误信息
- 设置入口
- 模型就位错误横幅（`ModelSetupBanner`，R6）：`get_model_status` 报告
  `ocr_ready === false` 或存在非 optional 条目 `invalid` 时显示「OCR 模型未
  就位，翻译功能不可用」+「重试」按钮；重试走 `retry_model_setup`，刷新后
  状态健康则自动消失；横幅不阻断主窗口其余功能（optional 条目不触发横幅）

### 选区窗口 (RegionSelector.tsx)

透明、无边框、置顶的 Tauri 窗口：
- 覆盖目标显示器或虚拟桌面
- 鼠标拖动生成矩形区域
- 显示边框和尺寸标注
- Esc 取消，Enter 确认
- 返回 monitor_id + x + y + width + height 到 Rust 侧
- 选区期间暂停当前实时任务

### 结果窗口 (ResultWindow.tsx)

- 迷你条形态：译文为主体，原文默认收起、点击展开
- 工具栏一行图标：置顶、暂停/继续（实时）、重新翻译（单次）、外观、关闭
- 无原生标题栏（`decorations: false` 由 vtrans-app 声明）：整个顶栏是拖动区域，
  关闭按钮有独立危险色悬停，迷你条保持圆角壳
- 外观控制：背景透明度（0.3–1.0）与字号（12–24px），CSS 变量即时生效并经
  `update_result_window_appearance` 持久化（不再整包 `save_settings`，
  实时会话运行期间也可保存）
- 字号由 `--result-font-size` 变量统一控制：译文/原文不再携带 `text-sm`/`text-xs`
  覆盖类，仅保留 `leading-*` 行距
- 启动水合：挂载时自行 `get_app_config` 应用持久化外观（各 WebView store 隔离，
  不依赖主窗口）；窗口存活期间 store 配置变化时同步重新应用
- 可置顶、可拖动、可缩放；`transparent: true` 由 vtrans-app 声明，半透明用 CSS 背景 alpha 实现

### 悬浮球 (FloatingBall.tsx)

- 默认关闭，启动按 `floating_ball.enabled` 显示；监听 `frontend_floater_enabled` 即时显隐
- 小圆球可拖动（`data-tauri-drag-region`），位置记忆到 localStorage 并 clamp 到可用显示器
- 点击展开紧凑菜单：框选翻译、实时翻译启停、暂停·继续、打开主窗口
- 外观小控制（透明度 0.3–1.0、直径 32–72px）：菜单内滑杆即时生效并经
  `update_floating_ball_appearance` 持久化；球背景 alpha 与直径由
  `--floater-opacity` / `--floater-size` CSS 变量驱动，折叠窗口尺寸、位置 clamp
  与展开菜单高度随直径动态适配
- 容器 `overflow-hidden`：展开/收起均无滚动条
- 复用 `services/translateActions.ts` 状态机，与主窗口共用、禁止复制

### 设置面板 (SettingsPanel.tsx)

- 悬浮球开关即时生效（`frontend_floater_enabled` 纯前端事件）
- 悬浮球透明度/大小滑块：即时保存（`update_floating_ball_appearance`），
  不走整包 `save_settings`；范围校验与后端一致
- 翻译引擎表单按 provider 条件渲染：
  - OpenAI：API 端点 + 必填模型名
  - DeepL：Free / Pro / 自定义端点三档选择
  - Google：API 端点 + 可选模型名
  - Azure：API 端点 + 区域（`region`，可空）
  - 百度：APP ID（随 `save_settings` 保存到配置）+ Secret（经
    `set_provider_credentials` 写入系统凭据，不落配置）
  - 本地：隐藏全部云端字段，提示本地模型不受云端参数影响
- 凭据保存：非百度 provider 用 `set_provider_credentials(providerId, { apiKey })`
  并显式携带草稿 provider id；百度用 `{ appId, secret }` 双字段
- 校验规则与 `vtrans-config` 对齐：本地忽略端点/模型/区域；远程端点必须
  http(s)；OpenAI 模型名必填；Azure 区域非空即校验；百度 APP ID 必填
- 「本地翻译模型」卡片（`ModelDownloadCard`）：
  - 挂载时经 `get_model_status` 水合条目状态（`ready` 已安装 / `missing`
    未安装 / `invalid` 校验失败），下载进度经 `model_download_progress`
    事件实时更新（进度条百分比 + role=progressbar）
  - 状态与下载标记存于 Zustand store：**设置面板关闭不中断后端下载**，
    重新挂载按最新状态与持续推送的进度事件水合；前端不保存任何模型内容
  - 按钮按状态切换：下载/重新下载（invalid）/ 删除（ready，二次确认）/
    取消下载（下载中）/ 刷新（状态未知）；取消/删除后重新拉取终态，
    用户主动取消的错误不作为失败展示

## 公开 API（前端内部）

### Services (services/)

```typescript
export async function startRegionSelection(): Promise<ScreenRegion>;
export async function captureOnce(region: ScreenRegion): Promise<OcrResult>;
export async function startLiveTranslation(config: PipelineConfig): Promise<void>;
export async function stopLiveTranslation(): Promise<void>;
export async function saveSettings(settings: AppConfig): Promise<void>;
export async function updateResultWindowAppearance(opacity: number, fontSizePx: number): Promise<void>;
export async function updateFloatingBallAppearance(opacity: number, sizePx: number): Promise<void>;
export async function getAppStatus(): Promise<AppStatus>;
export function setProviderCredentials(
  providerId: ProviderId,
  credentials: { apiKey?: string; appId?: string; secret?: string },
): Promise<void>;

// translateActions.ts：主窗口与悬浮球共用的翻译状态机
export function selectAndTranslateOnce(): Promise<TranslateActionResult>;
export function selectRegionForLive(): Promise<TranslateActionResult>;
export function startLive(): Promise<TranslateActionResult>;
export function toggleLivePause(): Promise<TranslateActionResult>;
export function stopLive(): Promise<TranslateActionResult>;
export function toggleLiveFromFloater(): Promise<TranslateActionResult>;

// resultAppearance.ts：迷你条外观（CSS 变量，无窗口 opacity API）
export function applyResultAppearance(root, opacity, fontSizePx): void;
export function persistResultAppearance(opacity, fontSizePx): Promise<void>;
export function applyHydratedAppearance(config, root?): { opacity; fontSizePx };

// floaterAppearance.ts：悬浮球外观（CSS 变量，无窗口 opacity API）
export function clampFloaterOpacity(value: number): number;
export function clampFloaterSizePx(value: number): number;
export function applyFloaterAppearance(root, opacity, sizePx): void;
export function persistFloaterAppearance(opacity, sizePx): Promise<void>;

// tauri.ts：翻译模型下载/状态 IPC（5 个命令均无参数）
export function downloadTranslationModel(): Promise<void>;
export function cancelTranslationModelDownload(): Promise<void>;
export function deleteTranslationModel(): Promise<void>;
export function getModelStatus(): Promise<ModelStatusReport>;
export function retryModelSetup(): Promise<ModelStatusReport>;

// modelActions.ts：下载/状态动作（设置卡片与主窗口共用，终态进 Zustand store）
export function refreshModelStatus(): Promise<ModelStatusReport>;
export function applyModelDownloadProgress(progress: ModelDownloadProgress): void;
export function downloadModel(): Promise<ModelStatusReport>;
export function cancelModelDownload(): Promise<ModelStatusReport>;
export function deleteModel(): Promise<ModelStatusReport>;
export function retryModelSetup(): Promise<ModelStatusReport>;

// events.ts：下载进度监听
export const MODEL_DOWNLOAD_PROGRESS = "model_download_progress";
export function onModelDownloadProgress(
  callback: (progress: ModelDownloadProgress) => void,
): Promise<Unlisten>;

export function onOcrCompleted(cb: (result: OcrResult) => void): Unlisten;
export function onTranslationCompleted(cb: (result: TranslationResult) => void): Unlisten;
export function onPipelineError(cb: (msg: string) => void): Unlisten;
```

浮球显隐事件为纯前端内部事件 `frontend_floater_enabled`（payload `{ enabled }`），
主窗口设置面板切换开关时 `emit`，不经过 Rust。

### 类型与映射 (types/)

```typescript
export type ProviderId = 'openai' | 'deepl' | 'google' | 'azure' | 'baidu' | 'local';
export type TranslationQuality = 'fast' | 'balanced';

export interface TranslationConfig {
  provider: ProviderId;
  region: string | null;  // Azure Translator 区域，仅 azure provider 使用
  app_id: string | null;  // 百度 APP ID（Secret 只存系统凭据）
  quality: TranslationQuality;
  // ...其余字段不变
}

// local-onnx -> local；openai/deepl/google/azure/baidu 原样透传；
// 未知值（含已废弃的 "api"）回退默认 provider "openai"
export function normalizeProviderId(raw: string): ProviderId;

// ── 模型下载/状态（发行部署，字段与 Rust DTO 一一对应，snake_case）──
export type ModelState = 'ready' | 'missing' | 'invalid';
export interface ModelEntryStatus {
  id: string;
  state: ModelState;
  optional: boolean;   // optional 条目缺失是「未安装」（missing），非校验失败
}
export interface ModelStatusReport {
  entries: ModelEntryStatus[];
  ocr_ready: boolean;
  translation_ready: boolean;
}
export interface ModelDownloadProgress {
  bytes: number;
  total: number;
  fraction: number;    // [0,1]，total 未知时为 0
}

export const TRANSLATION_MODEL_ENTRY_ID = "opus-mt-en-zh-int8"; // 与 manifest translation.model.id 一致
export function findTranslationModelEntry(report): ModelEntryStatus | null; // 精确 id → 首个 optional 兜底
export function hasModelSetupProblems(report: ModelStatusReport): boolean;   // ocr_ready false 或非 optional 条目 invalid（R6 横幅条件）
export type LocalProviderBlockReason = 'missing' | 'invalid' | 'downloading';
export function localProviderBlockReason(
  report: ModelStatusReport | null,
  downloading: boolean,
): LocalProviderBlockReason | null; // null = local 可选；null 报告不阻断（防闪烁）
```

`AppStatus.translation_provider` 由 `vtrans-app` 返回 provider 运行时实现 id，
云端 provider 与其配置 id 相同，仅本地 ONNX provider 报告 `"local-onnx"`，因此
前端只做这一个映射，其余原样透传。`TranslationConfig` 的 `region` / `app_id`
随整包 `save_settings` 持久化；Secret 与 API Key 一律不进入前端 store、配置、
事件或日志。模型下载进度仅进内存 store，模型内容不进入前端任何状态。

### Stores (stores/)

```typescript
interface AppState {
  mode: 'single' | 'live';
  status: PipelineStatus;
  ocrResult: OcrResult | null;
  translationResult: TranslationResult | null;
  error: string | null;
  setMode: (mode: 'single' | 'live') => void;
  setStatus: (status: PipelineStatus) => void;
  // ── 模型下载/状态（发行部署）──
  modelStatus: ModelStatusReport | null;             // get_model_status / retry 快照
  modelDownloadProgress: ModelDownloadProgress | null; // 最近一次下载进度事件
  translationModelDownloading: boolean;              // 下载在途标记（store 常驻，关闭面板不中断）
  setModelStatus: (report: ModelStatusReport) => void;
  setModelDownloadProgress: (progress: ModelDownloadProgress | null) => void;
  setTranslationModelDownloading: (downloading: boolean) => void;
}
```

## 内部文件结构

```text
src/
  main.tsx                  # React 入口
  App.tsx                   # 路由到不同窗口
  windows/
    MainWindow.tsx
    ResultWindow.tsx
    RegionSelector.tsx
    OverlayWindow.tsx
    FloatingBall.tsx
  components/
    ModeToggle.tsx
    LanguageSelector.tsx
    ProviderToggle.tsx
    ProviderSelect.tsx       # 主窗口引擎下拉（local 禁用联动）
    ModelSetupBanner.tsx     # 模型未就位错误横幅 + 重试（R6）
    ModelDownloadCard.tsx    # 设置面板「本地翻译模型」下载卡片
    SettingsPanel.tsx
    DebugPanel.tsx
    ErrorBanner.tsx
    StatusBar.tsx
    ResultCard.tsx
    MultiBoxResults.tsx
    TranslationBoxList.tsx
  hooks/
    useDebugFrame.ts
  stores/
    appStore.ts
  services/
    tauri.ts               # IPC 调用封装
    events.ts              # 事件监听
    translateActions.ts    # 主窗口与悬浮球共用翻译状态机
    resultAppearance.ts    # 迷你条外观应用与持久化
    floaterAppearance.ts   # 悬浮球外观应用与持久化
    modelActions.ts        # 模型下载/状态动作（设置卡片与主窗口共用）
  utils/
    floaterPosition.ts     # 悬浮球位置 clamp 与 localStorage 记忆
    floaterVisibility.ts   # 悬浮球显隐
  types/
    index.ts               # TS 类型定义
```

## 测试计划

| 测试项 | 类型 | 说明 |
|--------|------|------|
| Store 状态变更 | 单元 | setMode/update 正确更新 |
| IPC 调用封装 | 单元 | mock invoke，验证参数 |
| 事件监听 | 单元 | mock listen，验证回调 |
| 迷你条外观 | 单元 | clamp、CSS 变量应用、`update_result_window_appearance` 持久化（mock invoke） |
| 迷你条字体变量 | 单元 | SSR 断言译文/原文无 `text-sm`/`text-xs` 覆盖类，字号由 CSS 变量控制 |
| 无边框迷你条 | 单元 | 顶栏拖动区域、关闭按钮样式、圆角壳 |
| 悬浮球外观 | 单元 | clamp、CSS 变量应用、`update_floating_ball_appearance` 持久化（mock invoke） |
| 悬浮球滚动条 | 单元 | 容器 `overflow-hidden`，展开菜单渲染外观滑杆 |
| 悬浮球动作 | 单元 | mock IPC，覆盖框选翻译/实时启停/暂停继续/停止 |
| 浮球显隐 | 单元 | `frontend_floater_enabled` 事件链路与窗口 show/hide |
| 浮球位置 | 单元 | clamp 到显示器、localStorage 记忆往返 |
| 窗口路由 | 单元 | `floater` label 渲染 FloatingBall |
| 设置范围校验 | 单元 | 弹窗透明度 0.3–1.0、字号 12–24 整数；浮球透明度 0.3–1.0、直径 32–72 整数 |
| Provider 映射 | 单元 | `normalizeProviderId`：`local-onnx -> local`、五云端 id 透传、未知/废弃 `api` 回退 `openai` |
| Provider 切换表单 | 单元 | 设置面板按 provider 条件渲染端点/模型/区域/APP ID/Secret 字段；本地隐藏云端字段 |
| 凭据 IPC | 单元 | `set_provider_credentials` 只发送提供的字段；百度 `appId` + `secret` 双字段载荷 |
| ProviderToggle | 单元 | 六个选项、标签、选中态与 `aria-pressed` |
| 本地引擎禁用联动 | 单元 | `localProviderBlockReason`：下载中 / 模型缺失 / 校验失败 → 仅 local 选项禁用 + 提示；云端不受影响；null 报告不阻断 |
| 模型状态水合 | 单元 | `refreshModelStatus` 镜像快照进 store；翻译模型 `ready` 时清除在途标记与陈旧进度 |
| 下载进度事件 | 单元 | `onModelDownloadProgress` 监听 `model_download_progress`；`applyModelDownloadProgress` 镜像进 store（模型已 ready 的迟到事件不改标记） |
| 下载卡片 | 单元 | 按状态渲染下载/重新下载/删除（二次确认）/取消/刷新按钮；百分比取 `fraction` clamp 到 [0,1] |
| 模型错误横幅 | 单元 | `hasModelSetupProblems`：`ocr_ready === false` 或非 optional 条目 invalid 显示；optional 条目不触发；重试按钮回调 |
| 选区交互 | 手动 | 拖动生成矩形，Esc 取消，Enter 确认 |
| 结果展示 | 手动 | OCR/翻译结果正确显示 |
| 状态同步 | 手动 | 后端事件正确更新前端状态 |
| 悬浮球外观滑块 | 手动 | 菜单与主窗口设置面板滑块即时生效、重启保持、无滚动条 |
| 迷你条拖动/关闭 | 手动 | 无边框顶栏整体拖动、关闭按钮悬停反馈 |
| Provider 表单与凭据 | 手动 | 五个云端 provider 表单逐一切换并保存凭据，凭据只进 Windows 凭据管理器 |

## 验收标准

- [ ] 五窗口（main/result/selector/overlay/floater）路由正确
- [ ] 选区窗口可拖动选择、Esc/Enter 操作
- [ ] 主窗口控制按钮功能正常
- [ ] 结果窗口迷你条：译文为主体、原文可折叠、工具栏完整
- [ ] 迷你条外观：透明度/字号即时生效并持久化，仅 CSS alpha（无窗口 opacity API）
- [ ] 迷你条字体：字号由 `--result-font-size` 统一控制（无 `text-sm`/`text-xs` 覆盖）
- [ ] 迷你条无边框适配：顶栏整体拖动、关闭按钮样式、圆角壳
- [ ] 悬浮球：默认关闭、拖动、位置记忆、四项菜单动作 + 外观滑杆
- [ ] 悬浮球外观：透明度/直径 CSS 变量即时生效并持久化，折叠/展开无滚动条
- [ ] 五种云端 Provider（OpenAI/DeepL/Google/Azure/百度）+ 本地表单条件渲染正确
- [ ] OpenAI 配置 id 为 `openai`（非 `api`），`normalizeProviderId` 仅映射
      `local-onnx -> local`，未知/废弃 id 回退 `openai`
- [ ] 凭据（API Key/Secret）只经 `set_provider_credentials` 写入系统凭据，
      不存前端 store/配置/日志；百度为 APP ID + Secret 双字段
- [ ] 后端事件正确更新前端状态
- [ ] 窗口可置顶、拖动、缩放
- [ ] 图标使用 Lucide React
- [ ] README.md 完整

### 发行部署验收（本任务：模型下载卡片 + 本地引擎联动）

- [x] 设置面板「本地翻译模型」卡片：`get_model_status` 水合 + 下载/取消/
      删除/刷新按钮 + `model_download_progress` 进度条；下载状态存 store，
      关闭设置面板不中断后端下载
- [x] `ProviderSelect` 本地引擎禁用联动：`localProviderBlockReason`
      （missing/invalid/downloading）只禁用 local 选项并显示原因提示，云端
      选项不受影响；下载中切换 local 另有后端守卫
- [x] `ModelSetupBanner`（R6）：`hasModelSetupProblems` 条件显示 + 重试
      （`retry_model_setup`），状态健康自动消失，不阻断主窗口
- [x] services 新封装：`tauri.ts` 5 个无参数命令
      （`download_translation_model` / `cancel_translation_model_download` /
      `delete_translation_model` / `get_model_status` / `retry_model_setup`）+
      `modelActions.ts`（refresh/download/cancel/delete/retry/
      applyModelDownloadProgress）+ `events.ts` 的
      `MODEL_DOWNLOAD_PROGRESS` / `onModelDownloadProgress`
- [x] types 新契约：`ModelState` / `ModelEntryStatus` / `ModelStatusReport` /
      `ModelDownloadProgress`（snake_case，与 Rust DTO 一致）+
      `findTranslationModelEntry` / `hasModelSetupProblems` /
      `localProviderBlockReason` / `TRANSLATION_MODEL_ENTRY_ID`
- [x] 前端不保存任何模型内容；进度只进内存 store

## 开发注意事项

- 选区窗口使用透明 Tauri 窗口，CSS pointer-events 控制
- 选区坐标返回前转换为物理像素
- 前端不保存 API Key、模型原始输出或截图
- Tauri 2.11.5 无窗口级 opacity：透明度一律 CSS 背景 alpha，禁止 `setOpacity`
- 主窗口与悬浮球共用 `translateActions` 状态机，禁止复制逻辑
- 浮球显隐事件为纯前端事件，不新增 IPC Command
- 使用 Tailwind 而非内联样式
- Zustand store 不可变更新
- 组件保持小而专注，避免巨型组件
- 窗口标签通过 Tauri label 区分
- 多 Provider 同步（模块 10）：`AppStatus.translation_provider` 返回运行时实现 id
  （`openai`/`deepl`/`google`/`azure`/`baidu`/`local-onnx`）。前端
  `normalizeProviderId` 只把 `"local-onnx"` 映射为 `"local"`，云端 id 原样透传，
  已删除旧 `"api"` 分支；`ProviderId` 类型与凭据表单已扩展，百度凭据用
  `set_provider_credentials` 提交 APP ID + Secret。新增 provider 时必须同步更新
  后端白名单与本映射，否则状态水合会显示错误引擎。
