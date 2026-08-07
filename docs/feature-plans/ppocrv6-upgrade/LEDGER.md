# 功能台账：OCR 模型升级 PP-OCRv4 → PP-OCRv6 Small

已确认决策（2026-08-07）：

1. 统一模型：rec_ja / rec_en / rec_multi → 同一 v6 rec 模型 + 同一字典（解锁 auto/zh-CN）
2. 脚本方案 B：仓库脚本包含下载 → 转换 → 检查 → 基准 → manifest 回填全流程
3. 抛弃 v4：不保留回退；不做任何与 v4 的对比测试（含英文）；日文/竖排质量不纳入验收；验收以 v6 自身固定测试集断言 + Python 基准为准

| 日期 | 功能/任务 | 状态 | 说明 |
|------|-----------|------|------|
| 2026-08-07 | 功能：OCR v6 升级 | 开发中 | 决策 1/2/3 已确认；08 已交付并审查 |
| 2026-08-07 | 08 vtrans-models | 待整合 | 修复复检通过：5 项修复全部确认；fmt/clippy/test 全绿（43+8+5） |
| 2026-08-07 | 05 vtrans-ocr | 待派单 | v6 适配 + 回归测试（依赖 08 修复合并） |
| 2026-08-07 | 07 vtrans-translation | 待派单 | 仅测试字面量兼容修复（依赖 08 修复合并） |
| 2026-08-07 | 整合 | 待整合 | 依赖 08、05、07 完成 |

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

状态流转：待拆解 → 开发中 → 待审查 → 待整合 → 已整合 → 已验收 → 已关闭
