# 功能台账：发行部署（单文件夹安装 + 内置 OCR + 翻译模型一键下载）

## 功能信息
- 功能名称：发行部署（Portable Deployment）
- 需求来源：`docs/features/deployment/REQUIREMENTS.md`（用户需求文档，2026-08-17 提供）
- 优先级：P0（R1-R6）；P1（R7、config 迁移、凭据迁移、断点续传）
- 创建时间：2026-08-17
- 当前状态：开发中（阶段 A 已派发：03-security + 08-models 并行）

## 用户已确认决策（2026-08-17）
1. `download_url`：接受「版本化直链（GitHub Releases）+ 发布流程回填 sha256」；开发期用版本化占位 URL。
2. `VTRANS_CONFIG_DIR` / `VTRANS_MODEL_DIR`：删除文档条目（不补实现）；`VTRANS_MODEL_DIR` 保留 CLI 说明。
3. 开发模式 `data/` 落 `target/debug/data/`：接受。

## 子任务台账

| 序号 | 模块 | 任务单文件 | 分支 | 阶段 | 状态 | 备注 |
|------|------|-----------|------|------|------|------|
| 1 | 03-security | TASK-03-security.md | feat/03-dpapi-file-store | A | 开发中 | R5；DpapiFileStore + 迁移函数 |
| 2 | 08-models | TASK-08-models.md | feat/08-manifest-optional-entries | A | 开发中 | R3；optional/skipped 语义 |
| 3 | 10-app | TASK-10-app.md | feat/10-portable-data-layout | B | 待分配 | R1/R2/R4 后端/R5 构造点/R6/R7 核对 + LFS 配置 |
| 4 | 11-frontend | TASK-11-frontend.md | feat/11-model-download-ui | C | 待分配 | R4 UI/R6 横幅；合并顺序 app 先 |
| 5 | 文档同步 | TASK-DOCSYNC.md | docs/deployment-doc-sync | D | 待分配 | R7 文档侧 |

## 状态流转记录

### 2026-08-17 拆解完成
- 阅读 REQUIREMENTS.md、ARCHITECTURE.md、GIT_WORKFLOW.md、DEVELOPMENT.md、AGENT_DEV_PROMPT.md 与模块规格 02/03/07/08/10/11。
- 事实核验（需求锚点未重复探索，仅补充外围事实）：`src/components/ProviderSelect.tsx` 存在（R4 锚点正确）；`.gitattributes` 已含 `*.onnx filter=lfs`；`.gitignore` 当前整体忽略 `resources/models/ocr/*`（仅放行字典）与 `translation/*`；`tauri.conf.json` bundle 尚无 `resources` 键、`targets: "all"` 已满足；统筹提示词引用的全局 `docs/integration-report.md` 不存在（已知限制以 crate README 为准）。
- 影响面结论：涉及 03-security、08-models、10-app、11-frontend + 文档同步；排除 01-core（冻结、无契约变更）、02-config（schema 不变）、04/05/06/09（无改动）、07-translation（ModelLoad 缺失路径现状已满足，整合时复核）。
- 契约结论：冻结契约不涉及；新增 5 Command + 1 Event（IPC 契约在 PLAN.md 定稿）；manifest 字段为 08 内部 schema 扩展。
- 产出：PLAN.md、TASK-03/08/10/11、TASK-DOCSYNC、本台账。
- 待办：用户确认 3 个假设 → 分配阶段 A（03/08 并行）→ 阶段 B（10）→ 阶段 C（11，app 先合并）→ 阶段 D（DOCSYNC）→ 整合报告。
