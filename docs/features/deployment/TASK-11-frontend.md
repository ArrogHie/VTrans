# 模块开发说明：11-frontend — 发行部署「翻译模型下载 UI + 启动容错横幅」增量

## AGENT_DEV_PROMPT 参数
- MODULE_NUMBER: 11
- MODULE_NAME: frontend
- MODULE_SLUG: frontend
- CRATE_PATH: src
- SCOPE: frontend
- BRANCH_NAME: feat/11-model-download-ui（从 main 拉取；合并顺序：feat/10-portable-data-layout 必须先合并 main）

## 功能上下文
- 功能目标：设置页一键下载 403MB 本地翻译模型（进度/取消/重下/删除）；模型未就位时主窗口明确提示并可重试；未安装时禁止选 local 引擎。
- 本模块承担的部分：R4 前端（设置页「本地翻译模型」卡片 + ProviderSelect 联动）、R6 前端（错误横幅 + 重试按钮）。
- 上游已提供（10-app 合并 main 后）：5 个 Command 与 1 个 Event（契约见 TASK-10-app.md「IPC 契约」节，本节重复关键签名）。

## IPC 契约（TypeScript 侧，与 Rust 一一对应；先读 TASK-10-app.md）

新增 service 封装（`src/services/tauri.ts`，遵循既有 camelCase 参数约定）：
```typescript
export function downloadTranslationModel(): Promise<void>;
export function cancelTranslationModelDownload(): Promise<void>;
export function deleteTranslationModel(): Promise<void>;
export function getModelStatus(): Promise<ModelStatusReport>;
export function retryModelSetup(): Promise<ModelStatusReport>;
```
事件监听（`src/services/events.ts` 或既有事件服务）：
```typescript
export function onModelDownloadProgress(cb: (p: ModelDownloadProgress) => void): Unlisten;
```
类型（`src/types/index.ts`）：
```typescript
type ModelState = 'ready' | 'missing' | 'invalid';
interface ModelEntryStatus { id: string; state: ModelState; optional: boolean; }
interface ModelStatusReport { entries: ModelEntryStatus[]; ocr_ready: boolean; translation_ready: boolean; }
interface ModelDownloadProgress { bytes: number; total: number; fraction: number; }
```

## 任务要求
- 范围：仅限 `src/`。禁止修改其他 crate、src-tauri、vtrans-core。
- 新增 UI（`src/components/SettingsPanel.tsx`）：
  - 「本地翻译模型」卡片：状态=未安装 / 下载中（进度百分比 + 进度条）/ 已安装 / 校验失败；按钮=下载 / 取消 / 重新下载 / 删除（按状态互斥展示）。
  - 状态来源：挂载时 `get_model_status()`（`translation` 条目：`ready` → 已安装，`missing` → 未安装，`invalid` → 校验失败）；进度经 `onModelDownloadProgress` 实时更新；下载 invoke resolve 后重新 `get_model_status` 刷新终态。
  - 下载中：显示进度与「取消」按钮，禁用「下载/重新下载/删除」。
  - 删除：二次确认后调用 `deleteTranslationModel`，完成后刷新状态。
  - 后端错误（如校验失败）经既有错误展示通道呈现给用户。
- `src/components/ProviderSelect.tsx` 联动：
  - `translation.provider` 选 local 但模型未安装（`missing`）→ 禁用并提示「请先在设置中下载本地翻译模型」。
  - 下载进行中 → 禁止切 local（禁用或切换即提示），与后端拒绝双保险。
- R6 错误横幅（复用 `src/components/ErrorBanner.tsx` 或主窗口既有横幅位置）：
  - 主窗口挂载时 `get_model_status()`；`ocr_ready == false` 或存在非 optional 条目 `invalid` → 显示持久横幅「OCR 模型未就位，翻译功能不可用」+「重试」按钮 → `retryModelSetup()` → 刷新状态；修复成功自动消失。
  - 不得阻塞主窗口其余功能（设置、框选入口仍可见）。
- 约束（非实现代码）：
  - 前端不保存任何模型文件内容；进度数据只进内存状态；不把完整 URL 写日志/控制台。
  - 沿用 Tailwind 风格与既有组件拆分粒度；状态管理遵循既有 store 模式（如需新 store 状态，保持不可变更新）。
  - 下载期间组件卸载不中断下载（下载任务在后端）；重新挂载时以 `get_model_status` + 进行中状态水合（若后端仍在下载，进度事件持续推送——按后端事件语义处理）。
- 测试要求（新增，映射需求验收标准 3/6；`pnpm test` + `tsc --noEmit`）：
  - 单元：卡片四态渲染（未安装/下载中/已安装/校验失败）与按钮互斥；进度事件驱动百分比更新。
  - 单元：IPC 调用参数与命令名（mock invoke）：download/cancel/delete/getModelStatus/retryModelSetup 与 camelCase 契约一致。
  - 单元：ProviderSelect——local+未安装禁用并提示；下载中禁止切 local。
  - 单元：错误横幅——ocr_ready=false 显示、retry 成功后消失；事件监听注册/清理（unlisten）。
- 文档要求：同步 `src/README.md`（若有）与类型注释；新增已知限制（进度为后端事件推送、断点续传 P1 等）。
- 提交规范：`feat(frontend): <一句话描述>`，可多次提交，每次可编译（tsc）；PR 描述含实现说明、测试覆盖、验收 checklist。

## 横切标准提醒（逐项附带）
- 日志：前端 console 不打印完整模型 URL、API Key、模型内容；错误信息脱敏展示。
- 测试与风格：`pnpm test` 全绿；`pnpm exec tsc --noEmit` 零错误；Tailwind 风格一致；组件小而专注。
- 契约：参数名遵循 Tauri 2 默认 camelCase（本功能命令无参数，无风险）；事件 payload 字段 snake_case（与 Rust 一致：`bytes`/`total`/`fraction`）。

## 完成定义（DoD）
- [ ] `pnpm test` 全绿；`pnpm exec tsc --noEmit` 零错误
- [ ] 验收标准第 3 条前端侧满足：下载进度可见、可取消、完成后可切 local；重新下载/删除可用（端到端冒烟为手工验证项，与 10-app 合并后执行）
- [ ] 验收标准第 4 条前端侧满足：模型异常时横幅提示 + 重试（不自愈时错误可见、应用不静默）
- [ ] 未修改 src 之外的文件；IPC 契约与 TASK-10-app.md 一致；PR 描述含实现说明、测试覆盖、验收 checklist
