# frontend（模块 11）

VTrans 的 React + TypeScript 前端，负责主控制面板、透明区域选择器和翻译结果窗口；所有业务操作通过 Tauri IPC 调用 `vtrans-app`，不直接接触截图、模型或凭据。

## 模块职责

- 路由 Tauri 的 `main`、`selector`、`result` 三个窗口。
- 提供单次翻译和实时翻译的控制面板。
- 通过拖动框选生成物理像素坐标的 `ScreenRegion`。
- 监听后端 pipeline/model 事件，并在多个 Tauri WebView 间同步结果和实时会话状态。
- 展示 OCR 原文和翻译结果，支持复制、重新翻译、暂停/继续、置顶和窗口拖动。
- 提供只读设置面板（捕获间隔、差异阈值、超时、重试、快捷键、置顶），显示当前后端配置。

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
export function getAppStatus(): Promise<AppStatus>;
```

其余命令封装包括 `cancelRegionSelection`、`updateLiveRegion`、`setOcrLanguage`、`setTranslationProvider` 和 `loadLocalModels`。

### Event service

```ts
export function listenToEvent<K extends keyof EventPayloadMap>(
  event: K,
  callback: (payload: EventPayloadMap[K]) => void,
): Promise<Unlisten>;
export function subscribeToBackendEvents(handlers: ...): Promise<Unlisten>;
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
- IPC command 参数名和 payload 结构（含选区取消识别）。
- 物理像素坐标转换（含反向拖拽）和零尺寸选区拒绝。
- pipeline 状态标签和序列化错误分支。
- 事件监听回调（含 `onOcrCompleted` 等便捷封装）的 payload 解包。
- 后端 provider id（`"api"` / `"local-onnx"`）到前端配置标识符的映射与未知值回退。
- 模式切换控件在实时会话运行期间的禁用行为。

### 手工验证项

以下行为依赖真实 Tauri 运行时与 Windows 桌面环境，未纳入自动化测试，按模块规格测试计划登记为手动验证项：

| 验证项 | 步骤 | 预期 |
|--------|------|------|
| 选区交互 | 主窗口点击“选择屏幕区域”，在选区窗口拖动生成矩形 | 出现高亮边框与尺寸标注；Enter 确认后主窗口显示区域信息 |
| 选区取消 | 在选区窗口按 Esc | 选区窗口隐藏，主窗口不显示错误；实时会话若被暂停则保持暂停 |
| 多显示器选区 | 在非主显示器上拖动选区 | 返回的 `ScreenRegion` 落在该显示器物理像素坐标内（`monitor_id` 与 DPI 缩放正确） |
| 结果展示 | 完成一次单次翻译 | 结果窗口显示原文与译文；复制按钮写入剪贴板；重新翻译更新结果且清空旧译文 |
| 状态同步 | 发起实时翻译后观察主窗口与结果窗口 | 两窗口的状态/暂停/继续/停止保持一致 |
| 实时暂停 | 实时运行中点暂停再点继续 | 后端任务停止后前端显示暂停态；继续后恢复识别 |
| 快捷键会话 | 使用全局快捷键启动实时翻译 | 主窗口进入实时模式，暂停/停止按钮可用（依赖 `frontend_live_config` 事件，见已知限制 6） |
| 窗口控制 | 拖动/缩放主窗口与结果窗口，切换结果窗口置顶 | 窗口可拖动、可缩放；置顶开关即时生效 |

## 已知限制

1. 当前 `vtrans-app::capture_once` 公开命令返回 OCR 结果；单次翻译的译文依赖后续 app 层命令契约完善，前端不会伪造译文。
2. 完整 `AppConfig` 尚未由当前 app 层通过 IPC 返回，因此设置面板为只读，避免用未 hydrate 的默认值覆盖用户配置；OCR 语言和翻译 Provider 通过专用 command 立即保存。源语言/目标语言控件在对应 app IPC 提供前保持禁用。
3. 结果窗口初始可见性、透明选区和窗口尺寸由 `src-tauri/tauri.conf.json` 管理；前端只负责运行时显示、隐藏和置顶。
4. `model_dir` 是 Rust 配置中的 `PathBuf`，前端仅回传字符串或 `null`，不会读取模型文件。
5. 事件监听在每个 webview 中安装，窗口销毁时统一清理；事件到达前关闭的窗口不会补发历史结果。
6. 通过全局快捷键启动的实时会话依赖 `vtrans-app` 在快捷键路径补发 `frontend_live_config` 事件；`applyStatus` 保留回退逻辑：收到 `live_running && selected_region` 且本地没有 `liveConfig` 时，用 `config.capture` 默认值构造配置，让暂停/停止在事件到达前立即可用。若后端版本未补发该事件，捕获间隔/阈值会以默认值近似。
7. API Key、完整原文/译文、截图像素和模型原始输出不存储在前端 store，也不会写入浏览器日志。
