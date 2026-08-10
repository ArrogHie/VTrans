# frontend（模块 11）

VTrans 的 React + TypeScript 前端，负责主控制面板、透明区域选择器、翻译结果迷你条、选区方框与悬浮球；所有业务操作通过 Tauri IPC 调用 `vtrans-app`，不直接接触截图、模型或凭据。

## 模块职责

- 路由 Tauri 的 `main`、`selector`、`result`、`overlay`、`floater` 五个窗口。
- 提供单次翻译和实时翻译的控制面板。
- 翻译结果迷你条：译文为主、原文可折叠，支持置顶/暂停/重新翻译/外观/关闭；外观
  （背景透明度 0.3–1.0、字号 12–24px）即时生效并持久化；每个 WebView 独立
  水合 `get_app_config`，重启后保持用户保存值，不依赖主窗口。
- 悬浮球（默认关闭）：可拖动、位置记忆，点击展开框选翻译/实时翻译/暂停继续/打开主窗口
  菜单；支持透明度（0.3–1.0）与直径（32–72px）外观调节，即时生效并持久化。
- 通过拖动框选生成物理像素坐标的 `ScreenRegion`。
- 监听后端 pipeline/model 事件，并在多个 Tauri WebView 间同步结果和实时会话状态。
- 展示 OCR 原文和翻译结果，支持重新翻译、暂停/继续、置顶和窗口拖动。
- 设置面板支持选择五种云端翻译 Provider（OpenAI/DeepL/Google/Azure/百度）
  与本地 ONNX 模型，并按所选 Provider 条件渲染端点/模型/区域/APP ID 表单；
  API Key/Secret 只经 `set_provider_credentials` 写入系统凭据，不进配置、
  store、事件或日志。
- 常驻选区方框仅实时会话显示：单次翻译不显示方框，翻译完成后也不残留。
- 提供只读设置面板（捕获间隔、差异阈值、超时、重试、快捷键、置顶），显示当前后端配置。
- Debug 模式下实时显示进入 OCR 前的捕获帧缩略图（仅显示、不保存、不持久化）。

## 依赖关系

### 上游

- `vtrans-app`：唯一的 Rust/Tauri IPC 边界。
- `vtrans-core`：通过 `vtrans-app` 序列化 `ScreenRegion`、`OcrResult`、`TranslationResult` 和 `PipelineStatus`。

### 外部

- React 18、TypeScript、Vite
- Zustand：不可变前端状态
- `@tauri-apps/api` v2：`invoke`、`listen`、窗口控制
- Tailwind CSS：组件样式
- Lucide React：图标
- Vitest：前端单元测试

所有新增依赖均使用项目已有的 `package.json` 依赖；未修改 workspace Cargo 配置。

## 公开 API 概要

### IPC service

```ts
export function startRegionSelection(): Promise<ScreenRegion>;
export function captureOnce(region: ScreenRegion): Promise<OcrResult>;
export function startLiveTranslation(config: PipelineConfig): Promise<void>;
export function stopLiveTranslation(): Promise<void>;
export function saveSettings(settings: AppConfig): Promise<void>;
export function updateResultWindowAppearance(opacity: number, fontSizePx: number): Promise<void>;
export function updateFloatingBallAppearance(opacity: number, sizePx: number): Promise<void>;
export function getAppStatus(): Promise<AppStatus>;
export function setProviderCredentials(
  providerId: ProviderId,
  credentials: { apiKey?: string; appId?: string; secret?: string },
): Promise<void>;
```

其余命令封装包括 `cancelRegionSelection`、`updateLiveRegion`、`setOcrLanguage`、
`setTranslationProvider`、`setApiKey` 和 `loadLocalModels`。`setApiKey` 保持
原有签名（按当前已保存 provider 写入），设置面板统一改用
`setProviderCredentials`（显式传入草稿 provider id，只发送提供的字段；
百度凭据为 `appId` + `secret` 两个独立目标）。
两个外观命令只持久化对应字段，不获取实时会话锁、不重建翻译 provider，
实时运行中也可以保存（替代整包 `save_settings` 路径）。

### 类型与映射

```ts
export type ProviderId = "openai" | "deepl" | "google" | "azure" | "baidu" | "local";
export function normalizeProviderId(raw: string): ProviderId;
export function isCloudProvider(provider: ProviderId): boolean;
```

`normalizeProviderId` 把后端运行时实现 id 映射到前端配置标识符域：仅
`"local-onnx" -> "local"` 需要映射，OpenAI/DeepL/Google/Azure/百度原样透传，
未知值（含已废弃的 `"api"`）回退到默认 provider `"openai"`。
`TranslationConfig` 新增 `region`（Azure 区域）与 `app_id`（百度 APP ID）两个
可空字段；百度 Secret 与各 provider 的 API Key 一样只进系统凭据，不落配置。

### Event service

```ts
export function listenToEvent<K extends keyof EventPayloadMap>(
  event: K,
  callback: (payload: EventPayloadMap[K]) => void,
): Promise<Unlisten>;
export function subscribeToBackendEvents(handlers: ...): Promise<Unlisten>;
```

Debug 帧服务（仅 Debug 模式启用）：

```ts
export function subscribeToDebugFrames(
  onFrame: (frame: DebugFramePayload) => void,
): Promise<Unlisten>;
export function createLatestFrameStore<T>(): LatestFrameStore<T>;
export function useDebugFrame(enabled: boolean): DebugFramePayload | null;
```

`debug_frame_updated` 事件携带 base64 JPEG 缩略图（最长边 ≤ 480px）、区域坐标/尺寸、
帧序号和时间戳；`AppStatus.debug_mode` 决定面板是否渲染。

浮球显隐事件（纯前端内部事件，不经过 Rust）：

```ts
export function publishFrontendFloaterEnabled(enabled: boolean): Promise<void>;
export function listenToFrontendFloaterEnabled(
  callback: (payload: { enabled: boolean }) => void,
): Promise<Unlisten>;
```

### 翻译动作服务（主窗口与悬浮球共用）

```ts
export function selectAndTranslateOnce(): Promise<TranslateActionResult>;
export function selectRegionForLive(): Promise<TranslateActionResult>;
export function startLive(): Promise<TranslateActionResult>;
export function toggleLivePause(): Promise<TranslateActionResult>;
export function stopLive(): Promise<TranslateActionResult>;
export function toggleLiveFromFloater(): Promise<TranslateActionResult>;
```

### 外观与浮球工具

```ts
export function applyResultAppearance(
  root: { style: { setProperty(name: string, value: string): void } },
  opacity: number,
  fontSizePx: number,
): void;
export function persistResultAppearance(
  opacity: number,
  fontSizePx: number,
): Promise<void>;
export function applyHydratedAppearance(
  config: Pick<AppConfig, "result_window">,
  root?: { style: { setProperty(name: string, value: string): void } } | null,
): { opacity: number; fontSizePx: number };
export function clampFloaterOpacity(value: number): number;
export function clampFloaterSizePx(value: number): number;
export function applyFloaterAppearance(
  root: { style: { setProperty(name: string, value: string): void } },
  opacity: number,
  sizePx: number,
): void;
export function persistFloaterAppearance(opacity: number, sizePx: number): Promise<void>;
export function clampFloaterPosition(
  position: FloaterPosition,
  monitors: readonly FloaterMonitor[],
  ballSize?: number,
): FloaterPosition;
export function loadFloaterPosition(storage: Pick<Storage, "getItem">): FloaterPosition | null;
export function saveFloaterPosition(storage: Pick<Storage, "setItem">, position: FloaterPosition): void;
export function applyFloaterVisibility(window: FloatWindow, enabled: boolean): void;
```

### Store

```ts
interface AppState {
  mode: "single" | "live";
  status: PipelineStatus;
  ocrResult: OcrResult | null;
  translationResult: TranslationResult | null;
  error: string | null;
  setMode(mode: "single" | "live"): void;
  setStatus(status: PipelineStatus): void;
}
```

`updateLanguage(kind, language)` 的乐观更新与后端联动语义一致：`ocr.language` 与
`translation.source_language` 是后端联动字段（`vtrans-app` 的 `set_ocr_language` /
`set_source_language` 总是同时赋值两者，由 `vtrans_config::validate_language_linkage`
校验）。主窗口将两者合并为单一「识别语言」选择器，切换时本地 state 同步更新两个字段，避免 hydrate 回滚前
本地短暂不一致导致 UI 闪烁或与后端联动校验冲突；`target_language` 不参与联动，
`updateLanguage("target", …)` 行为不变。`setOcrLanguage` / `setSourceLanguage` 的
`tauri.ts` 封装签名不变，跨 IPC 序列化契约不变。

`providerSwitching` 与 `setProviderSwitching` 标记翻译引擎切换期间的状态：
主窗口 `changeProvider` 在 `await setTranslationProvider` 期间置位，禁用引擎
下拉框并显示进度反馈（spinner + `model_loading_progress` 事件驱动的百分比），
`finally` 中复位；失败走既有 `setStatus({ error })` 路径并恢复可用态。
切换开始时清空 `modelProgress`，避免命中缓存时闪烁上次残留的百分比。

## 构建与测试

在仓库根目录执行：

```powershell
# 安装依赖（首次或 lockfile 更新后）
pnpm install

# 前端生产构建
pnpm build

# 单元测试
pnpm test -- --run

# 开发模式
pnpm dev

# Tauri 开发模式（需要 Rust/Tauri 环境）
pnpm tauri dev
```

也可以直接使用已安装依赖运行类型检查：

```powershell
.\\node_modules\\.bin\\tsc.cmd --noEmit
```

## 测试覆盖

- Zustand store 的 mode/status、嵌套配置不可变更新和错误状态。
- 语言联动乐观更新：主窗口「识别语言」选择器切换时 `ocr.language` 与
  `translation.source_language` 同步更新；切换目标语言不触碰联动字段。
- IPC command 参数名和 payload 结构（含选区取消识别）。
- 物理像素坐标转换（含反向拖拽）和零尺寸选区拒绝。
- pipeline 状态标签和序列化错误分支。
- 事件监听回调（含 `onOcrCompleted` 等便捷封装）的 payload 解包。
- 最新值帧缓冲：多次推送只保留最新一帧，`clear` 释放缓存，绝不累积。
- `debug_frame_updated` 事件订阅：事件名、payload 解包与注销清理。
- DebugPanel 渲染：「Debug 模式 · 仅显示不保存」文案、缩略图 data URL、坐标/尺寸/帧号/
  时间戳叠加、可选 OCR 对照行的展示与截断。
- 结果窗口外观：透明度/字号 clamp、CSS 变量即时应用（不涉及任何窗口 opacity API）、
  经 `update_result_window_appearance` 持久化（不走整包 `save_settings`）。
- 迷你条字体变量：译文/原文不再携带 `text-sm`/`text-xs` 覆盖类，字号统一由
  `--result-font-size` 控制（SSR 渲染断言）。
- 无边框迷你条：顶栏整体拖动区域、独立关闭按钮样式、圆角壳。
- 悬浮球外观：透明度/直径 clamp、CSS 变量即时应用（无窗口 opacity API）、
  经 `update_floating_ball_appearance` 持久化。
- 悬浮球滚动条：容器 `overflow-hidden`，展开菜单与窗口高度按直径动态适配。
- 悬浮球：位置 clamp 与 localStorage 记忆、显隐事件（`frontend_floater_enabled`）链路、
  菜单动作（框选翻译/实时启停/暂停继续/外观滑杆）经 mock IPC 覆盖。
- 浮球路由与折叠态渲染、窗口路由（`floater` label → FloatingBall）。
- 设置面板范围校验：背景透明度 0.3–1.0、字号 12–24 整数，拒绝越界与非整数。
- provider id 映射：`"local-onnx" -> "local"`，五个云端 id 原样透传，
  未知 id 与已废弃的 `"api"` 回退到默认 `"openai"`。
- 引擎切换反馈：`providerSwitching` 状态切换、`model_loading_progress` 驱动
  `modelProgress`、ProviderSelect 切换期间下拉框禁用 + spinner/百分比、
  完成/失败后恢复。
- 设置面板按 provider 条件渲染：OpenAI（端点 + 必填模型）、DeepL（Free/Pro/
  自定义端点）、Google（端点 + 可选模型）、Azure（端点 + 区域）、百度
  （APP ID + Secret 双输入，经 `set_provider_credentials` 提交）、本地
  （隐藏全部云端字段）。
- ProviderToggle：六个 provider 选项的标签、选中态与 `aria-pressed`。
- 凭据 IPC：`set_provider_credentials` 只发送提供的字段，百度 `appId` + `secret`
  双字段载荷与后端契约一致。
- 模式切换控件在实时会话运行期间的禁用行为。

### 手工验证项

以下行为依赖真实 Tauri 运行时与 Windows 桌面环境，未纳入自动化测试，按模块规格测试计划登记为手动验证项：

| 验证项 | 步骤 | 预期 |
|--------|------|------|
| 选区交互 | 主窗口点击“选择屏幕区域”，在选区窗口拖动生成矩形 | 出现高亮边框与尺寸标注；Enter 确认后主窗口显示区域信息 |
| 选区取消 | 在选区窗口按 Esc | 选区窗口隐藏，主窗口不显示错误；实时会话若被暂停则保持暂停 |
| 多显示器选区 | 在非主显示器上拖动选区 | 返回的 `ScreenRegion` 落在该显示器物理像素坐标内（`monitor_id` 与 DPI 缩放正确） |
| 结果展示 | 完成一次单次翻译 | 结果窗口显示原文与译文；重新翻译更新结果且清空旧译文 |
| 单次模式无方框 | 单次模式下框选并完成翻译 | 选区确认后与翻译完成后屏幕上均无常驻选区方框 |
| 实时模式方框 | 实时模式下重新框选区域 | 确认后常驻选区方框出现在新区域并跟随区域更新 |
| 启动水合方框 | 实时会话暂停后重启应用，或单次翻译后重启应用 | 实时模式重启后按已选区域恢复方框；单次模式重启后不显示方框 |
| 状态同步 | 发起实时翻译后观察主窗口与结果窗口 | 两窗口的状态/暂停/继续/停止保持一致 |
| 实时暂停 | 实时运行中点暂停再点继续 | 后端任务停止后前端显示暂停态；继续后恢复识别 |
| 快捷键会话 | 使用全局快捷键启动实时翻译 | 主窗口进入实时模式，暂停/停止按钮可用（依赖 `frontend_live_config` 事件，见已知限制 6） |
| 窗口控制 | 拖动/缩放主窗口与结果窗口，切换结果窗口置顶 | 窗口可拖动、可缩放；置顶开关即时生效 |
| 迷你条外观 | 拖动「外观」里的透明度/字号滑块 | 弹窗背景立即变半透明（桌面透出）、译文字号即时变化；保存后重启仍生效 |
| 迷你条外观水合 | 保存 0.8/18 后重启应用并打开结果窗口 | 结果窗口启动即显示 0.8/18（滑块值与背景/字号一致），无需打开主窗口 |
| 无边框迷你条拖动 | 按住迷你条顶栏（VTRANS 标识与周围区域）拖动 | 整个顶栏都可拖动窗口；置顶/暂停/重新翻译/外观/关闭按钮仍可点击 |
| 迷你条关闭按钮 | 悬停顶栏最右侧关闭按钮 | 按钮背景变红、图标变白；点击隐藏窗口 |
| 迷你条原文折叠 | 点击迷你条「原文」行 | 原文默认收起，点击展开/再点收起；译文始终为主体 |
| 浮球显隐 | 主窗口设置面板切换「显示悬浮球」 | 切换后浮球立即出现/消失（不重启），重启后按保存的配置显示 |
| 浮球拖动与位置 | 拖动悬浮球到屏幕边缘，关闭菜单后重启应用 | 浮球停留在上次位置；显示器变化后自动夹回可见区域 |
| 浮球菜单 | 点击浮球，分别执行框选翻译/实时翻译/暂停继续/打开主窗口 | 四项动作分别触发对应主流程；实时状态文案随会话变化 |
| 浮球外观 | 展开浮球菜单调节透明度/大小，或在主窗口设置面板拖动同款滑块 | 球背景立即变半透明、直径即时变化，展开/收起均无滚动条；保存后重启仍生效 |
| Debug 帧显示 | 以 `--debug` 或 `VTRANS_DEBUG=1` 启动，开启实时翻译或单次翻译 | 主窗口出现「Debug 模式 · 仅显示不保存」面板，缩略图随捕获帧实时更新；正常启动时主窗口无该区块 |
| Debug 退出清理 | Debug 模式下停止翻译并退出应用 | 面板帧数据随窗口销毁释放，不落盘、不写入日志、不进入结果窗口 |
| Provider 表单 | 打开设置面板，依次选择 OpenAI/DeepL/Google/Azure/百度/本地 | 各 provider 只显示对应字段；切换 provider 自动套用默认端点，再手动改自定义端点 |
| 凭据保存 | 在百度表单输入 APP ID 与 Secret 并保存，或为非百度 provider 保存 API Key | 保存成功提示；凭据只进入 Windows 凭据管理器，`config.json` 与日志不含 Key/Secret |

## 已知限制

1. 当前 `vtrans-app::capture_once` 公开命令返回 OCR 结果；单次翻译的译文依赖后续 app 层命令契约完善，前端不会伪造译文。
2. 设置面板可编辑采集参数、API 端点/模型/超时/重试、快捷键与结果窗口置顶，通过 `save_settings` 整包保存。由于 app 层暂未提供完整配置读取命令（缺 `get_app_config`），OCR 语言、日志级别等未在表单中的字段沿用前端当前值保存，可能覆盖后端其它配置；建议 vtrans-app 补充配置读取或局部更新命令。API Key 管理依赖 vtrans-app 新增 `set_api_key` 命令（vtrans-security 的 Credential Manager 已具备写入能力），前端暂未开放 API Key 输入。
   （更新：`get_app_config` 已提供，各 WebView 挂载时自行水合；设置面板已开放
   凭据输入——API Key 经 `set_provider_credentials` 写入系统凭据，百度为
   APP ID + Secret 双字段，配置文件中仅存百度 APP ID，Secret 不落盘。）
3. 结果窗口初始可见性、透明选区和窗口尺寸由 `src-tauri/tauri.conf.json` 管理；前端只负责运行时显示、隐藏和置顶。
4. `model_dir` 是 Rust 配置中的 `PathBuf`，前端仅回传字符串或 `null`，不会读取模型文件。
5. 事件监听在每个 webview 中安装，窗口销毁时统一清理；事件到达前关闭的窗口不会补发历史结果。
6. 通过全局快捷键启动的实时会话依赖 `vtrans-app` 在快捷键路径补发 `frontend_live_config` 事件；`applyStatus` 保留回退逻辑：收到 `live_running && selected_region` 且本地没有 `liveConfig` 时，用 `config.capture` 默认值构造配置，让暂停/停止在事件到达前立即可用。若后端版本未补发该事件，捕获间隔/阈值会以默认值近似。
7. API Key、完整原文/译文、截图像素和模型原始输出不存储在前端 store，也不会写入浏览器日志。
8. Debug 面板仅在应用以 `--debug` 或 `VTRANS_DEBUG=1` 启动时出现（开关不写入 config.json）。
   帧图像由后端以 ≤10fps 节流、最长边 ≤480px 的 base64 JPEG 缩略图经 `debug_frame_updated`
   事件推送；前端仅保留内存中最新一帧，关闭 Debug 模式或退出即释放，不落盘、不写日志、
   不发送到结果窗口。Debug 关闭时前端不订阅该事件，整条链路零开销。
9. 常驻选区方框的显隐以 `vtrans-app` 的 `update_live_region(region, mode)` 契约为准：
   单次模式确认与单次捕获结束由后端隐藏方框，前端在单次翻译完成后也会显式隐藏一次。
   由于后端无法区分「暂停」与「停止」，停止后 `get_app_status` 仍可能报告 `live`；
   前端在打开选区窗口时会主动拉取一次状态以同步真实模式，拉取失败时按 `single`
   安全降级（不显示方框），下一次选区确认或单次捕获会立即纠正。
10. 结果窗口与悬浮球透明度完全由 CSS 背景 alpha 实现（`--result-opacity` /
    `--floater-opacity`）：Tauri 2.11.5 无窗口级 opacity 能力（Rust/JS/ACL 均无），
    前端不调用任何 `setOpacity` 类 API；文字不参与淡出，仅背景半透明。外观保存走
    `update_result_window_appearance` / `update_floating_ball_appearance` 专用命令
    （不再整包 `save_settings`），滑块改动防抖 350ms，实时会话运行期间也可保存。
    每个 WebView 的 Zustand store 相互隔离：result/floater 均在挂载时自行
    `get_app_config` 水合（ResultWindow 应用外观、FloatingBall 控制显隐与外观），
    不依赖主窗口；`DEFAULT_CONFIG.version` 与后端 `CURRENT_CONFIG_VERSION`（5）
    保持一致，避免未水合即保存被后端校验拒绝。
11. 悬浮球默认关闭（`floating_ball.enabled`），直径与透明度（`size_px` 32–72、
    `opacity` 0.3–1.0）由 CSS 变量 `--floater-size` / `--floater-opacity` 驱动，
    折叠窗口尺寸、位置 clamp 与展开菜单高度随直径动态适配；展开/收起容器
    `overflow-hidden`，无滚动条。位置记忆存于 localStorage（`vtrans.floater.position`），
    显示器拓扑变化时按首个可用显示器兜底。浮球窗口 `focus: false`，展开菜单通过
    编程式放大窗口实现，菜单为四项动作 + 外观小控制。
12. 设置面板切换 provider 时会套用该 provider 的规范端点并覆盖草稿中的自定义端点；
    需要自定义端点的用户应先选 provider 再修改端点。Google 的模型名在表单中为可选
    项（后端校验对非 OpenAI provider 不强制模型名），若后端实现对空字符串仍报错，
    前端会在错误信息中透出并提示补填。
13. 主窗口引擎切换的进度反馈依赖 `vtrans-app` 在 `set_translation_provider`
    路径补发 `model_loading_progress` 事件（分配单 1 的后端修复）。前端在事件
    到达前显示「正在切换翻译引擎…」spinner；命中缓存时进度近瞬时跳到 100%。
    若后端版本未补发事件，切换期间仅显示 spinner，完成后直接生效，不影响功能。
    切换完成/失败后 `providerSwitching` 复位，下拉框恢复可用。
