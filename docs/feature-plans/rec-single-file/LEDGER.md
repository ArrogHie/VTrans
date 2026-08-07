# 功能台账：rec 模型单份化（消除三份重复拷贝）

已确认决策（2026-08-07）：

1. 单份文件命名为 `ocr/rec.onnx`；rec_ja / rec_en / rec_multi 槽位与 id 保留（`ppocr-rec-v6` / `-en` / `-multi`）
2. 05 / 07 零改动：05 已按 path 共享 rec session（main）；07 引用为测试夹具
3. 模型自动获取 / 随安装包分发不在本次范围（用户明确延后）
4. 沿用上一功能决策：不做与 v4 的对比测试；字典统一为 `ppocrv6_dict.txt`

| 日期 | 功能/任务 | 状态 | 说明 |
|------|-----------|------|------|
| 2026-08-07 | 功能：rec 模型单份化 | 已拆解 | 计划与 TASK-08 已产出，等待开发 |
| 2026-08-07 | 08 vtrans-models | 已整合 | Review 通过，已合并到 main（merge no-ff） |
| 2026-08-07 | 整合 | 已验收 | workspace 全量门禁 + 真实模型校验 + backfill 幂等实测通过；整合报告已产出 |

审查记录：

- 08 Review（2026-08-07，commit c7c32b6）：
  - ✅ 质量门禁：fmt / clippy / test 全绿（43 单测 + 9 集成 + 5 doctest，含新增 `shared_rec_path_verifies_with_single_file`）
  - ✅ 契约一致：三槽位 path → `ocr/rec.onnx`，id/sha/size 保留；05 共享逻辑按 path 判定直接生效，无运行时代码改动
  - ✅ 模块边界：仅 vtrans-models（README/manifest/tests）+ 脚本 + 文档；未触碰其他 crate 与 vtrans-core
  - ✅ 横切标准：无敏感日志、无调试残留、错误路径不变
  - ✅ 验收核对：磁盘单份 rec（21,159,378 B）；`vtrans-verify-models` 9/9 valid；backfill 幂等收敛实测通过（stale rec_en/rec_multi 被清理、二次运行仍单份）
  - ✅ 文档同步：DEVELOPMENT.md §4 / docs/modules/08-models.md / vtrans-models README

整合验证（2026-08-07，merge 到 main）：

- ✅ `cargo check --workspace` / `cargo test --workspace` / `cargo clippy --workspace --all-targets` / `cargo fmt --all -- --check`
- ✅ `pnpm test` 177/177；`pnpm exec tsc --noEmit`
- ✅ `vtrans-verify-models` 真实目录 9/9 valid
- ✅ 磁盘清理完成：`rec_ja.onnx` → `rec.onnx`，删除 `rec_en.onnx` / `rec_multi.onnx`（省 42.3 MB）

整合时复核项：

- ✅ `docs/integration-report.md`「det/rec_ja/rec_en 三 session 加载（~280 ms）」已更新为单 rec session 描述
- ⚠️ 遗留：`docs/integration-report.md` §5.5「7 个文件 / verified 7/7」为 v4 时代计数，与当前 9 项不一致，后续文档清理
