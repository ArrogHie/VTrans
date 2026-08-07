## 模块开发说明：11 frontend — 翻译模型升级 / 语言统一 增量

### AGENT_DEV_PROMPT 参数

- MODULE_NUMBER: 11
- MODULE_NAME: frontend
- MODULE_SLUG: frontend
- CRATE_PATH: src
- SCOPE: frontend
- BRANCH_NAME: feat/11-new-translate-model

### 功能上下文

- 功能目标：主窗口「OCR 语言」与「源语言」两个下拉独立显示、改动任一自动联动另一项；本地模型能力提示更新（en/ja → zh-CN）；翻译质量档位 UI（A1）；Provider id 映射同步（A2）
- 决策状态（已确认 2026-08-07）：A1 质量档位 UI 纳入本次；A2 本地实现 id 为 `"local-native"`（`normalizeProviderId` 增加映射）
- 本模块承担的部分：`MainWindow` 语言联动交互与 store 同步；`isLocalPairSupported` 与提示文案更新；`SettingsPanel` 质量档位；`normalizeProviderId` 映射；对应测试
- 上游已提供：10 的联动命令（`set_ocr_language` / `set_source_language` 后端已双向同步）；02 的 `translation.quality` 字段；`AppStatus.translation_provider` 新实现 id（A2）

### 任务要求

- 范围：仅限 `src/`（含 `src/types`、`src/services`、`src/components`、`src/windows`、测试）；禁止修改 Rust 侧与 `src-tauri`
- 行为变更（约束性定义）：
  - `MainWindow.tsx`：`changeOcrLanguage` 成功后 `updateLanguage("ocr", value)` 并同步 `updateLanguage("source", value)`（本地 store 两字段一致）；`changeSourceLanguage` 对称处理。后端命令只调用被改动的那一个（后端已联动），避免双 IPC
  - `src/types/index.ts`：
    - `isLocalPairSupported` 更新：`provider === "local"` 时支持 `(source === "en" || source === "ja") && target === "zh-CN"`
    - 主窗口本地提示文案更新：「本地模型支持 en → zh-CN 与 ja → zh-CN 离线翻译；zh-CN 源语言请使用云端 API」
    - `normalizeProviderId`：A2 通过后增加 `"local-native" → "local"` 映射（保留 `"local-onnx"` 兼容映射可接受，注释说明）
    - `TranslationConfig` 类型新增 `quality: "fast" | "balanced"`；`DEFAULT_CONFIG` 同步（`quality: "fast"`）
  - `SettingsPanel.tsx`（或主窗口语言区，按现有 UI 归属）：质量档位单选 Fast/Balanced，随整包 `save_settings` 持久化；本地 Provider 生效（后端 10 已接入）
  - 语言下拉选项集保持 `[auto, ja, en, zh-CN]` 不变（OCR 与源语言两套选项同构）
- 约束（非实现代码）：
  - 禁止复制 `translateActions` 逻辑；联动逻辑集中在 `MainWindow` 的变更处理函数（组件保持小而专注）
  - 不新增 IPC Command/Event；`tauri.ts` 封装签名不变
  - 前端不保存 API Key / 模型原始输出 / 截图（既有红线）
- 测试要求（`pnpm test` + `pnpm exec tsc --noEmit`）：
  - 联动：改 OCR 语言 → store 中 source 同步；改源语言 → OCR 同步；`updateLanguage` 调用断言
  - `isLocalPairSupported`：`en→zh-CN` / `ja→zh-CN` 支持；`zh-CN→zh-CN`、`auto`、其他 target 不支持；`api` Provider 恒支持
  - `normalizeProviderId`：`"local-native"` / `"local-onnx"` → `"local"`；`"api"` → `"api"`
  - `DEFAULT_CONFIG.quality === "fast"`；SettingsPanel 档位渲染与保存参数断言
  - 既有 177 项测试全量回归
- 文档要求：`src/README.md`（若有）与 `docs/modules/11-frontend.md` 同步（联动交互、质量档位、Provider id 映射、验收标准）

### 横切标准提醒

- 日志：前端无日志约定（错误走状态栏展示）；错误信息对用户友好（`getIpcErrorMessage`）
- 测试与风格：vitest 全绿、tsc 零错误；Tailwind 而非内联样式；Zustand 不可变更新

### 完成定义（DoD）

- [ ] 质量门禁通过：`pnpm test`；`pnpm exec tsc --noEmit`
- [ ] 联动交互、提示文案、质量档位、Provider id 映射测试全绿
- [ ] 未修改 Rust 侧代码与 `src-tauri`
- [ ] PR 描述含实现说明、测试覆盖、验收 checklist
