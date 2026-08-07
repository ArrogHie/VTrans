# 整合报告：OCR 模型升级 PP-OCRv4 → PP-OCRv6 Small

> 日期：2026-08-07
> 范围：08 vtrans-models + 05 vtrans-ocr + 07 vtrans-translation（测试兼容）
> 状态：已整合、已验收

## 合并记录

| 模块 | 分支 | 合并顺序 | 结果 |
|------|------|----------|------|
| 08 vtrans-models | `feat/08-ppocrv6-models` | 1 | ✅ merge --no-ff → main |
| 05 vtrans-ocr | `feat/05-ppocrv6-ocr` | 2 | ✅ merge --no-ff → main |
| 07 vtrans-translation | `fix/07-ppocrv6-params-test` | 3 | ✅ merge --no-ff → main |
| 协调文档 | main 直接提交 | 4 | ✅ ledger/plan/AGENT_DEV_PROMPT 笔误修复 |

合并顺序按依赖层级：08（schema）→ 05（OCR 适配，依赖 08）→ 07（测试兼容，依赖 08）。05 分支实际基于 08 修复提交开发，合并时先合 08 再合 05，拓扑顺序成立，无冲突。

## 契约结论

- **冻结契约（vtrans-core）**：零改动。`OcrProvider` / `OcrOptions` / `OcrResult` / `OcrError` 全部不变。
- **vtrans-models schema**：`PreprocessParams` 新增 7 个具体类型字段（`#[serde(default = ...)]`），manifest version 保持 1、向后兼容；默认值与指南 §10.1 一致。
- **IPC / Provider id**：无新增 Command/Event；`"pp-ocr"` id 不变；`auto` / `zh-CN` 经 rec_multi 槽位解锁（前端已支持，无需改动）。

## 集成验证

| 项目 | 结果 |
|------|------|
| `cargo fmt --all -- --check` | ✅ PASS |
| `cargo check --workspace` | ✅ PASS |
| `cargo test --workspace` | ✅ PASS（全部 crate 测试 + doctest 全绿） |
| `cargo clippy --workspace --all-targets` | ✅ PASS（零警告） |
| `pnpm exec vitest run` | ✅ PASS（27 文件 / 177 测试） |
| `pnpm exec tsc --noEmit` | ✅ PASS |

### 端到端冒烟（真实 v6 模型）

| 验收条目 | 结果 |
|----------|------|
| 英文清晰文字识别正确（固定测试集断言） | ✅ 18/18 行全出；长句完整（"Varsha Deshpande"、"Wakidi sold a painting to the president of Indonesia for a bus ticket" 等） |
| `auto` / `zh-CN` OCR 语言可用 | ✅ 中文 4/4 行正确（含 auto/zh-CN 集成测试） |
| 长行（60+ 字符）完整识别，无截断 | ✅ 无压缩、无分片接缝伪影（动态宽单次推理） |
| 竖排文字至少不崩溃 | ✅ `vertical_text_does_not_crash` 通过 |
| 类数一致性 / SHA-256 / 加载期校验 | ✅ C=18710 == 18708+blank+space；inspect 报告入库；加载期 fail-fast 有诊断 |
| 与 Python 基准对照 | ✅ BGR 误差 0.0；det 输入最大误差 0.0175；det 框 18/18 坐标一致 |
| 单次链路（捕获→OCR→翻译→事件） | ✅ pipeline_verify 实测：OCR 15 行 1397ms → local-onnx 1268ms → 事件序列完整 |
| 回归范围（实时/快捷键/语言切换/设置） | ✅ workspace 全量测试（含 pipeline 实时 12 项、app 命令/事件契约、前端 177 测试）全绿 |

## 遗留问题 / 已知限制

| # | 遗留项 | 状态 |
|---|--------|------|
| 1 | v6 通用模型日文/竖排质量未专项验收（不纳入验收） | 已登记（05 README / docs/modules/05-ocr.md） |
| 2 | 中文 "PP-OCRv6" 等字母数字两侧空格缺失（v6 模型行为，非管线 bug） | 已登记 |
| 3 | 英文个别长句框首 "..." 偶尔解码为 ".."（一次字符差异） | 已登记 |
| 4 | 超 3200px 病态超宽行仍回退 320px 分片（约 4K 全宽行场景） | 已登记，后续优化 |
| 5 | 识别批处理（batch 4–16）未实现 | 后续优化，不阻塞 |
| 6 | `crates/vtrans-ocr/tests/long_line_regression.rs` 头部注释仍引用旧脚本名 `scripts/download_models.ps1` | 非阻塞文档笔误，后续顺手修正 |
| 7 | `docs/GIT_WORKFLOW.md` §7「字典文件不提交 Git」与 ppocrv6_dict.txt 入库决策措辞不一致 | 非阻塞文档同步 |

## 结论

- [x] 功能已整合，验收通过
- [ ] 存在遗留问题，需跟踪（均非阻塞，见上表 #1-7；#1-5 已在模块文档登记为已知限制）
