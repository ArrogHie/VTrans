# 功能台账：OCR 模型升级 PP-OCRv4 → PP-OCRv6 Small

已确认决策（2026-08-07）：

1. 统一模型：rec_ja / rec_en / rec_multi → 同一 v6 rec 模型 + 同一字典（解锁 auto/zh-CN）
2. 脚本方案 B：仓库脚本包含下载 → 转换 → 检查 → 基准 → manifest 回填全流程
3. 抛弃 v4：不保留回退；不做任何与 v4 的对比测试（含英文）；日文/竖排质量不纳入验收；验收以 v6 自身固定测试集断言 + Python 基准为准

| 日期 | 功能/任务 | 状态 | 说明 |
|------|-----------|------|------|
| 2026-08-07 | 功能：OCR v6 升级 | 开发中 | 决策 1/2/3 已确认；08 已交付并审查 |
| 2026-08-07 | 08 vtrans-models | 已整合 | 已合并到 main（merge 08，no-ff） |
| 2026-08-07 | 05 vtrans-ocr | 已整合 | 已合并到 main（merge 05，no-ff）；5 真实模型回归 5/5 |
| 2026-08-07 | 07 vtrans-translation | 已整合 | 审查通过（47+9+10 全绿）；已合并到 main |
| 2026-08-07 | 整合 | 已验收 | 全量门禁 + 端到端冒烟通过；整合报告已产出 |

审查记录（2026-08-07）：

- 08 质量门禁：fmt ✅ / clippy ✅ / test ✅（43 单测 + 8 集成 + 5 doctest）
- 下游破坏：仅测试目标 3 处（ocr 单测 + ocr doctest + translation 单测）；非测试代码全绿
- ONNX 实测：det 动态 `[N,3,H,W]` opset 14；rec `[N,3,48,W]` 动态宽、C=18710 opset 11；类数一致性 ✅
- 契约决策：接受具体类型 + `#[serde(default)]`（弃用 Option 方案），05/07 任务单已同步

复检记录（2026-08-07，commit 629813e）：

- ✅ tokenizer.json 已出库；`.gitignore` 恢复 `translation/*` 整目录忽略（仅保留 `ocr/ppocrv6_dict.txt` 白名单）
- ✅ 旧 `scripts/download_models.ps1` 已删除；README.md / GIT_WORKFLOW.md 同步为新脚本
- ✅ manifest `image_size` 已改 [640,640]
- ✅ `scripts/ppocrv6/inspect_report.json` 已入库（det 动态 `[N,3,H,W]` opset 14；rec `[N,3,48,W]` 动态宽 C=18710 opset 11；类数一致性 ✅）
- ✅ `inspect_onnx.py` 写入前 mkdir parents（并加 dtype 可读名）
- ✅ 模块边界：仅 vtrans-models（lib.rs / manifest.rs）+ 脚本 + 文档，未触碰其他 crate 与前端
- ⚠️ 遗留：`docs/GIT_WORKFLOW.md` §7「字典文件不提交 Git」与新决策（ppocrv6_dict.txt 入库）措辞不一致；由 08 顺手修正或在整合阶段处理

05 审查记录（2026-08-07，commits aca1c98 / 042081e）：

- ✅ 质量门禁：fmt / clippy / test 全绿（70 单测 + 35 doctest + 5 provider_load）
- ✅ 真实模型回归（`--ignored`）：英文长句完整、中文 auto/zh-CN 4/4、竖排不崩溃、auto 无 multi 报错 —— 5/5 通过（4.57s）
- ✅ 实现核对：det 动态 H/W + 最近 32 对齐 + BGR（baseline 误差 0.0）；DB box_threshold/max_candidates/min_box_size 全参数化；rec 48 高、动态宽单次推理 ≤3200、超宽分片；类数加载期 fail-fast（诊断信息含 shape/字典行数/append_space/blank_index/路径）；rec_ja/en/multi 同模型共享 session；`--dump-det-input` 支持基准对照
- ✅ 模块边界：仅 vtrans-ocr 4 个源文件 + 测试素材 + 文档；未触碰其他 crate / 前端 / app
- ✅ 契约一致：OcrProvider / OcrOptions / OcrError 零改动；provider id 不变
- ⚠️ 小问题（非阻塞）：`tests/long_line_regression.rs` 头部注释仍引用旧脚本 `scripts/download_models.ps1`，整合阶段顺手修正
- ⚠️ workspace 全量：仅剩 07 `vtrans-translation` 测试字面量 1 处失败（E0063，计划内）

整合记录（2026-08-07，main 3c1d77b）：

- ✅ merge --no-ff feat/08-ppocrv6-models → main
- ✅ merge --no-ff feat/05-ppocrv6-ocr → main
- ✅ 协调文档更新提交（ledger/plan）
- ✅ 整合验证：fmt ✅ / cargo check --workspace ✅ / cargo test --workspace：仅 07 local_onnx.rs:1094 失败（计划内）
- ⏳ 待办：07 修复 → 全量测试/clippy 复跑 → 端到端冒烟 → 整合报告

07 审查与最终整合记录（2026-08-07）：

- ✅ 07 门禁：fmt / clippy / test 全绿（47 单测 + 9 集成 + 10 doctest）；改动仅 local_onnx.rs 1 处测试字面量（DEFAULT_* 常量）
- ✅ merge --no-ff fix/07-ppocrv6-params-test → main
- ✅ 最终全量：cargo test --workspace 全绿 / clippy workspace 零警告 / vitest 177 通过 / tsc 通过
- ✅ 端到端冒烟：英文 18/18、中文 4/4、pipeline 单次链路完整
- ✅ 整合报告：docs/feature-plans/ppocrv6-upgrade/INTEGRATION.md
- ✅ 功能状态：已验收（遗留项均非阻塞，已登记已知限制）

状态流转：待拆解 → 开发中 → 待审查 → 待整合 → 已整合 → 已验收 → 已关闭
