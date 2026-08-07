# 功能开发计划：OCR 模型升级 PP-OCRv4 → PP-OCRv6 Small

## 概述

- 需求来源：用户 2026-08-07 提出；参考文档 `docs/PP-OCRv6_small_ONNX_Rust_TS_接入指南.md`（已入库）
- 功能目标：将 OCR 检测/识别模型从 PP-OCRv4（RapidOCR v3.9.2 ONNX）升级为 PP-OCRv6 Small（det + rec），提升识别精度；统一 rec 槽位解锁 `auto` / `zh-CN` OCR 语言；**彻底弃用 v4**，不保留回退路径
- 使用场景：全部 OCR 路径（单次框选翻译、实时区域翻译）；无用户交互变化，模型在应用启动加载时生效
- 优先级 / 版本目标：P1 / 建议 v0.2.0
- 状态：开发中（08 已交付并审查，5 项打回修复中）

## 验收标准（用户可验证）

- [ ] 应用启动日志显示加载 v6 模型（det/rec id 含 `ppocrv6`），SHA-256 校验通过，类数一致性检查通过
- [ ] 英文清晰屏幕文字识别结果正确（固定测试集断言，与 Python 基准文本一致）
- [ ] `auto` / `zh-CN` OCR 语言可用：选择「自动检测」或「简体中文」可识别中文清晰文字
- [ ] 长行（60+ 字符）完整识别，无截断
- [ ] 竖排文字至少不崩溃（质量不作承诺，登记为已知限制）
- [ ] 单次/实时链路、快捷键、语言切换无回归
- [ ] 仓库内脚本可复现「下载 → 转换 ONNX → 检查 → Python 基准 → 回填 manifest」全流程（开发机需 Python/PaddlePaddle）
- [ ] 文档同步：`docs/DEVELOPMENT.md` §4、两个 crate README、`docs/modules/05-ocr.md`、`docs/modules/08-models.md`、集成报告已知限制

## 涉及模块与顺序

| 序号 | 模块 | 任务类型 | 依赖 | 建议分支 | 状态 |
|------|------|----------|------|----------|------|
| 1 | 08 vtrans-models | 修改（schema + manifest + 脚本 + 文档） | — | `feat/08-ppocrv6-models` | 已整合 |
| 2 | 05 vtrans-ocr | 修改（预处理/后处理/字典/测试/文档） | 依赖 1（schema 字段） | `feat/05-ppocrv6-ocr` | 已整合 |
| 3 | 07 vtrans-translation | 修改（仅测试字面量兼容修复） | 依赖 1 | `fix/07-ppocrv6-params-test` | 待派单 |
| 4 | 整合（协调者） | 合并 + workspace 验证 + 端到端 + 报告 | 依赖 1、2、3 | main 上整合 | 整合中 |

排除项（不拆任务）：

- 01 vtrans-core：冻结契约零改动（无新类型 / trait / serde 变更）
- 02 vtrans-config：无新增配置字段
- 10 vtrans-app：无新增 Command/Event；provider id `"pp-ocr"` 不变
- 11 frontend：OCR 语言选项已含 `auto` / `zh-CN`（`src/windows/MainWindow.tsx` `OCR_LANGUAGES`），无需改动
- 09 vtrans-pipeline：不受影响
- 07 vtrans-translation：仅新增 1 处测试字面量兼容修复（`PreprocessParams` 新字段），运行时代码不受影响

## 契约变更

- **冻结契约（vtrans-core）**：不涉及。`OcrProvider` trait、`OcrOptions`、`OcrResult`、`OcrError` 全部不变。
- **vtrans-models schema（08 内部契约，已评审并通知下游 05/07）**：
  - `PreprocessParams` 扩展**具体类型字段 + `#[serde(default = ...)]`**（已确认：不用 Option；manifest version 保持 1、向后兼容）：
    - det：`box_threshold=0.45`、`max_candidates=3000`、`min_box_size=3.0`
    - rec：`rec_input_height=48`、`rec_input_width=320`、`rec_append_space=true`、`rec_blank_index=0`
  - 现有字段随新 manifest 更新：`det_threshold` 0.3 → 0.2、`unclip_ratio` 2.0 → 1.4
  - **`image_size` 从 [960,960] 改为 [640,640]**：实测 det ONNX 为动态 H/W，Python 基准与指南 §6.1/§10.1 均为 640 上限；三者必须一致
  - 字典：`ppocrv6_dict.txt`（18,708 行 + blank + space = 18,710 类）；ja / en / auto 三个槽位共用同一字典
- **IPC 契约**：无新增 Command/Event。
- **Provider id / 语言对**：id 不变；rec_ja / rec_en / rec_multi 统一指向 v6 rec → `auto` / `zh-CN` 解锁（后端行为变化，前端已支持）。

## 风险与假设

已确认决策（2026-08-07）：

1. 统一模型：rec_ja / rec_en / rec_multi 指向同一 `PP-OCRv6_small_rec` ONNX 与同一字典 → 解锁 `auto` / `zh-CN`
2. 脚本方案 B：`scripts/` 包含下载 → PaddleX paddle2onnx 转换 → ONNX 检查 → Python 基准 → manifest 回填的完整可复现流程（开发机需 Python 3.10/3.11、PaddlePaddle 3.0+、paddlex[ocr]、paddle2onnx 插件）
3. 抛弃 v4：不保留 v4 模型、不回退、不兼容 v4 专有参数路径；**不做任何与 v4 的对比测试**（含英文）；日文/竖排质量不纳入验收

剩余风险：

- 风险 A：v6 通用模型对日文/竖排质量无官方承诺，已明确不纳入验收；本次不建立与 v4 的对比基线，验收只依赖 v6 自身固定测试集断言与 Python 基准
- 风险 B：ONNX 元数据未知（输入宽高固定/动态、节点名、是否内嵌 character 表、opset）→ 必须先 inspect 再编码；禁止硬编码节点名（指南 §5.3 / §11）
- 风险 C：通道顺序（BGR vs RGB）以 Python 基准为准，不得凭 ImageNet 直觉改动（指南 §6.3 / §16.2）
- 风险 D：转换工具链版本漂移（paddle2onnx / ONNX Runtime）→ 锁版本，升级后重跑固定回归集
- 风险 E：识别批处理（batch 4–16）当前未实现 → 标记为后续优化，不阻塞本次
- 风险 F（已实测确认）：det ONNX 输入动态 `[N,3,H,W]`（opset 14），rec 输入 `[N,3,48,W]` 动态宽、输出 C=18710（opset 11），类数一致性通过；05 按动态路径实现，勿假设固定 640×640
- 风险 G：08 打回项（tokenizer.json 入库、v4 脚本残留、image_size 对齐、inspect 报告入库）未修复前不得合并，见 08 任务单「打回修复项」
- 已知限制对照（`docs/integration-report.md` §6）：日文 OCR 实测、30 分钟长稳等手工项不受本次影响，仍需人工；本次新增「v6 日文/竖排质量未专项验收」已知限制

## 实施顺序

1. 08 打回修复（tokenizer.json 出库、v4 脚本处置、image_size 640、inspect 报告入库、inspect 脚本 mkdir）后重新提交
2. 08 合并到 main → 05 从 main 拉分支 `feat/05-ppocrv6-ocr`；07 从 main 拉分支 `fix/07-ppocrv6-params-test`（仅测试字面量）
3. 05、07 合并 → 整合验证（workspace 门禁 + 端到端 + 回归）→ 整合报告 → 台账关闭
