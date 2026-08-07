# 模块 08：vtrans-models 模型管理

| 属性 | 值 |
|------|-----|
| Crate | `vtrans-models` |
| 分支 | `feat/08-new-translate-model` |
| 上游依赖 | `vtrans-core` |
| 层级 | 1 |
| 复杂度 | 中 |
| 阶段 | Phase 1（v0.3.0 增量：翻译模型升级） |

## 职责

管理 OCR 和翻译模型的清单定义（manifest v2）、完整性校验、生命周期和路径解析。模型文件不提交 Git，通过 manifest 和下载脚本管理。

## 公开 API

```rust
/// 模型清单（manifest v2，A4 破坏性升级：v1 翻译段不再支持）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelManifest {
    pub version: u32,                          // 当前仅支持 2
    pub ocr: OcrModelGroup,                    // v1 兼容，结构不变
    pub translation: Option<TranslationModels>,// null 表示无本地翻译
}

pub struct OcrModelGroup {
    pub det: ModelEntry,                       // 文本检测模型
    pub rec_ja: ModelEntry,                    // 日文识别模型
    pub rec_en: ModelEntry,                    // 英文识别模型
    pub rec_multi: Option<ModelEntry>,         // 多语言识别（可选）
    pub dicts: HashMap<String, PathBuf>,       // 字典文件
    pub preprocess_params: PreprocessParams,
}

/// 双引擎翻译组
pub struct TranslationModels {
    pub target: String,                        // "zh-Hans"
    pub engines: TranslationEngines,           // en_zh + ja_zh
    pub budget_mb: TranslationBudget,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,     // 溯源信息（脚本回填）
}

pub struct TranslationEngines {
    pub en_zh: BergamotModelGroup,
    pub ja_zh: CTranslate2ModelGroup,
}

pub struct BergamotModelGroup {
    pub engine: String,                        // "bergamot"
    pub model: ModelEntry,                     // model.enzh.intgemm.alphas.bin
    pub src_vocab: ModelEntry,                 // srcvocab.enzh.spm
    pub trg_vocab: ModelEntry,                 // trgvocab.enzh.spm
    pub lexical_shortlist: ModelEntry,         // lex.50.50.enzh.s2t.bin
    pub beam_size: usize,                      // 默认 1
    pub gemm_precision: String,                // "int8shiftAlphaAll"
}

pub struct CTranslate2ModelGroup {
    pub engine: String,                        // "ctranslate2"
    pub model: ModelEntry,                     // model.bin
    pub config: ModelEntry,                    // config.json
    pub source_vocabulary: ModelEntry,         // source_vocabulary.json
    pub target_vocabulary: ModelEntry,         // target_vocabulary.json
    pub source_spm: ModelEntry,                // source.spm
    pub target_spm: ModelEntry,                // target.spm
    pub beam_size_fast: usize,                 // 1
    pub beam_size_balanced: usize,             // 4
    pub max_input_tokens: usize,               // 256
}

pub struct TranslationBudget {
    pub hard_mb: u64,                          // 200
    pub target_mb: u64,                        // 175
    pub en_zh_mb: u64,                         // 65
    pub ja_zh_mb: u64,                         // 110
}

pub struct ModelEntry {
    pub id: String,
    pub path: PathBuf,                         // 相对于 models/ 目录
    pub sha256: String,
    pub size_bytes: u64,
}

pub struct PreprocessParams { /* v1 兼容，见「PP-OCRv6 Small 模型清单」 */ }

/// 遗留推理参数（v1 兼容保留；v2 翻译参数按引擎存放）
pub struct InferenceParams {
    pub max_batch_size: usize,
    pub num_beams: usize,
}

/// 模型管理器
pub struct ModelManager { /* ... */ }

impl ModelManager {
    pub fn from_manifest_dir(dir: &Path) -> Result<Self, ModelError>;
    pub fn manifest(&self) -> &ModelManifest;
    pub fn verify_integrity(&self) -> Result<VerifyReport, ModelError>;
    pub fn model_path(&self, entry: &ModelEntry) -> PathBuf;
    /// Bergamot en→zh 引擎的绝对路径（模型、双词表、短表），供 07 消费
    pub fn en_zh_paths(&self) -> Option<BergamotPaths>;
    /// CTranslate2 ja→zh 引擎的绝对路径（模型、config、双词表、双 SPM），供 07 消费
    pub fn ja_zh_paths(&self) -> Option<CTranslate2Paths>;
    pub fn load_progress(&self) -> Option<f32>;
}

pub struct VerifyReport {
    pub checked: usize,
    pub passed: usize,
    pub failed: Vec<String>,
}
```

## 错误类型

```rust
#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("manifest not found at {0}")]
    ManifestNotFound(PathBuf),
    #[error("manifest parse error: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("model file not found: {0}")]
    FileNotFound(PathBuf),
    #[error("sha256 mismatch for {id}: expected {expected}, got {actual}")]
    HashMismatch { id: String, expected: String, actual: String },
    #[error("unsupported manifest version: {0}")]
    UnsupportedVersion(u32),
    /// 规格外补充：文件存在但读取失败（权限等）
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
```

校验类错误复用 `Parse`（JSON/字段缺失）与 `UnsupportedVersion`（版本），不新增变体；`UnsupportedVersion(1)` 表示 v1 manifest 被 v2 拒绝（A4）。

## 内部文件结构

```text
crates/vtrans-models/
├── Cargo.toml
├── README.md
├── src/
│   ├── lib.rs          # re-export
│   ├── manifest.rs     # ModelManifest v2 schema
│   ├── manager.rs      # ModelManager 实现（含按引擎路径辅助）
│   ├── verify.rs       # SHA-256 校验
│   ├── path.rs         # 路径解析 + BergamotPaths / CTranslate2Paths
│   └── bin/verify_models.rs   # 完整性校验 CLI
├── resources/
│   └── manifest.json   # 默认清单模板（v2，与 src-tauri 同步回填）
└── tests/
    ├── integrity.rs    # 批量校验集成测试（v2）
    └── fixtures/

scripts/translation/          # 翻译模型准备脚本（方案 B，Windows 优先）
├── fetch_firefox_enzh.py     # Mozilla registry → en-zh Release base-memory 下载 + SHA-256 校验
├── convert_ja_zh_ct2.ps1     # shun89/opus-mt-ja-zh → CTranslate2 INT8（锁 4.8.1）+ 体积实测
├── audit_model_sizes.py      # 200 MB 门禁（en-zh 65 / ja-zh 110 / 总 200，目标 175）
├── backfill_translation_manifest.py  # SHA-256 / size_bytes 回填 translation 段
└── setup_translation_models.ps1      # 总入口：下载 → 转换 → 审计 → 回填
```

## 测试计划

| 测试项 | 类型 | 说明 |
|--------|------|------|
| v2 manifest 解析 | 单元 | 双引擎 JSON 正确反序列化（含 budget、metadata） |
| v2 序列化往返 | 单元 | to_string → from_json_str 结果一致；空 metadata 不序列化 |
| v1 manifest 拒绝 | 单元/集成 | `UnsupportedVersion(1)`（含无 translation 段的 v1） |
| 缺失字段 | 单元 | 必填字段缺失返回 Parse 错误 |
| SHA-256 校验 | 单元 | 匹配返回 Ok，不匹配返回 HashMismatch（含翻译条目） |
| 文件不存在 | 单元/集成 | OCR 与翻译条目缺失均返回 FileNotFound |
| 路径解析 | 单元 | 相对路径正确解析为绝对路径 |
| 按引擎路径辅助 | 单元/集成 | `en_zh_paths` / `ja_zh_paths` 覆盖全部 10 个条目；无 translation 时返回 None |
| VerifyReport | 集成 | 多文件批量校验结果汇总（OCR + 双引擎 + dicts） |
| OCR 段回归 | 单元/集成 | v4 缺省字段取 v6 默认值、rec 三槽位共享单文件 |
| audit_model_sizes.py | 脚本自测 | `--self-test` 对合成目录的通过/超限行为（见下） |

体积审计自测：

```powershell
python scripts\translation\audit_model_sizes.py --self-test
```

自测构造两个合成目录（预算内 / 超限）验证门禁行为，全部符合预期才退出 0；可在 CI 中作为门禁自身的冒烟测试。

## 验收标准

- [x] manifest.json 为 v2 且可正确解析（OCR 段 v1 兼容）
- [x] SHA-256 校验功能正常（新引擎条目全部走校验）
- [x] 缺失文件返回明确错误（`FileNotFound`）
- [x] v1 manifest 被拒绝（`UnsupportedVersion(1)`）
- [x] 按引擎路径辅助（`en_zh_paths` / `ja_zh_paths`）可用
- [x] 提供 manifest.json 模板（crate 内 + src-tauri 资源）
- [x] 模型文件不提交 Git（.gitignore 忽略 `translation/*`）
- [x] `scripts/translation/` 全流程可复现（下载 → 转换 → 审计 → 回填）
- [x] README.md 完整

## 开发注意事项

- manifest.json 位于 `src-tauri/resources/models/` 目录（`crates/vtrans-models/resources/manifest.json` 为同内容模板，回填脚本同步更新）
- SHA-256 使用 sha2 crate，8 KiB 分块流式计算
- 模型路径在 manifest 中用相对路径，运行时解析为绝对路径
- .gitignore 排除 `*.onnx`、`*.bin` 与 `src-tauri/resources/models/translation/*`
- 翻译模型布局（与接入指南 §29 一致）：
  - `translation/en-zh/`：`model.enzh.intgemm.alphas.bin`、`srcvocab.enzh.spm`、`trgvocab.enzh.spm`、`lex.50.50.enzh.s2t.bin`
  - `translation/ja-zh/`：`model.bin`、`config.json`、`source_vocabulary.json`、`target_vocabulary.json`、`source.spm`、`target.spm`
- ja-zh 的 Marian 模型共享词表：转换器只产出 `shared_vocabulary.json`，脚本按 schema 复制为 source/target 两份词表（内容相同）
- 溯源锁定（B4）：registry `generated` 时间、模型 revision 目录、HF commit、`converted_with` 均写入 manifest metadata；registry/HF 漂移后必须重跑脚本回填

## PP-OCRv6 Small 模型清单（v0.2.0，v1/v2 通用）

OCR 段结构与字段在 v1 → v2 中完全不变。自 v0.2.0 起 OCR 检测/识别模型为 PP-OCRv6 Small。

### 模型条目

| 槽位 | id | 文件 | SHA-256 | size_bytes |
|------|----|------|---------|------------|
| det | `ppocr-det-v6` | `ocr/det.onnx` | `d73e0058...c9410e` | 9880512 |
| rec_ja | `ppocr-rec-v6` | `ocr/rec.onnx` | `5435fd74...a24634` | 21159378 |
| rec_en | `ppocr-rec-v6-en` | `ocr/rec.onnx` | 同上（同一 v6 rec） | 21159378 |
| rec_multi | `ppocr-rec-v6-multi` | `ocr/rec.onnx` | 同上（同一 v6 rec） | 21159378 |

rec_ja / rec_en / rec_multi 三槽位共享同一份 `PP-OCRv6_small_rec` ONNX（磁盘仅 `ocr/rec.onnx` 一份）。

### 字典

`ocr/ppocrv6_dict.txt`（18708 行，SHA-256 `b5f2bfe2...e401c5d`）。三个语言槽位（`ja` / `en` / `auto`）指向同一文件。

### preprocess_params（v6 默认值）

| 字段 | 值 | 说明 |
|------|-----|------|
| `image_size` | `[640, 640]` | 检测输入上限 |
| `mean` / `std` | ImageNet 均值/方差 | BGR 通道顺序 |
| `det_threshold` | 0.2 | DB 二值化阈值 |
| `unclip_ratio` | 1.4 | 外扩系数 |
| `box_threshold` | 0.45 | 框置信度过滤（缺省默认） |
| `max_candidates` | 3000 | 最大候选框数（缺省默认） |
| `min_box_size` | 3.0 | 最短边过滤（缺省默认） |
| `rec_input_height` | 48 | 识别输入高（缺省默认） |
| `rec_input_width` | 320 | 识别输入宽（缺省默认） |
| `rec_append_space` | true | 类别表追加空格（缺省默认） |
| `rec_blank_index` | 0 | CTC blank 索引（缺省默认） |

v4 时代的旧 manifest（无上述可选字段）仍可反序列化，缺省字段自动取 v6 默认值（OCR 段向后兼容）。

## 翻译模型清单（v0.3.0，manifest v2）

### en-zh（Bergamot / Mozilla Release base-memory）

registry 生成时间：`2026-08-07T00:43:32Z`；revision：`llmaat_finetune10M_qe8_f2_ByQcSxGXQRqGi-UTxYE43g`。

| 槽位 | id | 文件 | SHA-256 | size_bytes |
|------|----|------|---------|------------|
| model | `enzh-model` | `translation/en-zh/model.enzh.intgemm.alphas.bin` | `4e5accc1...c2157c`（registry 官方 uncompressedHash） | 43849787 |
| src_vocab | `enzh-src-vocab` | `translation/en-zh/srcvocab.enzh.spm` | `bd9b6550...97c8c5`（下载实测） | 806952 |
| trg_vocab | `enzh-trg-vocab` | `translation/en-zh/trgvocab.enzh.spm` | `aded6993...adf223d`（下载实测） | 772004 |
| lexical_shortlist | `enzh-lexical-shortlist` | `translation/en-zh/lex.50.50.enzh.s2t.bin` | `8575d8da...22f681`（下载实测） | 4485184 |

参数：`beam_size=1`、`gemm_precision="int8shiftAlphaAll"`。包体约 49.91 MB。

### ja-zh（CTranslate2 INT8 / shun89/opus-mt-ja-zh）

HF revision：`0728b51b9be02330f7bce262a4d47f611fd3a2a4`；转换工具：`ctranslate2 4.8.1`（INT8）。实测包体约 85.05 MB（≤110 MB 预算）。

| 槽位 | id | 文件 | SHA-256 | size_bytes |
|------|----|------|---------|------------|
| model | `jazh-model` | `translation/ja-zh/model.bin` | `76cf3986...39ea8e` | 79567635 |
| config | `jazh-config` | `translation/ja-zh/config.json` | `72901fbd...270e7ee` | 233 |
| source_vocabulary | `jazh-source-vocabulary` | `translation/ja-zh/source_vocabulary.json` | `32f4aa94...dc76b1` | 1427305 |
| target_vocabulary | `jazh-target-vocabulary` | `translation/ja-zh/target_vocabulary.json` | `32f4aa94...dc76b1`（与 source 相同） | 1427305 |
| source_spm | `jazh-source-spm` | `translation/ja-zh/source.spm` | `25103859...d63289` | 1312134 |
| target_spm | `jazh-target-spm` | `translation/ja-zh/target.spm` | `fe81460c...79c56e` | 1312134 |

参数：`beam_size_fast=1`、`beam_size_balanced=4`、`max_input_tokens=256`。

### 体积预算（实测）

| 项 | 实测 | 预算 |
|----|------|------|
| en-zh | 49.91 MB | ≤ 65 MB |
| ja-zh | 85.05 MB | ≤ 110 MB |
| 合计 | 134.96 MB | 目标 ≤ 175 / 硬门槛 ≤ 200 MB |

`src-tauri/resources/models/manifest.json` 中 `translation.budget_mb` 与审计脚本默认值一致；超限时 `audit_model_sizes.py` 非零退出。

## 脚本（方案 B）

`scripts/translation/setup_translation_models.ps1` 提供「下载 → 转换 → 审计 → 回填」全流程；分步脚本与用法见 `crates/vtrans-models/README.md` 第 6 节。开发机要求见 `docs/DEVELOPMENT.md` §4。

所有脚本支持 `-h` / `--help` 用法说明，退出码语义：0 成功、1 失败、2 用法错误（PowerShell 脚本为 0 成功 / 非 0 失败）。

## 向后兼容

manifest schema version 1 → 2 为**破坏性升级**（A4 已确认）：v1 的 `translation` 单 ONNX 段不再支持，v1 manifest（含无 translation 段）被 `UnsupportedVersion(1)` 拒绝。OCR 段结构与字段完全不变（v1 兼容）；`PreprocessParams` 缺省字段、`InferenceParams` 类型保留供 v1 时代代码兼容。
