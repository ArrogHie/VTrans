# 功能开发计划：rec 模型单份化（消除三份重复拷贝）

## 概述

- 需求来源：用户指令「当前只解决只保留一份模型的问题，模型获取后面再考虑。解决 rec 三份文件是同一模型的重复拷贝，浪费约 42MB 的问题」
- 功能目标：manifest 的 `rec_ja` / `rec_en` / `rec_multi` 三个槽位指向同一份 ONNX 文件；磁盘只保留一份 rec 模型，删除两份冗余拷贝（2 × 21,159,378 B ≈ 42.3 MB）；运行时只加载一个 rec session
- 使用场景：模型部署目录准备（`scripts/ppocrv6/setup_ppocrv6.ps1`）、模型校验（`vtrans-verify-models`）、OCR 运行时加载
- 优先级 / 版本目标：P1 / v0.2.x
- 状态：已拆解（待开发）

## 现状事实（代码与磁盘为准）

- 两份 `manifest.json`（`src-tauri/resources/models/` 与 `crates/vtrans-models/resources/`）：三槽位 `path` 分别为 `ocr/rec_ja.onnx` / `ocr/rec_en.onnx` / `ocr/rec_multi.onnx`，SHA-256 相同（`5435fd74...a24634`）、size 相同（21159378），id 分别为 `ppocr-rec-v6` / `ppocr-rec-v6-en` / `ppocr-rec-v6-multi`
- 磁盘 `src-tauri/resources/models/ocr/` 存在三份物理拷贝（各 21,159,378 B）
- `crates/vtrans-ocr/src/provider.rs`（05 已合并 main）已按 **path 相等**判定共享：`rec_en_shared` / `rec_multi_shared` 时对 `rec_ja` 的 session 做 `Arc::clone`，**无需任何运行时改动**
- 冗余来源：`scripts/ppocrv6/backfill_manifest.py` 的 `targets` 把同一 `--rec` 产物复制为三个文件
- `crates/vtrans-translation/src/local_onnx.rs` 引用 rec 路径处为 `#[cfg(test)]` 合成夹具，不受影响
- `tauri.conf.json` 的 `bundle` 未声明 resources，模型不随安装包打包（打包/获取问题本次不解决，登记后续事项）
- `.gitignore` 忽略 `ocr/*`（仅放行 `ppocrv6_dict.txt`），onnx 不入库；删除两个冗余 onnx 属本地磁盘清理

## 验收标准（用户可验证）

- [ ] `src-tauri/resources/models/ocr/` 下 rec 模型只有一份文件；`rec_en.onnx` / `rec_multi.onnx` 不存在
- [ ] 两份 manifest.json 的 rec_ja / rec_en / rec_multi 三槽位 `path` 均指向同一文件，sha256 / size 一致
- [ ] `cargo run --bin vtrans-verify-models -- --models src-tauri/resources/models` 输出 `all model files are valid`
- [ ] 应用启动日志：det 加载 1 次、rec 加载 1 次（rec_en / rec_multi 共享同一 session，不再出现三个 rec session）
- [ ] 重新运行 setup / backfill 后仍只部署一份 rec（脚本可复现、幂等收敛）
- [ ] ja / en / auto OCR 语言均可用（回归冒烟）
- [ ] 文档同步：`docs/DEVELOPMENT.md` §4、`docs/modules/08-models.md`、`crates/vtrans-models/README.md`

## 涉及模块与顺序

| 序号 | 模块 | 任务类型 | 依赖 | 建议分支 | 状态 |
|------|------|----------|------|----------|------|
| 1 | 08 vtrans-models | 修改（manifest + 部署脚本 + 文档 + 磁盘清理） | — | `feat/08-rec-single-file` | 待分配 |
| 2 | 05 vtrans-ocr | 无改动（共享逻辑已存在） | — | — | 不派单 |
| 3 | 07 vtrans-translation | 无改动（引用为测试夹具） | — | — | 不派单 |

## 契约变更

- 冻结契约：**不涉及**（`ModelManifest` 属 vtrans-models，非 vtrans-core；槽位名、id、serde 表示均不变）
- IPC 契约：不涉及
- 配置 / Provider / 模型：rec 三槽位 `path` 收敛为同一文件（假设命名为 `ocr/rec.onnx`）；id 保留用于槽位语义标识；`dicts` / `preprocess_params` / translation 组不变

## 风险与假设

- 假设 1：单份文件命名为 `ocr/rec.onnx`（统一命名，避免 `rec_ja` 误导为日语专用；改动面均在本任务清单内）
- 假设 2：`rec_en.onnx` / `rec_multi.onnx` 由 08 Agent 负责从本地删除（模型文件不入 git，删除不产生 git diff，以 verify 通过为证）
- 风险：无运行时风险（05 共享逻辑已就绪）；整合时需复核 `docs/integration-report.md` 第 145 行「det/rec_ja/rec_en 三 session 加载 ~280 ms」描述并更新
- 已知限制排除：模型自动获取 / 随安装包分发（`bundle.resources` 未声明模型）不在本次范围，登记后续事项；`%APPDATA%` 模型副本行为（integration-report 遗留 7）不变

## 后续事项（本次不处理）

- 模型获取 / 校验 / 随安装包分发方案（用户明确「后面再考虑」）
