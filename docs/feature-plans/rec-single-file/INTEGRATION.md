# 整合报告：rec 模型单份化（消除三份重复拷贝）

## 合并记录

| 模块 | 分支 | 合并顺序 | 结果 |
|------|------|----------|------|
| 08 vtrans-models | `feat/08-rec-single-file`（c7c32b6） | 1 | ✅ 无冲突，`--no-ff` 合并到 main |

05 / 07 不派单、无改动：05 已按 path 共享 rec session（main）；07 引用为测试夹具。

## 集成验证

- workspace 编译/测试/clippy/fmt：PASS
  - `cargo check --workspace` ✅
  - `cargo test --workspace` ✅（全 crate 全绿，含新增 `shared_rec_path_verifies_with_single_file`）
  - `cargo clippy --workspace --all-targets` ✅
  - `cargo fmt --all -- --check` ✅
- 前端测试/类型检查：PASS（`pnpm test` 177/177；`pnpm exec tsc --noEmit`）
- 模型校验：`vtrans-verify-models --models src-tauri/resources/models` → `verified 9/9`、`all model files are valid` ✅
- 端到端冒烟（验收标准逐条）：
  - [x] 磁盘 `ocr/` 仅一份 rec（`rec.onnx`，21,159,378 B）；`rec_en.onnx` / `rec_multi.onnx` 已删除（省 42,318,756 B ≈ 42.3 MB）
  - [x] 两份 manifest 三槽位 `path` 均指向 `ocr/rec.onnx`，sha256 / size 一致
  - [x] verify 9/9 valid
  - [x] 启动仅加载一个 rec session（provider 按 path 相等 `Arc::clone`；`docs/integration-report.md` 相关描述已同步）
  - [x] backfill 幂等收敛实测：预置 stale `rec_en/rec_multi` 后被清理，二次运行仍单份
  - [x] ja / en / auto 语言路由逻辑无改动（05 共享判定按 path，回归由 workspace 测试覆盖）
  - [x] 文档同步（DEVELOPMENT.md §4 / 08-models.md / vtrans-models README）
- 回归范围：单次/实时翻译链路、配置、IPC、前端均未改动；workspace 全量测试通过，无回归

## 遗留问题

- 模型自动获取 / 随安装包分发：不在本次范围（用户明确延后），后续另行立项
- `docs/integration-report.md` §5.5「7 个文件 / verified 7/7」为 v4 时代遗留计数，与当前 v6 布局（9 项）不一致，登记为后续文档清理项
- 新布局下 Release 冒烟未重新实测启动耗时（原 ~280 ms 为三 session 数据）；单 rec session 预期更快，待下次 Release 验证时更新实测值

## 结论

- [x] 功能已整合，验收通过，关闭
- [ ] 存在遗留问题，需跟踪（列明负责人）：文档计数清理项无负责人，随下次文档同步处理
