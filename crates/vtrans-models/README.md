# vtrans-models

模型管理模块。管理 OCR 和翻译模型的清单定义、完整性校验和生命周期。

## 职责

管理 OCR 和翻译模型的清单定义（`manifest.json`）、SHA-256 完整性校验、
路径解析和加载进度跟踪。模型文件不提交 Git，通过 manifest 和下载脚本管理。

## 依赖

### 上游 crate

- `vtrans-core` — `Language` 类型（用于翻译模型支持的语言对）

### 外部 crate

| crate | 用途 | 许可证 |
|-------|------|--------|
| `serde` / `serde_json` | manifest 序列化/反序列化 | MIT/Apache-2.0 |
| `thiserror` | 错误类型派生 | MIT/Apache-2.0 |
| `tracing` | 结构化日志 | MIT |
| `sha2` | SHA-256 哈希校验 | MIT/Apache-2.0 |

## 公开 API 概要

```rust
// 清单 schema
pub struct ModelManifest { pub version: u32, pub ocr: OcrModelGroup, pub translation: Option<TranslationModelGroup> }
pub struct OcrModelGroup { pub det: ModelEntry, pub rec_ja: ModelEntry, pub rec_en: ModelEntry, pub rec_multi: Option<ModelEntry>, pub dicts: HashMap<String, PathBuf>, pub preprocess_params: PreprocessParams }
pub struct TranslationModelGroup { pub model: ModelEntry, pub tokenizer: ModelEntry, pub supported_pairs: Vec<(Language, Language)>, pub max_length: usize, pub inference_params: InferenceParams }
pub struct ModelEntry { pub id: String, pub path: PathBuf, pub sha256: String, pub size_bytes: u64 }
pub struct PreprocessParams { pub image_size: (u32, u32), pub mean: [f32; 3], pub std: [f32; 3], pub det_threshold: f32, pub unclip_ratio: f32 }
pub struct InferenceParams { pub max_batch_size: usize, pub num_beams: usize }

// 模型管理器
pub struct ModelManager { /* ... */ }
impl ModelManager {
    pub fn from_manifest_dir(dir: &Path) -> Result<Self, ModelError>;
    pub fn manifest(&self) -> &ModelManifest;
    pub fn manifest_dir(&self) -> &Path;
    pub fn verify_integrity(&self) -> Result<VerifyReport, ModelError>;
    pub fn model_path(&self, entry: &ModelEntry) -> PathBuf;
    pub fn load_progress(&self) -> Option<f32>;
    pub fn set_load_progress(&mut self, progress: Option<f32>);
}

// 校验报告
pub struct VerifyReport { pub checked: usize, pub passed: usize, pub failed: Vec<String> }

// 错误类型
pub enum ModelError {
    ManifestNotFound(PathBuf),
    Parse(serde_json::Error),
    FileNotFound(PathBuf),
    HashMismatch { id: String, expected: String, actual: String },
    UnsupportedVersion(u32),
    Io(std::io::Error),
}
```

## 文件结构

```text
crates/vtrans-models/
├── Cargo.toml
├── README.md
├── resources/
│   └── manifest.json          # 默认清单模板
├── src/
│   ├── lib.rs                 # ModelError 定义 + re-export
│   ├── manifest.rs            # ModelManifest schema
│   ├── manager.rs             # ModelManager 实现
│   ├── verify.rs              # SHA-256 校验 + VerifyReport
│   └── path.rs                # 路径解析
└── tests/
    ├── integrity.rs           # 集成测试
    └── fixtures/
        └── sample.txt         # 测试夹具
```

## 构建/测试

```powershell
cargo build -p vtrans-models
cargo test -p vtrans-models
cargo clippy -p vtrans-models --all-targets
cargo fmt -p vtrans-models -- --check
```

## 模型文件管理

- `manifest.json` 位于 `src-tauri/resources/models/` 目录，提交到 Git
- 模型文件（`*.onnx`、`*.bin`）不提交 Git，通过 `.gitignore` 排除
- 下载脚本 `scripts/download_models.ps1` 下载并校验模型文件
- 运行时通过 `ModelManager::from_manifest_dir` 加载清单并校验完整性

## 已知限制

- 仅支持 manifest schema version 1
- `verify_integrity` 逐文件串行校验，大量模型时可能较慢（可改为并行）
- 字典文件仅校验存在性，不校验 SHA-256（manifest 中无字典哈希）
- `load_progress` 目前仅提供 getter/setter，实际下载进度由上层驱动

## 详细规格

参见 [docs/modules/08-models.md](../../docs/modules/08-models.md)
