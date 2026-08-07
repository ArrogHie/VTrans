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
    pub box_threshold: f32,        // 可选，缺省 0.45
    pub max_candidates: usize,     // 可选，缺省 3000
    pub min_box_size: f32,         // 可选，缺省 3.0
    pub rec_input_height: u32,     // 可选，缺省 48
    pub rec_input_width: u32,      // 可选，缺省 320
    pub rec_append_space: bool,    // 可选，缺省 true
    pub rec_blank_index: usize,    // 可选，缺省 0
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
- 提供下载脚本 scripts/ppocrv6/setup_ppocrv6.ps1
- .gitignore 排除 *.onnx 和 *.bin 模型文件

## PP-OCRv6 Small 模型清单（v0.2.0）

自 v0.2.0 起 OCR 检测/识别模型升级为 PP-OCRv6 Small，彻底弃用 PP-OCRv4。

### 模型条目

| 槽位 | id | 文件 | SHA-256 | size_bytes |
|------|----|------|---------|------------|
| det | `ppocr-det-v6` | `ocr/det.onnx` | `d73e0058...c9410e` | 9880512 |
| rec_ja | `ppocr-rec-v6` | `ocr/rec.onnx` | `5435fd74...a24634` | 21159378 |
| rec_en | `ppocr-rec-v6-en` | `ocr/rec.onnx` | 同上（同一 v6 rec） | 21159378 |
| rec_multi | `ppocr-rec-v6-multi` | `ocr/rec.onnx` | 同上（同一 v6 rec） | 21159378 |

rec_ja / rec_en / rec_multi 三槽位共享同一份 `PP-OCRv6_small_rec` ONNX
（磁盘仅 `ocr/rec.onnx` 一份，运行时只加载一个 rec session），因此
`auto` / `zh-CN` OCR 语言可用。

### 字典

`ocr/ppocrv6_dict.txt`（官方 `ppocrv6_dict.txt`，18708 行，SHA-256
`b5f2bfe2...e401c5d`）。三个语言槽位（`ja` / `en` / `auto`）指向同一文件。
识别输出类别数 = 字典行数 + blank + space = 18710，转换后已通过类数一致性检查。

### preprocess_params（v6 默认值）

| 字段 | 值 | 说明 |
|------|-----|------|
| `image_size` | `[640, 640]` | 检测输入上限（与 Python 基准 limit_side=640 一致） |
| `mean` / `std` | ImageNet 均值/方差 | BGR 通道顺序（以 Python 基准为准） |
| `det_threshold` | 0.2 | DB 二值化阈值（原 v4 0.3） |
| `unclip_ratio` | 1.4 | 外扩系数（原 v4 2.0） |
| `box_threshold` | 0.45 | 框置信度过滤 |
| `max_candidates` | 3000 | 最大候选框数 |
| `min_box_size` | 3.0 | 最短边过滤 |
| `rec_input_height` | 48 | 识别输入高 |
| `rec_input_width` | 320 | 识别输入宽（右补零） |
| `rec_append_space` | true | 类别表追加空格 |
| `rec_blank_index` | 0 | CTC blank 索引 |

### 脚本（方案 B）

`scripts/ppocrv6/setup_ppocrv6.ps1` 提供「下载 → 转换 → 检查 → 基准 →
回填」全流程；`inspect_onnx.py` / `baseline_ocr.py` / `backfill_manifest.py`
为分步工具。开发机要求见 `docs/DEVELOPMENT.md` §4 与接入指南 §4.1。

### 向后兼容

manifest schema version 仍为 1。v4 时代的 manifest（无 `box_threshold` 等
新字段）仍可反序列化，缺省字段自动取上述 v6 默认值。
