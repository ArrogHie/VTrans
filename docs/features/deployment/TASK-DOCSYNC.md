# 模块开发说明：文档同步 — 发行部署「新布局与下载流程」文档增量

## AGENT_DEV_PROMPT 参数
- MODULE_NUMBER: —（跨模块文档任务，参照 TASK-DOCSYNC 先例）
- MODULE_NAME: docs
- MODULE_SLUG: docs
- CRATE_PATH: docs（及 crate README 仅当对应模块任务遗漏时补——正常情况下各模块任务已同步各自 README）
- SCOPE: docs
- BRANCH_NAME: docs/deployment-doc-sync（从 main 拉取；前置：feat/10-portable-data-layout 与 feat/11-model-download-ui 已合并 main）

## 功能上下文
- 功能目标：文档与新数据布局（`{exe}/data/`）、下载流程、新 IPC 契约一致（需求 R7，P1）。
- 本任务承担的部分：`docs/DEVELOPMENT.md`、`docs/ARCHITECTURE.md`、`docs/modules/03/08/10/11.md` 的同步；环境变量表处置。

## 任务要求
- 范围：仅限 `docs/` 下的以下文件；不修改任何源码、crate README（各模块任务已覆盖）、vtrans-core。
- 同步内容（以 main 实际代码为准，先读代码再改文档）：
  1. `docs/DEVELOPMENT.md`：
     - §4 模型文件准备：新布局说明——OCR 模型随仓库 LFS 入库、打包内置；翻译模型不进包、设置页下载到 `{exe}/data/models/translation/`；`scripts/ppocrv6/setup_ppocrv6.ps1` 保留为开发机重生成工具、不参与打包（R2 语义）。
     - §7 日志与调试：生产日志位置改为 `{exe}/data/logs/`（不再 `%APPDATA%\com.vtrans.app\logs\`）。
     - §8 环境变量表：**用户已确认（2026-08-17）：删除未实现条目** `VTRANS_CONFIG_DIR` / `VTRANS_MODEL_DIR`；`VTRANS_MODEL_DIR` 保留 `verify_models` CLI 的说明并注明「仅 CLI 生效，应用未实现」。
     - 新增「便携数据布局」小节或并入 §4/§7：`data/` 结构（config.json、logs/、credentials.bin、models/）、系统级例外（WebView2、NSIS 卸载注册表、MSVC 运行库）、perMachine 不支持、开发模式 `data/` 落 `target/debug/`。
  2. `docs/ARCHITECTURE.md` §6.4：Commands 列表补 `download_translation_model`、`cancel_translation_model_download`、`delete_translation_model`、`get_model_status`、`retry_model_setup`；Events 列表补 `model_download_progress`（payload 形状）；备注「图像不跨 IPC」不变。
  3. `docs/modules/03-security.md`：公开 API 补 `DpapiFileStore` 与 `migrate_windows_to_dpapi`；职责段补「凭据本地化（DPAPI 文件存储，替代系统凭据管理器为默认）——WindowsCredentialStore 保留为迁移来源与兼容实现」；已知限制（用户绑定）。
  4. `docs/modules/08-models.md`：`ModelEntry` 新字段、`VerifyReport.skipped`、optional 缺失语义；`verify_models.rs` CLI 同语义。
  5. `docs/modules/10-app.md`：Commands 清单补 5 个新命令；Events 补 `model_download_progress`；AppState 说明补下载 CancellationToken 与模型就位状态；数据目录锚定 `{exe}/data` 的说明；启动容错（模型未就位应用仍启动、翻译入口返回明确错误）。
  6. `docs/modules/11-frontend.md`：设置面板「本地翻译模型」卡片、ProviderSelect local 禁用联动、错误横幅 + 重试；services/types 新封装与类型。
- 约束（非实现代码）：
  - 只写文档，不改代码；与 main 实际代码不符处**以代码为准**并标注（若发现文档与代码冲突超出本功能范围，记录待办并 `warn` 在 PR 描述，不擅自大改）。
  - 禁止出现敏感信息（API Key 示例、完整下载 URL 的签名参数、真实 sha256 可写但以 manifest 为准——文档不重复抄写哈希，引用 manifest）。
- 提交规范：`docs(deployment): <一句话描述>`，可多次提交；PR 描述含改动清单与「以代码为准」的核对说明。

## 横切标准提醒（逐项附带）
- 文档风格与既有模块文档一致（表格 + 契约代码块）；不引入 TODO 占位。

## 完成定义（DoD）
- [ ] 上述 6 项同步完成且与 main 实际代码核对一致（关键符号：`DpapiFileStore`、`optional`、5 个命令名、事件名）
- [ ] `VTRANS_CONFIG_DIR`/`VTRANS_MODEL_DIR` 处置与用户决策一致
- [ ] 无源码改动；PR 描述含改动清单
