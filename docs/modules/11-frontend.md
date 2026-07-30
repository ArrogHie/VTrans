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

实现三个 Tauri 窗口的 React UI：主窗口（控制面板）、选区窗口（透明全屏选区）、结果窗口（原文+译文展示）。管理前端状态、通过 IPC 调用 Rust 后端、监听后端事件。

## 技术选型

- React 18 + TypeScript + Vite
- 状态管理：Zustand（轻量，适合 Tauri 应用）
- 样式：Tailwind CSS（工具优先，快速迭代）
- 图标：Lucide React
- Tauri IPC：@tauri-apps/api v2

## 三窗口设计

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

- 原文区域（只读展示）
- 译文区域（只读展示）
- 复制原文、复制译文按钮
- 重新翻译按钮
- 暂停/继续实时识别按钮
- 可置顶、可拖动、可缩放

## 公开 API（前端内部）

### Services (services/)

```typescript
export async function startRegionSelection(): Promise<ScreenRegion>;
export async function captureOnce(region: ScreenRegion): Promise<OcrResult>;
export async function startLiveTranslation(config: PipelineConfig): Promise<void>;
export async function stopLiveTranslation(): Promise<void>;
export async function saveSettings(settings: AppConfig): Promise<void>;
export async function getAppStatus(): Promise<AppStatus>;

export function onOcrCompleted(cb: (result: OcrResult) => void): Unlisten;
export function onTranslationCompleted(cb: (result: TranslationResult) => void): Unlisten;
export function onPipelineError(cb: (msg: string) => void): Unlisten;
```

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
  types/
    index.ts               # TS 类型定义
```

## 测试计划

| 测试项 | 类型 | 说明 |
|--------|------|------|
| Store 状态变更 | 单元 | setMode/update 正确更新 |
| IPC 调用封装 | 单元 | mock invoke，验证参数 |
| 事件监听 | 单元 | mock listen，验证回调 |
| 选区交互 | 手动 | 拖动生成矩形，Esc 取消，Enter 确认 |
| 结果展示 | 手动 | OCR/翻译结果正确显示 |
| 状态同步 | 手动 | 后端事件正确更新前端状态 |

## 验收标准

- [ ] 三窗口可创建和切换
- [ ] 选区窗口可拖动选择、Esc/Enter 操作
- [ ] 主窗口控制按钮功能正常
- [ ] 结果窗口显示原文和译文
- [ ] 后端事件正确更新前端状态
- [ ] 窗口可置顶、拖动、缩放
- [ ] 图标使用 Lucide React
- [ ] README.md 完整

## 开发注意事项

- 选区窗口使用透明 Tauri 窗口，CSS pointer-events 控制
- 选区坐标返回前转换为物理像素
- 前端不保存 API Key、模型原始输出或截图
- 使用 Tailwind 而非内联样式
- Zustand store 不可变更新
- 组件保持小而专注，避免巨型组件
- 窗口标签通过 Tauri label 区分
