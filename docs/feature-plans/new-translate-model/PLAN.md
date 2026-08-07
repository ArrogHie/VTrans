# 功能开发计划：翻译模型升级（离线 en→zh / ja→zh）+ OCR 语言与源语言统一

## 概述

- 需求来源：用户 2026-08-07 提出；参考 `docs/feature-plans/new-translate-model/英中_日中_轻量离线翻译_Rust_TS_接入指南.md`（已入库）与 `compact-translation-en-zh-ja-zh-starter/` 骨架包
- 功能目标：本地翻译模型从「opus-mt-en-zh-int8 ONNX（约 403 MB，仅 en→zh）」升级为「Bergamot en→zh + CTranslate2 INT8 ja→zh（翻译模型总预算硬门槛 ≤200 MB）」；同时将 OCR 语言与翻译源语言拆为两个独立设置项并强制统一（改动任一自动同步另一项）
- 使用场景：单次框选翻译与实时区域翻译的本地翻译路径；主窗口「语言与引擎」设置区；安装包模型体积与 CI 体积门禁
- 优先级 / 版本目标：P1 / 建议 v0.3.0
- 状态：开发中（02 已整合；A1–A4 待用户确认，不影响 02 结果）

## 验收标准（用户可验证）

- [ ] 设置中「OCR 语言」与「源语言」是两个独立下拉项；改动任一，另一项自动跟随，二者（含 `auto`）始终一致；整包保存时不一致会被拒绝
- [ ] 本地模型支持离线 en→zh 与 ja→zh：英文截图翻译质量不劣化（固定回归集），日文截图可离线翻译（新增固定样本回归）
- [ ] 本地翻译模型总大小 ≤ 200 MB（CI 门禁），目标 ≤ 175 MB（en-zh ≤ 65 MB、ja-zh ≤ 110 MB，以实测为准）
- [ ] 应用启动加载本地模型时按 manifest 校验 SHA-256，模型缺失/不匹配给出明确错误，不静默回退 API
- [ ] OCR 语言为 `auto` 时，翻译按 OCR 检测结果路由（日文检测 → ja→zh；英文检测 → en→zh）；无检测结果时按 Unicode heuristic 兜底
- [ ] （待确认 A1）UI 提供翻译质量档位 Fast / Balanced，选择后对本地模型 beam 参数生效并持久化
- [ ] 仓库内脚本可复现「下载 → 转换 → 体积审计 → SHA-256 回填 manifest」全流程（开发机需 Python + C++ 工具链，见 DEVELOPMENT.md）
- [ ] 旧的 403 MB ONNX 模型不再被 manifest 引用、不再随发布分发
- [ ] 质量门禁全绿：`cargo fmt` / `cargo clippy --workspace --all-targets` / `cargo test --workspace` / `pnpm test` / `pnpm exec tsc --noEmit`；日志无敏感数据

## 现状与目标差距

| 维度 | 现状（代码为准） | 目标 |
|------|-----------------|------|
| 本地翻译引擎 | `LocalTranslationProvider`（`local_onnx.rs`）：单 ONNX 模型 + HF `tokenizer.json`，`id = "local-onnx"` | Native 多引擎 Provider：Bergamot（en-zh）+ CTranslate2（ja-zh）+ SentencePiece，`id` 待确认（A2，建议 `"local-native"`） |
| 本地模型 | `opus-mt-en-zh-int8` ONNX 403,368,390 B，仅 `en→zh-CN` | en-zh ≤ 65 MB + ja-zh ≤ 110 MB，共 ≤ 200 MB，支持 `en→zh-CN` / `ja→zh-CN` |
| manifest | v1：`translation` 为单 `TranslationModelGroup{model, tokenizer, ...}` | v2（破坏性，A4）：`translation` 重构为双引擎条目（各自模型/词表/参数 + 体积预算） |
| 配置 | `ocr.language` 与 `translation.source_language` 两字段分开、无联动；config version 3 | 两字段独立设置但强制一致；新增 `translation.quality`（A1）；config version 4 + 迁移 |
| 语言路由 | `source=auto` 时仅做日文标点规范化，翻译请求仍带 `Auto`（本地 Provider 拒绝 Auto） | OCR 后按 `detected_language` / Unicode heuristic 解析出具体源语言再翻译 |
| 文本分块 | `MAX_TRANSLATION_CHUNK_CHARS=2000` 字符硬切 | 标点感知切分 + 语言相关字符预算（对齐新模型 `max_input_tokens=256`） |
| 前端 | 两个下拉无联动；提示「本地模型仅支持 en→zh-CN」 | 下拉双向联动；提示更新为「支持 en/ja → zh-CN」；质量档位 UI（A1） |
| 打包 | `bundle.resources` 未声明 native 依赖 | native `translation_bridge.dll` 随安装包分发（10 维护 tauri.conf.json） |
| 许可证 | 未登记翻译引擎许可证 | `licenses/` 登记 Bergamot MPL-2.0、CTranslate2 MIT、MarianMT/SentencePiece Apache-2.0 |

## 涉及模块与顺序

| 序号 | 模块 | 任务类型 | 依赖 | 建议分支 | 状态 |
|------|------|----------|------|----------|------|
| 1 | 02 vtrans-config | 修改（schema v4 + 联动校验 + 迁移） | — | `feat/02-new-translate-model` | 已整合 |
| 2 | 08 vtrans-models | 修改（manifest v2 + 下载/转换/审计脚本 + 文档） | — | `feat/08-new-translate-model` | 待分配 |
| 3 | 07 vtrans-translation | 主责重构（native C++ bridge + FFI + Native Provider + 验证 CLI + 许可证） | 依赖 1（quality 字段）、2（manifest v2） | `feat/07-new-translate-model` | 待分配 |
| 4 | 09 vtrans-pipeline | 修改（auto 源路由 + 标点感知分块） | 依赖 core 契约（可并行，端到端验证依赖 3） | `feat/09-new-translate-model` | 待分配 |
| 5 | 10 vtrans-app | 修改（语言联动命令 + Provider 组装 + 打包资源 + Provider id 契约） | 依赖 1、3、4 | `feat/10-new-translate-model` | 待分配 |
| 6 | 11 frontend | 修改（联动 UI + 本地能力提示 + 质量档位 UI + 测试） | 依赖 5（Provider id）、1（schema） | `feat/11-new-translate-model` | 待分配 |
| 7 | 整合（协调者） | 合并 + workspace 门禁 + 端到端 + 报告 | 依赖 1–6 | main 上整合 | 待整合 |

排除项（不拆任务）：

- 01 vtrans-core：冻结契约零改动。`Language`（auto/zh-CN/ja/en）已覆盖新模型语言对；`TranslationProvider` / `TranslationRequest` / `TranslationResult` / `TranslationError` 全部不变；错误码映射复用现有变体（`UnsupportedPair` / `ModelLoad` / `Inference` / `Cancelled` 等），不新增变体
- 03 vtrans-security：无新凭据
- 04 vtrans-capture：不涉及
- 06 vtrans-text：标点感知分块归 09 pipeline（翻译链路专属），06 现有 `split_paragraphs` 不动

## 契约变更

### 冻结契约（vtrans-core）

**不涉及。** 新增 `SourceLanguage`/`TranslationService` 之类的指南建议类型**不采纳**——现有 `Language` + `TranslationProvider` 契约已等价，避免无谓的冻结契约变更。本功能所有跨模块通信仍走既有类型。

### vtrans-models schema（08 内部契约，破坏性，评审后通知 07/10）

- `SUPPORTED_MANIFEST_VERSION` 1 → 2；`ModelManifest::validate` 拒绝 v1
- `TranslationModelGroup` 重构为双引擎结构（约束性定义，08 可细化）：
  - `translation.target: String`（`"zh-Hans"`）
  - `translation.engines.en_zh`：`engine="bergamot"`，含 `model` / `src_vocab` / `trg_vocab` / `lexical_shortlist`（均 `ModelEntry` 语义：path/sha256/size_bytes）+ `beam_size`（默认 1）+ `gemm_precision`（`"int8shiftAlphaAll"`）
  - `translation.engines.ja_zh`：`engine="ctranslate2"`，含 `model` / `config` / `source_vocabulary` / `target_vocabulary` / `source_spm` / `target_spm` + `beam_size_fast`（1）/ `beam_size_balanced`（4）/ `max_input_tokens`（256）
  - `translation.budget_mb`：`hard=200`、`target=175`、`en_zh=65`、`ja_zh=110`
- 新增 `ModelManager` 辅助：按引擎返回解析后的绝对路径（模型、词表、spm）
- 文件布局（`.gitignore` 已忽略 `translation/*`，模型不入库）：
  - `models/translation/en-zh/`：`model.enzh.intgemm.alphas.bin`、`srcvocab.enzh.spm`、`trgvocab.enzh.spm`、`lex.50.50.enzh.s2t.bin`、`manifest.json`
  - `models/translation/ja-zh/`：`model.bin`、`config.json`、`source_vocabulary.json`、`target_vocabulary.json`、`source.spm`、`target.spm`、`manifest.json`

### vtrans-config（02 内部契约）

- `CURRENT_CONFIG_VERSION` 3 → 4；迁移 v3→v4：新增 `translation.quality`（默认 `"fast"`），并强制 `translation.source_language = ocr.language`（以 OCR 语言为权威，迁移时同步）
- 校验规则新增：`ocr.language != translation.source_language` → `ConfigError::Validation`（跨字段一致性）
- 新增字段：`translation.quality: String`（`"fast" | "balanced"`，默认 `"fast"`；校验非法值拒绝）

### IPC 契约（10 + 11 两端）

- **无新增 Command / Event**
- 行为变更：`set_ocr_language` 与 `set_source_language` 改为双向联动（各写两个配置字段）；前端 `tauri.ts` 封装签名不变
- `AppStatus.translation_provider` 实现 id：待确认 A2（建议 `"local-onnx"` → `"local-native"`；若改，必须同步 `src/types/index.ts` 的 `normalizeProviderId` 与 `crates/vtrans-app/README.md`、`tests/contracts.rs`）
- 配置标识符白名单 `"api" / "local"` 不变

### 配置 / Provider / 模型 / 打包

- Provider id：`"api"` 不变；本地实现 id 见 A2
- 语言对：本地 `(en, zh-CN)` + `(ja, zh-CN)`；`zh-CN` 源语言本地不支持（API 仍可），UI 提示
- 新增脚本目录 `scripts/translation/`（下载/转换/体积审计/manifest 回填，方案 B 与 `scripts/ppocrv6/` 同构）
- 打包：`src-tauri/tauri.conf.json` 的 `bundle.resources` 增加 native `translation_bridge.dll`（由 10 维护；07 产出 dll 到约定目录）
- 运行时模型目录不变：`model_dir`（默认 `app_data_dir/models`），`translation/` 子目录随 manifest 更新

## 风险与假设

### 假设（待用户确认）

- A1：翻译质量档位（Fast / Balanced）**纳入本次**（指南 §7 明确建议 UI 提供，配置模板已含两档参数）；若裁剪则 02/10/11 三处同步缩小范围
- A2：本地 Provider 运行时 id 从 `"local-onnx"` **改为 `"local-native"`**（语义准确；两端契约同步在本功能范围内）；若希望最小改动可保持 `"local-onnx"` 不变
- A3：**删除**旧 ONNX 单模型路径（`local_onnx.rs` 及其测试），指南已明确弃用「ONNX 自回归 decoder」路线，不保留双维护
- A4：manifest **v2 为破坏性升级**（v1 翻译段不再支持），与「模型整体替换」一致；OCR 段结构不变（v1 兼容字段保留）

### 风险

- B1：Bergamot v0.4.5（MPL-2.0）+ CTranslate2 4.8.1（MIT）+ SentencePiece（Apache-2.0）Windows 原生构建复杂（CMake + MSVC + 第三方源码/二进制），开发机需新增 C++ 工具链；必须锁版本并记录构建步骤
- B2：ja-zh INT8 实际体积为预估（85–110 MB），以实测目录大小为准；总预算硬门槛 200 MB，超限必须重新量化/裁剪
- B3：CTranslate2 / Bergamot 原生调用是长阻塞且不可中断，取消语义退化为「调用前后检查 + 新任务等待/丢弃」（与现有 ONNX `terminate()` 不同），需在 07 README 登记
- B4：Mozilla 模型 registry 持续更新 → 下载脚本必须冻结 revision + SHA-256，运行时只读 manifest，禁止跟随 latest
- B5：CPU oversubscription → 线程上限约束（OCR det/rec 各 2、Bergamot 2、CTranslate2 intra 2 / inter 1），07 实现时不得放开为「全核」
- B6：许可证合规（MPL-2.0 引擎 + Apache-2.0 模型）需在 `licenses/` 登记 NOTICE；正式商业发布前需法律复核（指南 §24），本次仅完成登记
- B7：`docs/integration-report.md` 实际不存在（角色设定文档与仓库现状不一致）；本项目按 `docs/feature-plans/*/INTEGRATION.md` 组织整合报告，本次沿用该组织方式

## 实施顺序

1. 用户确认 A1–A4 假设（不影响 02/08 起步；A1 影响 02/10/11 范围，A2 影响 10/11，A3 影响 07 范围，A4 影响 08）
2. 02 与 08 并行开发（层级 1，互不依赖），先后合并到 main
3. 07 从 main 拉分支（依赖 02 quality 字段 + 08 manifest v2），09 可同时从 main 拉分支（仅依赖 core 契约）
4. 07 / 09 合并 → 10（联动命令 + Provider 组装 + 打包资源）→ 11（联动 UI + 提示 + 质量档位）
5. 整合：workspace 门禁 → 真实模型端到端冒烟 → 回归 → INTEGRATION.md → 台账关闭

## 已知限制对照

- 现有 `docs/feature-plans/*/INTEGRATION.md` 中登记的已知限制（日文 OCR 质量、长行分片等）不受本功能影响
- 本功能新增已知限制（登记于各任务单 README / 整合报告）：B3 取消语义、B2 体积实测口径、ja-zh 质量基准（首次建立）
