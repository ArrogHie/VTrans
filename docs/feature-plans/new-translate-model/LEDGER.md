# 功能台账：翻译模型升级（离线 en→zh / ja→zh）+ OCR 语言与源语言统一

> 状态流转：待拆解 → 开发中 → 待审查 → 待整合 → 已整合 → 已验收 → 已关闭

## 待用户确认的决策（2026-08-07，影响任务范围）

| # | 决策 | 推荐 | 影响范围 |
|---|------|------|----------|
| A1 | 翻译质量档位 Fast/Balanced 是否纳入本次 | 纳入（指南 §7 建议） | 02 / 10 / 11 |
| A2 | 本地 Provider 运行时 id `"local-onnx"` → `"local-native"` | 改为 `"local-native"` | 10 / 11（两端契约同步） |
| A3 | 删除旧 ONNX 单模型路径（`local_onnx.rs`） | 删除（指南弃用 ONNX 自回归路线） | 07 |
| A4 | manifest v2 为破坏性升级（v1 翻译段不再支持） | 破坏性升级（模型整体替换） | 08 / 07 / 10 |

## 功能台账

| 日期 | 功能/任务 | 状态 | 说明 |
|------|-----------|------|------|
| 2026-08-07 | 功能：翻译模型升级 + 语言统一 | 待拆解 | PLAN.md / TASK-02/07/08/09/10/11 已产出，待确认 A1–A4 |
| 2026-08-07 | 功能：翻译模型升级 + 语言统一 | 开发中 | 02 已整合；其余任务待分配/开发（A1–A4 待确认） |
| 2026-08-07 | 02 vtrans-config | 已整合 | 已合并到 main（merge --no-ff）；质量门禁与审查通过 |
| 2026-08-07 | 08 vtrans-models | 待分配 | 依赖 A4；manifest v2 + scripts/translation/ |
| 2026-08-07 | 07 vtrans-translation | 待分配 | 依赖 1（quality）、2（manifest v2）、A3；native bridge + Provider |
| 2026-08-07 | 09 vtrans-pipeline | 待分配 | auto 源路由 + 标点感知分块（可并行） |
| 2026-08-07 | 10 vtrans-app | 待分配 | 依赖 1、3、4、A2；联动命令 + Provider 组装 + 打包资源 |
| 2026-08-07 | 11 frontend | 待分配 | 依赖 5、A1、A2；联动 UI + 提示 + 质量档位 |
| 2026-08-07 | 整合 | 待整合 | 门禁 + 端到端 + 回归 + INTEGRATION.md |

## 审查记录

### 02 vtrans-config（2026-08-07）

- ✅ 质量门禁：fmt / clippy（零警告）/ test 全绿（101 单测 + 22 集成 + 10 doctest）
- ✅ 契约一致：`CURRENT_CONFIG_VERSION` 3→4；`translation.quality`（`"fast"`/`"balanced"`，serde default `"fast"`）与任务单一致；`validate_language_linkage` 跨字段校验（OCR 语言 == 源语言）就位
- ✅ 迁移：v3→v4 幂等；source 语言以 OCR 语言为权威同步；quality 缺省补 `"fast"`、显式值保留；v4 重迁移无副作用（测试覆盖）
- ✅ 模块边界：diff 仅限 `crates/vtrans-config/` + `docs/modules/02-config.md`；未触碰 vtrans-core 与其他 crate
- ✅ 横切标准：错误复用 `ConfigError`（`Validation` / `UnsupportedVersion`）；无敏感日志
- ✅ 验收标准：quality 持久化、迁移一致、跨字段校验、既有测试回归 全部满足
- ⚠️ 待同步：A1（quality 是否纳入本次）影响 10/11 消费该字段的范围；若 A1 裁定不纳入，`quality` 字段保留但前端暂不暴露

## 整合记录

### 02 合并（2026-08-07）

- ✅ merge --no-ff `feat/02-new-translate-model` → main（8 文件 +427/-35）
- ✅ 合并后 workspace 验证：fmt / check / test（全 crate 含 doctest）/ clippy 零警告 / pnpm test 177 通过 / tsc 全绿
- ✅ 工作区整理：feature-plans/new-translate-model 文档（PLAN/LEDGER/TASK + 接入指南与 starter）已提交到 main；旧 `docs/integration-report.md` 与根目录 PP-OCRv6 指南按用户暂存删除提交（2 个 docs commit）
- ⚠️ 遗留：删除的两个文档仍有引用（`docs/AGENT_FEATURE_COORDINATOR_PROMPT.md` / `docs/AGENT_COORDINATOR_PROMPT.md` 引用 integration-report；`crates/vtrans-models/src/manifest.rs` 文档注释引用根目录 PP-OCRv6 指南），待用户确认后由相关模块/文档负责人同步
