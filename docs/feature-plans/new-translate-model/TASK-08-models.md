## 模块开发说明：08 vtrans-models — 翻译模型升级 增量

### AGENT_DEV_PROMPT 参数

- MODULE_NUMBER: 08
- MODULE_NAME: vtrans-models
- MODULE_SLUG: models
- CRATE_PATH: crates/vtrans-models
- SCOPE: models
- BRANCH_NAME: feat/08-new-translate-model

### 功能上下文

- 功能目标：本地翻译模型升级为 Bergamot en→zh + CTranslate2 INT8 ja→zh 双引擎，总预算 ≤ 200 MB（见 `docs/feature-plans/new-translate-model/PLAN.md` 与接入指南）
- 本模块承担的部分：manifest schema v2（翻译段重构为双引擎）、模型下载/转换/体积审计/回填脚本（`scripts/translation/`）、资源 manifest 更新、路径解析辅助
- 上游已提供：`vtrans-core`（`Language` serde 表示不变）

### 任务要求

- 范围：仅限本模块（`crates/vtrans-models`）+ 新增 `scripts/translation/`；禁止修改其他 crate；禁止修改 vtrans-core；禁止修改 workspace 根 Cargo.toml
- Schema 变更（约束性定义，实现细节可细化但字段语义不得偏离）：
  - `SUPPORTED_MANIFEST_VERSION` 1 → 2；`ModelManifest::validate` 拒绝 v1（`UnsupportedVersion`）；OCR 段结构与字段完全不变（v1 兼容）
  - `TranslationModelGroup` 重构为双引擎结构：
    ```rust
    pub struct TranslationModels {
        pub target: String,                    // "zh-Hans"
        pub engines: TranslationEngines,
        pub budget_mb: TranslationBudget,
    }
    pub struct TranslationEngines {
        pub en_zh: BergamotModelGroup,
        pub ja_zh: CTranslate2ModelGroup,
    }
    pub struct BergamotModelGroup {
        pub engine: String,                    // "bergamot"
        pub model: ModelEntry,                 // model.enzh.intgemm.alphas.bin
        pub src_vocab: ModelEntry,             // srcvocab.enzh.spm
        pub trg_vocab: ModelEntry,             // trgvocab.enzh.spm
        pub lexical_shortlist: ModelEntry,     // lex.50.50.enzh.s2t.bin
        pub beam_size: usize,                  // 默认 1
        pub gemm_precision: String,            // "int8shiftAlphaAll"
    }
    pub struct CTranslate2ModelGroup {
        pub engine: String,                    // "ctranslate2"
        pub model: ModelEntry,                 // model.bin
        pub config: ModelEntry,                // config.json
        pub source_vocabulary: ModelEntry,     // source_vocabulary.json
        pub target_vocabulary: ModelEntry,     // target_vocabulary.json
        pub source_spm: ModelEntry,            // source.spm
        pub target_spm: ModelEntry,            // target.spm
        pub beam_size_fast: usize,             // 1
        pub beam_size_balanced: usize,         // 4
        pub max_input_tokens: usize,           // 256
    }
    pub struct TranslationBudget {
        pub hard_mb: u64,      // 200
        pub target_mb: u64,    // 175
        pub en_zh_mb: u64,     // 65
        pub ja_zh_mb: u64,     // 110
    }
    ```
  - `ModelManager` 新增辅助：按引擎返回解析后的绝对路径（模型、词表、spm、config），供 07 消费；`all_entries()` 覆盖所有新条目
- 脚本（`scripts/translation/`，方案 B 与 `scripts/ppocrv6/` 同构，Windows 优先）：
  - `fetch_firefox_enzh.py`：解析 Mozilla registry（`https://storage.googleapis.com/moz-fx-translations-data--303e-prod-translations-data/db/models.json`），锁定 en-zh Release `base-memory` 模型，下载并校验 SHA-256，输出下载清单
  - `convert_ja_zh_ct2.ps1`（Windows PowerShell，替代指南中的 .sh）：下载 `shun89/opus-mt-ja-zh` → `ct2-transformers-converter` INT8 转换 → 目录体积实测（锁 `ctranslate2==4.8.1`）
  - `audit_model_sizes.py`：200 MB 门禁（en-zh ≤ 65、ja-zh ≤ 110、总 ≤ 200/目标 175），超限非零退出
  - `backfill_translation_manifest.py`（或 ps1）：实测 SHA-256 / size_bytes 回填 `src-tauri/resources/models/manifest.json` 的 translation 段
  - 总入口 `setup_translation_models.ps1`：下载 → 转换 → 体积审计 → 回填，可复现全流程
  - 锁定版本与 revision 写入脚本常量与生成的 manifest（`model_revision` / `converted_with` / `registry_generated` 等元数据字段，schema 可加 `metadata: HashMap<String,String>`）
- 资源 manifest：`src-tauri/resources/models/manifest.json` 更新为 v2（OCR 段原样保留；translation 段按上述结构，真实 sha256 由脚本回填后提交；模型二进制不入库——`.gitignore` 已忽略 `translation/*`）
- 约束（非实现代码）：
  - 必须保留 `ModelEntry{id, path, sha256, size_bytes}` 语义，新引擎条目全部走 SHA-256 校验
  - 不得把模型文件、SPM、词表提交 Git
  - manifest 路径约定：`translation/en-zh/`、`translation/ja-zh/`（与指南 §29 一致）
- 测试要求：
  - v2 manifest 解析/序列化往返；v1 manifest 拒绝（`UnsupportedVersion(1)`）
  - 新条目 `all_entries()` / 路径解析辅助单测；缺失文件返回 `FileNotFound`
  - `audit_model_sizes.py` 对合成目录的通过/超限行为（脚本自测或 CI 说明）
  - 既有 OCR 段测试全量回归
- 文档要求：crate README（manifest v2 说明、脚本用法）；`docs/modules/08-models.md` 同步（新 schema、脚本、模型清单 v0.3.0）；`docs/DEVELOPMENT.md` 开发机要求补充（Python 3.10+、CTranslate2 4.8.1、CMake/MSVC 用于 07，下载转换需网络）

### 横切标准提醒

- 日志：下载/转换/校验进度 `info!`；失败 `warn!`/`error!`；不记录 URL 查询参数中的敏感信息（无凭据场景）
- 错误：复用 `ModelError`（`Parse` / `FileNotFound` / `HashMismatch` / `UnsupportedVersion`）；新增校验类错误优先复用 `Parse`/`Validation` 语义，不随意加变体；如确需新增须在 PR 说明
- 测试与风格：fmt / clippy 零警告；脚本带 `-h` 用法与退出码说明

### 完成定义（DoD）

- [ ] 质量门禁通过：`cargo fmt --all -- --check`；`cargo clippy -p vtrans-models --all-targets`；`cargo test -p vtrans-models`
- [ ] `scripts/translation/` 全流程可复现（下载→转换→审计→回填），体积门禁生效
- [ ] 资源 manifest.json 为 v2（真实 sha256 回填），模型二进制未入库
- [ ] 未修改其他 crate 与 vtrans-core
- [ ] PR 描述含 schema 说明、脚本用法、验收 checklist
