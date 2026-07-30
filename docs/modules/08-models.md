# 模块 08：vtrans-models 模型管理

| 属性 | 值 |
|------|-----|
| Crate | `vtrans-models` |
| 分支 | `feat/08-models` |
| 上游依赖 | `vtrans-core` |
| 层级 | 1 |
| 复杂度 | 中 |
| 阶段 | Phase 1 |

## 职责

管理 OCR 和翻译模型的清单定义、完整性校验、生命周期和路径解析。模型文件不提交 Git，通过 manifest 和下载脚本管理。

## 公开 API

```rust
/// 模型清单
[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelManifest {
    pub version: u32,
    pub ocr: OcrModelGroup,
    pub translation: Option<TranslationModelGroup>,
}

pub struct OcrModelGroup {
    pub det: ModelEntry,      // 文本检测模型
    pub rec_ja: ModelEntry,   // 日文识别模型
    pub rec_en: ModelEntry,   // 英文识别模型
    pub rec_multi: Option<ModelEntry>, // 多语言识别（可选）
    pub dicts: HashMap<String, PathBuf>, // 字典文件
    pub preprocess_params: PreprocessParams,
}

pub struct TranslationModelGroup {
    pub model: ModelEntry,
    pub tokenizer: ModelEntry,
    pub supported_pairs: Vec<(Language, Language)>,
    pub max_length: usize,
    pub inference_params: InferenceParams,
}

pub struct ModelEntry {
    pub id: String,
    pub path: PathBuf,       // 相对于 models/ 目录
    pub sha256: String,
    pub size_bytes: u64,
}

pub struct PreprocessParams {
    pub image_size: (u32, u32),
    pub mean: [f32; 3],
    pub std: [f32; 3],
    pub det_threshold: f32,
    pub unclip_ratio: f32,
}

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
[derive(Debug, thiserror::Error)]
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
}
```

## 内部文件结构

```text
crates/vtrans-models/
├── Cargo.toml
├── README.md
├── src/
│   ├── lib.rs          # re-export
│   ├── manifest.rs      # ModelManifest schema
│   ├── manager.rs       # ModelManager 实现
│   ├── verify.rs        # SHA-256 校验
│   └── path.rs          # 路径解析
├── resources/
│   └── manifest.json    # 默认清单模板
└── tests/
```

## 测试计划

| 测试项 | 类型 | 说明 |
|--------|------|------|
| Manifest 解析 | 单元 | 有效 JSON 正确反序列化 |
| 缺失字段 | 单元 | 必填字段缺失返回 Parse 错误 |
| SHA-256 校验 | 单元 | 匹配返回 Ok，不匹配返回 HashMismatch |
| 文件不存在 | 单元 | 返回 FileNotFound |
| 路径解析 | 单元 | 相对路径正确解析为绝对路径 |
| VerifyReport | 集成 | 多文件批量校验结果汇总 |

## 验收标准

- [ ] manifest.json 可正确解析
- [ ] SHA-256 校验功能正常
- [ ] 缺失文件返回明确错误
- [ ] 提供 manifest.json 模板
- [ ] 模型文件不提交 Git
- [ ] README.md 完整

## 开发注意事项

- manifest.json 位于 src-tauri/resources/models/ 目录
- SHA-256 使用 sha2 crate
- 模型路径在 manifest 中用相对路径，运行时解析为绝对路径
- 提供下载脚本 scripts/download_models.ps1
- .gitignore 排除 *.onnx 和 *.bin 模型文件
