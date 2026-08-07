# vtrans-models

模型管理模块：负责 OCR 与翻译模型的清单（manifest v2）定义、SHA-256 完整性校验、按引擎的路径解析和加载进度状态。

## 1. 模块概述

本模块管理模型文件的生命周期元数据：解析 `manifest.json`、校验模型文件存在且 SHA-256 匹配、把相对路径解析为运行时绝对路径（含按引擎聚合的路径辅助，供 `vtrans-translation` 的 native bridge 消费），并保存加载进度状态。

边界：本模块做清单 schema 解析与校验、批量完整性校验、路径解析、加载进度状态存取；不做 ONNX / Bergamot / CTranslate2 推理（`vtrans-ocr` / `vtrans-translation`）、不下载模型文件（`scripts/translation/setup_translation_models.ps1`）、不管理模型安装布局（`src-tauri/resources/models/`）。
本模块不持有文件句柄、不启动线程，是纯同步库模块；取消与后台执行由消费方编排。

自 v0.3.0 起 manifest schema 升级为 v2（破坏性，A4 已确认）：OCR 段结构完全不变（v1 兼容，PP-OCRv6 Small det + rec），translation 段从单 ONNX 模型 + tokenizer 重构为双引擎结构：

- en_zh：Bergamot（Marian），含模型、source/target SPM 词表、词汇短表
- ja_zh：CTranslate2 INT8，含模型、config、source/target 词表、source/target SPM
- budget_mb：hard 200 / target 175 / en_zh 65 / ja_zh 110（与体积审计脚本门禁一致）
- 新增可选 metadata：model_revision / converted_with / registry_generated 等溯源信息

v1 manifest（含无 translation 段的）一律被 ModelManifest::validate 拒绝，错误为 UnsupportedVersion(1)。

## 2. 依赖关系

上游 crate：vtrans-core（无直接类型依赖；v2 翻译段不再使用 Language 序列化语言对）。

外部 crate：serde + serde_json、thiserror、tracing、sha2；dev 依赖 tempfile。

下游消费方：vtrans-ocr（OcrModelGroup / ModelEntry / model_path / verify_integrity）、vtrans-translation（双引擎清单与 en_zh_paths / ja_zh_paths）、vtrans-app（ModelManager 装配与校验报告）。

## 3. 快速上手

最小可用示例（自建临时目录并生成 v2 manifest；真实项目由下载脚本准备文件）：

```rust
use std::path::Path;
use sha2::{Digest, Sha256};
use vtrans_models::{ModelError, ModelManager};

fn write_model(dir: &Path, rel: &str, data: &[u8]) -> String {
    let full = dir.join(rel);
    std::fs::create_dir_all(full.parent().expect("path has parent")).unwrap();
    std::fs::write(&full, data).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

fn main() -> Result<(), ModelError> {
    let dir = std::env::temp_dir().join("vtrans-models-demo");
    let det_sha = write_model(&dir, "ocr/det.onnx", b"det-model");
    let rec_sha = write_model(&dir, "ocr/rec.onnx", b"rec-model");
    let manifest = format!(
        r#"{{
  "version": 2,
  "ocr": {{
    "det": {{ "id": "det", "path": "ocr/det.onnx", "sha256": "{det_sha}", "size_bytes": 9 }},
    "rec_ja": {{ "id": "ppocr-rec-v6", "path": "ocr/rec.onnx", "sha256": "{rec_sha}", "size_bytes": 9 }},
    "rec_en": {{ "id": "ppocr-rec-v6-en", "path": "ocr/rec.onnx", "sha256": "{rec_sha}", "size_bytes": 9 }},
    "rec_multi": {{ "id": "ppocr-rec-v6-multi", "path": "ocr/rec.onnx", "sha256": "{rec_sha}", "size_bytes": 9 }},
    "dicts": {{}},
    "preprocess_params": {{ "image_size": [640, 640], "mean": [0.485, 0.456, 0.406], "std": [0.229, 0.224, 0.225], "det_threshold": 0.2, "unclip_ratio": 1.4 }}
  }},
  "translation": null
}}"#
    );
    std::fs::write(dir.join("manifest.json"), manifest).unwrap();
    let manager = ModelManager::from_manifest_dir(&dir)?;
    let report = manager.verify_integrity()?;
    if !report.is_ok() {
        for failure in &report.failed {
            eprintln!("{failure}");
        }
        return Err(ModelError::FileNotFound(Path::new("models").to_path_buf()));
    }
    let det_path = manager.model_path(&manager.manifest().ocr.det);
    println!("det model: {}", det_path.display());
    Ok(())
}
```

生命周期：ModelManager 由消费方创建并持有，内部只保存 manifest 与目录路径；verify_integrity 逐文件打开、校验后立即关闭，不保留句柄。

## 4. 公开 API 概要

所有类型在 crate 根重新导出，也可从子模块导入。

| 类型 | 用途 |
|------|------|
| ModelManifest | 根清单：版本 + OCR 组 + 可选双引擎翻译组 |
| OcrModelGroup | OCR 模型组（det / rec 三槽位 / dicts / preprocess_params，v1 兼容） |
| TranslationModels / TranslationEngines | 双引擎翻译组（en_zh Bergamot + ja_zh CTranslate2） |
| BergamotModelGroup / CTranslate2ModelGroup | 各引擎的文件条目与解码参数 |
| TranslationBudget | 体积预算（hard / target / 每语言对） |
| ModelEntry | 单个模型条目：id、相对路径、SHA-256、大小 |
| PreprocessParams / InferenceParams | OCR 预处理参数 / 遗留推理参数（v1 兼容保留） |
| ModelManager | 加载清单、路径解析、按引擎路径辅助、完整性校验、进度状态 |
| BergamotPaths / CTranslate2Paths | 按引擎解析出的绝对路径集合 |
| VerifyReport | 批量校验结果汇总 |
| ModelError | 错误枚举 |
| path::resolve_model_path / is_relative | 路径工具 |
| verify::verify_entry | 单个模型条目校验 |

核心类型签名：

```rust
pub struct ModelManifest {
    pub version: u32,                                // 当前仅支持 2（v1 拒绝）
    pub ocr: OcrModelGroup,
    pub translation: Option<TranslationModels>,      // null 表示无本地翻译
}

pub struct TranslationModels {
    pub target: String,                              // "zh-Hans"
    pub engines: TranslationEngines,                 // en_zh + ja_zh
    pub budget_mb: TranslationBudget,                // hard/target/en_zh/ja_zh
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,           // model_revision / converted_with / ...
}

pub struct BergamotModelGroup {
    pub engine: String,                              // "bergamot"
    pub model: ModelEntry,                           // model.enzh.intgemm.alphas.bin
    pub src_vocab: ModelEntry,                       // srcvocab.enzh.spm
    pub trg_vocab: ModelEntry,                       // trgvocab.enzh.spm
    pub lexical_shortlist: ModelEntry,               // lex.50.50.enzh.s2t.bin
    pub beam_size: usize,                            // 默认 1
    pub gemm_precision: String,                      // "int8shiftAlphaAll"
}

pub struct CTranslate2ModelGroup {
    pub engine: String,                              // "ctranslate2"
    pub model: ModelEntry,                           // model.bin
    pub config: ModelEntry,                          // config.json
    pub source_vocabulary: ModelEntry,               // source_vocabulary.json
    pub target_vocabulary: ModelEntry,               // target_vocabulary.json
    pub source_spm: ModelEntry,                      // source.spm
    pub target_spm: ModelEntry,                      // target.spm
    pub beam_size_fast: usize,                       // 1
    pub beam_size_balanced: usize,                   // 4
    pub max_input_tokens: usize,                     // 256
}

pub struct TranslationBudget {
    pub hard_mb: u64,                                // 200
    pub target_mb: u64,                              // 175
    pub en_zh_mb: u64,                               // 65
    pub ja_zh_mb: u64,                               // 110
}

pub struct ModelEntry { pub id: String, pub path: PathBuf, pub sha256: String, pub size_bytes: u64 }

impl ModelManager {
    pub fn from_manifest_dir(dir: &Path) -> Result<Self, ModelError>;
    pub fn manifest(&self) -> &ModelManifest;
    pub fn verify_integrity(&self) -> Result<VerifyReport, ModelError>;
    pub fn model_path(&self, entry: &ModelEntry) -> PathBuf;
    pub fn en_zh_paths(&self) -> Option<BergamotPaths>;
    pub fn ja_zh_paths(&self) -> Option<CTranslate2Paths>;
    pub fn load_progress(&self) -> Option<f32>;
    pub fn set_load_progress(&mut self, progress: Option<f32>);
}

pub struct VerifyReport { pub checked: usize, pub passed: usize, pub failed: Vec<String> }

pub enum ModelError {
    ManifestNotFound(PathBuf),
    Parse(serde_json::Error),
    FileNotFound(PathBuf),
    HashMismatch { id: String, expected: String, actual: String },
    UnsupportedVersion(u32),
    Io(std::io::Error),
}
```

serde 表示：ModelManifest 及其子结构实现 Serialize / Deserialize；dicts 为对象，image_size 为数组。VerifyReport 也可序列化，便于跨 IPC 传递校验结果。PreprocessParams 的 det/rec 新字段均为 serde default，v4 时代旧 manifest 反序列化后自动取 PP-OCRv6 默认值。

## 5. 行为契约

- 错误语义：ManifestNotFound 不可重试；Parse、Io 修复后可重试；UnsupportedVersion 需要升级/降级 manifest（v1 不被 v2 接受，属 A4 破坏性升级）。
- verify_integrity 永远返回 Ok(report)；FileNotFound、HashMismatch、Io 都以字符串进入 report.failed，消费方应检查 report.is_ok()。
- 并发模型：ModelManager 自动实现 Send + Sync，无内部锁；只读方法并发安全；set_load_progress 需要 &mut self。
- 取消语义：本模块无异步 API；verify_integrity 是同步阻塞操作，大模型目录应放到后台线程。
- 资源生命周期：不持有文件句柄，逐文件打开、校验后立即关闭，drop 无副作用。
- 边界条件：空目录返回 ManifestNotFound；rec_multi 与 translation 为 None 合法；dicts 可为空；空文件可以校验；路径辅助方法不检查存在性。

## 6. 模型准备与 manifest 回填

模型文件不提交 Git（.gitignore 忽略 src-tauri/resources/models/translation/*）。scripts/translation/ 提供全流程：

```powershell
.\scripts\translation\setup_translation_models.ps1
```

分步脚本：

| 脚本 | 作用 |
|------|------|
| fetch_firefox_enzh.py | 解析 Mozilla registry，锁定 en-zh Release base-memory，下载 4 个 Bergamot 文件并校验 SHA-256，输出每对下载清单 |
| convert_ja_zh_ct2.ps1 | 下载 shun89/opus-mt-ja-zh（锁定 revision）→ ct2-transformers-converter INT8 转换（锁 ctranslate2==4.8.1）→ 目录体积实测（<=110 MB） |
| audit_model_sizes.py | 200 MB 门禁：en-zh <= 65 / ja-zh <= 110 / 总 <= 200（目标 175）；含 --self-test |
| backfill_translation_manifest.py | 实测 SHA-256 / size_bytes 回填 translation 段（拒绝占位哈希），写入溯源 metadata |
| setup_translation_models.ps1 | 总入口：下载 → 转换 → 审计 → 回填 |

开发机要求见 docs/DEVELOPMENT.md 第 4 节（Python 3.10+、CTranslate2 4.8.1、网络；07 还需要 CMake/MSVC 构建 native bridge）。

## 7. 集成注意事项

| 坑 | 正确做法 |
|----|----------|
| from_manifest_dir 不会创建 manifest.json，首次运行必失败 | 先确保 src-tauri/resources/models/manifest.json 已随应用分发 |
| 模型文件未下载时 verify_integrity 报告大量失败 | 先运行 setup_translation_models.ps1；把失败项当作下载检查清单 |
| verify_integrity 同步阻塞，数百 MB 模型可能耗时数秒 | 在 spawn_blocking 或独立线程中调用 |
| 用 model_path / en_zh_paths / ja_zh_paths 判断文件是否可用 | 先 verify_integrity，再使用路径结果 |
| v1 manifest 无法加载 | 属 A4 破坏性升级；运行回填脚本生成 v2，或保持 translation: null |
| 期待 verify_integrity 返回 Err 来处理失败 | 检查 report.is_ok() 与 report.failed |

## 8. 设计决策记录

| 决策 | 理由 | 备选方案 |
|------|------|----------|
| manifest v2 为破坏性升级（A4 确认） | 翻译段整体替换为双引擎，v1 单 ONNX 路径已删除；OCR 段保持 v1 兼容 | 兼容解析 v1 翻译段（保留已删除旧路径，违背 A3） |
| ModelError 增加 Io(#[from] std::io::Error) | 文件存在但不可读不能归为 FileNotFound，保持错误链完整 | 把 IO 错误格式化成字符串塞进 Parse |
| verify_integrity 聚合所有失败而非短路 | 批量校验应一次给出全部失败文件 | 首个错误即返回 |
| SHA-256 以 8 KiB 分块流式计算 | 模型文件可达数百 MB，避免整文件载入内存 | read_to_end 一次性读入 |
| 字典文件只校验存在性，不校验哈希 | 规格的 dicts 只含路径，无哈希字段 | 为字典引入哈希字段（改动 schema） |
| 按引擎提供 en_zh_paths / ja_zh_paths 聚合路径 | 07 的 native bridge 需要一组完整路径，避免重复拼装 | 只暴露 model_path |
| 翻译段 metadata 用 HashMap 且缺省为空 | 下载/转换脚本写入溯源信息，schema 不枚举固定键 | 固定字段 |
| ja-zh 词表按共享词表复制两份布局 | Marian 共享词表，转换器只产出 shared_vocabulary.json；schema 固定两个词表条目 | 改 schema 为单共享词表 |

## 9. 已知限制

| 限制 | 类型 | 缓解/规避 |
|------|------|-----------|
| 仅支持 manifest schema version 2（v1 拒绝） | 设计使然（A4） | 版本字段保留，后续扩展 SUPPORTED_MANIFEST_VERSION |
| 字典文件没有 SHA-256 校验 | 设计使然（规格如此） | 需要强校验时自行对 dicts 文件额外做哈希 |
| verify_integrity 串行逐文件校验 | 性能限制 | 大目录可在上层并行调用 verify_entry |
| size_bytes 不参与校验 | 待优化 | 先做大小预检可跳过明显错误的哈希计算 |
| en-zh 词表/短表哈希来自下载时实测 | 设计使然（registry 仅对 model 提供 uncompressedHash） | revision 与 registry_generated 已冻结进 metadata |
| ja-zh model.bin 哈希依赖本机转换产物 | 设计使然（INT8 转换确定性强） | converted_with / ct2_model_revision 冻结；升级需重转并回填 |
| 验证 CLI 依赖真实模型目录 | 设计使然 | vtrans-verify-models 读取 --models / $VTRANS_MODEL_DIR |

## 10. 构建与测试

```powershell
cargo check -p vtrans-models
cargo test -p vtrans-models
cargo clippy -p vtrans-models --all-targets
cargo fmt -p vtrans-models -- --check
```

模型准备（需要网络与 Python 开发机）：

```powershell
.\scripts\translation\setup_translation_models.ps1
python scripts\translation\audit_model_sizes.py --self-test
```

测试覆盖：v2 manifest 解析/序列化往返、v1 拒绝（UnsupportedVersion(1)）、缺失字段、SHA-256 匹配/不匹配、文件不存在（含翻译条目）、OCR 段全量回归、路径解析、按引擎路径辅助、批量校验报告。

部署模型后可用独立验证 CLI 全量校验：

```powershell
cargo run --bin vtrans-verify-models -- --models src-tauri/resources/models
```

## 11. 详细规格

参见 docs/modules/08-models.md（含 v0.3.0 模型清单与脚本说明）。
