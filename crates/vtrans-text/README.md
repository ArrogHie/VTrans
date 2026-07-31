# vtrans-text

文本标准化模块：清除异常空格和不可见字符、合并 OCR 行、规范化日文标点、计算文本指纹去重、按段落切分并限制长度。

## 职责

屏幕翻译流水线中，OCR 的原始输出不能直接送翻译引擎：OCR 行需要按空间位置合并成段落，
文本里可能混入零宽字符、全角空格、全角 ASCII 等噪声，实时模式需要指纹去重避免重复翻译，
单次请求还需要限制长度。本模块是纯逻辑、无 IO 依赖的文本处理层，不修改专有名词、不做词形分析。

## 依赖

| 类型 | Crate | 用途 |
|------|-------|------|
| 上游 | `vtrans-core` | `OcrLine` 类型、`truncate_for_log` 日志截断 |
| 外部 | `thiserror` | `TextError` 错误类型派生 |
| 外部 | `tracing` | 结构化日志（`#[instrument]`、`warn!`/`debug!`） |
| dev | `pretty_assertions` | 测试断言美化 |

无新增外部依赖：指纹采用内联实现的 FNV-1a（64 位），避免为低复杂度纯逻辑模块引入
`twox-hash`/`fnv` 等外部 crate。

## 公开 API 概要

```rust
pub struct TextNormalizer; // 无状态命名空间
impl TextNormalizer {
    pub fn clean(raw: &str) -> String;                         // 去不可见字符、规范化空白与全角 ASCII
    pub fn merge_lines(lines: &[OcrLine]) -> String;           // 按 reading_order + Y 间距合并为段落
    pub fn fingerprint(text: &str) -> u64;                     // FNV-1a 指纹（空白不敏感）
    pub fn split_paragraphs(text: &str, max_len: usize) -> Vec<String>; // 按段落切分并限制每段长度
    pub fn split_paragraphs_default(text: &str) -> Vec<String>; // 使用默认 max_len = 2000
    pub fn validate_length(text: &str, max_len: usize) -> Result<(), TextError>;
}

pub fn is_duplicate(a: &str, b: &str) -> bool;   // 指纹一致即视为重复

pub mod japanese {
    pub fn normalize_punctuation(text: &str) -> String; // 日文标点规范化（，→、 等）
}

pub const DEFAULT_MAX_PARAGRAPH_LEN: usize = 2000; // 默认每段最大字符数
pub const MERGE_LINE_GAP_RATIO: f32 = 0.75;        // 行间距阈值 = 平均行高 × 该比例

pub enum TextError {
    TooLong(usize),   // 文本超过长度上限（字符数）
    Failed(String),   // 预留：未来可失败路径
}
```

推荐调用顺序（流水线 09 的使用方式）：

```text
merge_lines(lines) -> clean(text) -> [日文源文本时] japanese::normalize_punctuation(text)
    -> fingerprint(text) 去重 -> split_paragraphs(text, max_len) -> 逐段 validate_length
```

## 构建与测试

```powershell
cargo build -p vtrans-text
cargo test -p vtrans-text
cargo clippy -p vtrans-text --all-targets
cargo fmt --all -- --check
```

## 已知限制

- `clean` 是语言无关的：全角逗号 `，`、句号 `．`、波浪线 `～` 不会被改写，因为它们的规范形式
  取决于源语言。日文源文本需要额外调用 `japanese::normalize_punctuation`（字符级规则，不做语言检测，
  对中文文本使用会把 `，` 改写成 `、`，改变原意）。
- `merge_lines` 依赖 OCR 提供的 `reading_order` 与多边形坐标：`reading_order` 不可靠时，
  段落顺序可能不符合视觉阅读顺序；行合并的空格启发式对「数字被拆行」等罕见情况不保证完美
  （如 `1,` + `000` 已正确处理，但 `3.` + `14` 会合并为 `3.14` 前的句号边界不处理）。
- 指纹是 FNV-1a 64 位哈希，非加密用途；对空白/换行/零宽字符不敏感，但对新增空格导致的
  词边界变化敏感（这是特性：避免把语义不同的文本判为重复）。
- 超长且无空白/无句读的段落会被硬切在字符边界，可能把词或组合字符拆开（`max_len` 过小时）。
- `split_paragraphs` 把每个换行都视为段落边界；多行段落（如诗歌）会被拆成多个元素。
- 无 IO、无线程、无异步：不需要运行时，适合高覆盖率单元测试。

## 详细规格

参见 `docs/modules/06-text.md`。
