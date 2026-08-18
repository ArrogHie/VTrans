# 功能台账：发行部署（单文件夹安装 + 内置 OCR + 翻译模型一键下载）

## 功能信息
- 功能名称：发行部署（Portable Deployment）
- 需求来源：`docs/features/deployment/REQUIREMENTS.md`（用户需求文档，2026-08-17 提供）
- 优先级：P0（R1-R6）；P1（R7、config 迁移、凭据迁移、断点续传）
- 创建时间：2026-08-17
- 当前状态：已整合（main 831d561 + fix/repack-download-url 待合并）；待用户手工验收（GUI/安装冒烟，见 INTEGRATION_REPORT.md 遗留问题 1）

## 用户已确认决策（2026-08-17）
1. `download_url`：接受「版本化直链（GitHub Releases）+ 发布流程回填 sha256」；开发期用版本化占位 URL。
2. `VTRANS_CONFIG_DIR` / `VTRANS_MODEL_DIR`：删除文档条目（不补实现）；`VTRANS_MODEL_DIR` 保留 CLI 说明。
3. 开发模式 `data/` 落 `target/debug/data/`：接受。

## 用户验收反馈与修复（2026-08-18）
1. **bug：校验按钮提示误导**——翻译模型缺失（skipped）时仍显示「本地模型校验通过」。修复：前端 `VerifyReport` 补 `skipped` 字段 + `verifyReportMessage` 三态文案（fix/11-verify-skipped-message，e7ca84d），合并 main（831d561）；372 前端测试全绿。
2. **模型托管落地**——用户选定 GitHub Releases；main 已推送 origin（7ebbbb2..831d561，含 2 个 LFS 对象）；release v0.1.0 已创建并上传 translation ONNX（403368390 字节，资产名 `model.onnx`，gh 未应用 `#` 重命名语法）；manifest `download_url` 修正为 `…/v0.1.0/model.onnx`（fix/repack-download-url 待合并）。**已安装用户升级方式**：删除 `{安装目录}/data/models/manifest.json` 后重启（`load_or_repair_manifest` 仅在 manifest 缺失/损坏时从内置源重拷），或重装新安装包。

## 子任务台账

| 序号 | 模块 | 任务单文件 | 分支 | 阶段 | 状态 | 备注 |
|------|------|-----------|------|------|------|------|
| 1 | 03-security | TASK-03-security.md | feat/03-dpapi-file-store | A | 已整合 | 合并 main（526f93c）；fmt/clippy/test/workspace PASS，111 测试 |
| 2 | 08-models | TASK-08-models.md | feat/08-manifest-optional-entries | A | 已整合 | 合并 main（19b7aca）；fmt/clippy/test PASS，77 测试 |
| 3 | 10-app | TASK-10-app.md | feat/10-portable-data-layout | B | 已整合 | 合并 main（b87a22d）；四门禁 + workspace 测试全绿（143 单测 + 19 契约） |
| 4 | 11-frontend | TASK-11-frontend.md | feat/11-model-download-ui | C | 已整合 | 合并 main（3e2acf7）；368 前端测试 + tsc 通过 |
| 5 | 文档同步 | TASK-DOCSYNC.md | docs/deployment-doc-sync | D | 已整合 | 合并 main（4da1a49）；DEVELOPMENT/ARCHITECTURE/modules 03/08/10/11 同步 |
| — | 07-translation | （整合修复） | fix/07-model-entry-fields | A' | 已整合 | 合并 main（ae38b9e）；local_onnx.rs 测试助手补 3 字段 |

## 状态流转记录

### 2026-08-17 阶段 A/B/C 开发与整合完成
- 阶段 A（并行 worktree 有冲突教训→后续隔离）：03-security（b902b18，DpapiFileStore + 迁移，111 测试）与 08-models（ab5245e/c31b299/05e925b，optional/skipped 语义，77 测试）Review 通过后按序合并 main（526f93c、19b7aca）。
- 环境补齐：git-lfs 3.7.1 自 GitHub Releases 安装至 msys64（winget/pacman 不可用）。
- 整合修复：08 新字段使 vtrans-translation 测试助手编译失败 → 派 fix/07 子代理补字段（d8419420），合并 ae38b9e。
- 阶段 B：10-app（718684c/985c108/184b474）——R1 数据根、R2 自恢复模型就位 + bundle.resources + LFS（ocr onnx 指针入库、403MB 模型确认忽略）、R4 五命令 + 进度事件、R5 DPAPI 构造点 + 迁移 + 降级链、R6 启动容错；四门禁 + 契约测试全绿，合并 b87a22d。
- 阶段 C：11-frontend（4 提交）——下载卡片四态、ProviderSelect 禁用联动、启动横幅 + 重试；368 测试 + tsc 全绿，合并 3e2acf7（app 先合）。
- workspace 验证：cargo test --workspace 全绿（api_provider 本地服务器用例需清空全部代理环境变量，含 all_proxy——环境问题非代码问题）；clippy --workspace 仅 1 条既有 deepl.rs 警告（非本功能引入）。
- 打包冒烟：cargo tauri build 成功——NSIS 36MB + MSI 41MB；release 资源目录含 manifest/ocr 双模型/字典/tokenizer.json（约 36MB），无 translation/model.onnx（403MB 未进包）。
- 阶段 D 文档同步进行中（docs/deployment-doc-sync）。

## 整合记录（已完成）
1. ✅ 阶段 A：03 + 08 合并 main，Review 与门禁全过
2. ✅ 阶段 A'：fix/07 合并 main
3. ✅ 阶段 B：10-app 合并 main，workspace 编译/测试/clippy/fmt 通过
4. ✅ 阶段 C：11-frontend 合并 main，pnpm 368 测试 + tsc 通过
5. ✅ 打包冒烟：cargo tauri build 产出 NSIS+MSI，资源清单正确
6. ⏳ 阶段 D 文档同步：已完成（b5c2a98/1156202），合并 4da1a49
7. ⏳ 端到端 GUI 冒烟与手工验证项：待用户执行（见 INTEGRATION_REPORT.md）
8. ✅ 输出 INTEGRATION_REPORT.md；功能状态：已整合 / 待验收

### 2026-08-17 拆解完成
- 阅读 REQUIREMENTS.md、ARCHITECTURE.md、GIT_WORKFLOW.md、DEVELOPMENT.md、AGENT_DEV_PROMPT.md 与模块规格 02/03/07/08/10/11。
- 事实核验（需求锚点未重复探索，仅补充外围事实）：`src/components/ProviderSelect.tsx` 存在（R4 锚点正确）；`.gitattributes` 已含 `*.onnx filter=lfs`；`.gitignore` 当前整体忽略 `resources/models/ocr/*`（仅放行字典）与 `translation/*`；`tauri.conf.json` bundle 尚无 `resources` 键、`targets: "all"` 已满足；统筹提示词引用的全局 `docs/integration-report.md` 不存在（已知限制以 crate README 为准）。
- 影响面结论：涉及 03-security、08-models、10-app、11-frontend + 文档同步；排除 01-core（冻结、无契约变更）、02-config（schema 不变）、04/05/06/09（无改动）、07-translation（ModelLoad 缺失路径现状已满足，整合时复核）。
- 契约结论：冻结契约不涉及；新增 5 Command + 1 Event（IPC 契约在 PLAN.md 定稿）；manifest 字段为 08 内部 schema 扩展。
- 产出：PLAN.md、TASK-03/08/10/11、TASK-DOCSYNC、本台账。
- 待办：用户确认 3 个假设 → 分配阶段 A（03/08 并行）→ 阶段 B（10）→ 阶段 C（11，app 先合并）→ 阶段 D（DOCSYNC）→ 整合报告。
