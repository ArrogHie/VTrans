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
- OCR 语言选择：自动、日语、英语
- 源语言选择：自动、中文、日语、英语
- 目标语言选择：中文、日语、英语
- 翻译引擎切换：API / 本地
- 当前状态与简要错误信息
- 设置入口

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
- 外观控制：背景透明度（0.3–1.0）与字号（12–24px），CSS 变量即时生效并 `save_settings` 持久化
- 启动水合：挂载时自行 `get_app_config` 应用持久化外观（各 WebView store 隔离，
  不依赖主窗口）；窗口存活期间 store 配置变化时同步重新应用
- 可置顶、可拖动、可缩放；`transparent: true` 由 vtrans-app 声明，半透明用 CSS 背景 alpha 实现

### 悬浮球 (FloatingBall.tsx)

- 默认关闭，启动按 `floating_ball.enabled` 显示；监听 `frontend_floater_enabled` 即时显隐
- 小圆球可拖动（`data-tauri-drag-region`），位置记忆到 localStorage 并 clamp 到可用显示器
- 点击展开紧凑菜单：框选翻译、实时翻译启停、暂停·继续、打开主窗口
- 复用 `services/translateActions.ts` 状态机，与主窗口共用、禁止复制

## 公开 API（前端内部）

### Services (services/)

```typescript
export async function startRegionSelection(): Promise<ScreenRegion>;
export async function captureOnce(region: ScreenRegion): Promise<OcrResult>;
export async function startLiveTranslation(config: PipelineConfig): Promise<void>;
export async function stopLiveTranslation(): Promise<void>;
export async function saveSettings(settings: AppConfig): Promise<void>;
export async function getAppStatus(): Promise<AppStatus>;

// translateActions.ts：主窗口与悬浮球共用的翻译状态机
export function selectAndTranslateOnce(): Promise<TranslateActionResult>;
export function selectRegionForLive(): Promise<TranslateActionResult>;
export function startLive(): Promise<TranslateActionResult>;
export function toggleLivePause(): Promise<TranslateActionResult>;
export function stopLive(): Promise<TranslateActionResult>;
export function toggleLiveFromFloater(): Promise<TranslateActionResult>;

// resultAppearance.ts：迷你条外观（CSS 变量，无窗口 opacity API）
export function applyResultAppearance(root, opacity, fontSizePx): void;
export function persistResultAppearance(config, opacity, fontSizePx): Promise<AppConfig>;
export function applyHydratedAppearance(config, root?): { opacity; fontSizePx };

export function onOcrCompleted(cb: (result: OcrResult) => void): Unlisten;
export function onTranslationCompleted(cb: (result: TranslationResult) => void): Unlisten;
export function onPipelineError(cb: (msg: string) => void): Unlisten;
```

浮球显隐事件为纯前端内部事件 `frontend_floater_enabled`（payload `{ enabled }`），
主窗口设置面板切换开关时 `emit`，不经过 Rust。

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
    StatusBar.tsx
    ResultCard.tsx
  stores/
    appStore.ts
  services/
    tauri.ts               # IPC 调用封装
    events.ts              # 事件监听
    translateActions.ts    # 主窗口与悬浮球共用翻译状态机
    resultAppearance.ts    # 迷你条外观应用与持久化
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
| 迷你条外观 | 单元 | clamp、CSS 变量应用、`save_settings` 持久化（mock invoke） |
| 悬浮球动作 | 单元 | mock IPC，覆盖框选翻译/实时启停/暂停继续/停止 |
| 浮球显隐 | 单元 | `frontend_floater_enabled` 事件链路与窗口 show/hide |
| 浮球位置 | 单元 | clamp 到显示器、localStorage 记忆往返 |
| 窗口路由 | 单元 | `floater` label 渲染 FloatingBall |
| 设置范围校验 | 单元 | 透明度 0.3–1.0、字号 12–24 整数 |
| 选区交互 | 手动 | 拖动生成矩形，Esc 取消，Enter 确认 |
| 结果展示 | 手动 | OCR/翻译结果正确显示 |
| 状态同步 | 手动 | 后端事件正确更新前端状态 |

## 验收标准

- [ ] 五窗口（main/result/selector/overlay/floater）路由正确
- [ ] 选区窗口可拖动选择、Esc/Enter 操作
- [ ] 主窗口控制按钮功能正常
- [ ] 结果窗口迷你条：译文为主体、原文可折叠、工具栏完整
- [ ] 迷你条外观：透明度/字号即时生效并持久化，仅 CSS alpha（无窗口 opacity API）
- [ ] 悬浮球：默认关闭、拖动、位置记忆、四项菜单动作
- [ ] 后端事件正确更新前端状态
- [ ] 窗口可置顶、拖动、缩放
- [ ] 图标使用 Lucide React
- [ ] README.md 完整

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
