# vtrans-text

## 1. 模块概述

屏幕翻译流水线中的文本标准化层：清除 OCR 输出中的异常空白与不可见字符、把零散的 OCR 行合并成段落、规范化日文标点、计算文本指纹用于去重、按段落切分并限制单段长度。

**边界**：
- 做：纯文本处理（合并 / 清洗 / 指纹 / 切分），无 IO、无网络、无模型推理，可在任意线程同步调用。
- 不做：不负责 OCR 识别（vtrans-ocr）、翻译（vtrans-translation）或画面采集（vtrans-capture）。
- 不做：不做语言检测与形态分析——日文标点规则需要调用方在已知源语言为日文时显式启用。
- 不做：不持有运行状态或资源，`TextNormalizer` 是无状态入口，无生命周期管理负担。

## 2. 依赖关系

| 方向 | Crate | 关系 |
|------|-------|------|
| 上游 | `vtrans-core` | 消费 `OcrLine`（text / confidence / polygon / reading_order，派生 serde，JSON 表示用于 IPC）；日志工具 `truncate_for_log` |
| 外部 | `thiserror` | `TextError` 错误类型派生 |
| 外部 | `tracing` | 结构化日志（`#[instrument]`、`warn!` / `debug!`） |
| dev | `pretty_assertions` | 测试断言美化 |
| 下游（直接） | `vtrans-pipeline` (09) | 需要 `merge_lines` + `clean` 后处理 OCR 结果、`fingerprint` / `is_duplicate` 做实时去重、`split_paragraphs` / `validate_length` 切分翻译请求 |
| 下游（间接） | `vtrans-app` (10) | 经 pipeline 间接使用，不直接依赖本 crate |

本模块不定义跨 IPC 的 serde 类型；`TextError` 不派生 `Serialize`（纯内部错误）。

## 3. 快速上手

```rust
use vtrans_core::OcrLine;
use vtrans_text::{is_duplicate, japanese, TextNormalizer};

fn main() -> Result<(), vtrans_text::TextError> {
    // 1. OCR 输出（此处模拟）：同一段落两行 + 新段落一行
    let lines = vec![
        OcrLine::new("こんにちは、", 0.9, [[0., 0.], [60., 0.], [60., 20.], [0., 20.]], 0),
        OcrLine::new("世界。", 0.9, [[60., 0.], [120., 0.], [120., 20.], [60., 20.]], 1),
        OcrLine::new("次へ進む", 0.9, [[0., 40.], [80., 40.], [80., 60.], [0., 60.]], 2),
    ];

    // 2. 按 reading_order + 行间距合并为段落（段落间以 \n 分隔）
    let merged = TextNormalizer::merge_lines(&lines);

    // 3. 清洗：去零宽字符、全角空格/全角 ASCII 转半角；日文源文本再规范化标点
    let cleaned = TextNormalizer::clean(&merged);
    let text = japanese::normalize_punctuation(&cleaned);

    // 4. 指纹去重：与上一帧比较，未变化则跳过翻译
    let previous_frame = "こんにちは、世界。\n次へ進む";
    if is_duplicate(&text, previous_frame) {
        println!("文本未变化，跳过翻译");
    }

    // 5. 切分为不超过 2000 字符的段落，逐段做长度校验
    for chunk in TextNormalizer::split_paragraphs_default(&text) {
        TextNormalizer::validate_length(&chunk, 2000)?;
    }
    Ok(())
}
```

说明：
- **所有权**：所有方法返回 owned `String` / `Vec<String>` / `u64`，入参为 `&str` / `&[OcrLine]`，无借用逃逸；`TextNormalizer` 为无状态单元结构体，无需实例化或持有。
- **异步**：本模块完全同步，无 `tokio`、无 `CancellationToken`；调用方可在任意线程直接调用。

## 4. 公开 API 概要

| 公开项 | 签名 | 用途 |
|--------|------|------|
| `TextNormalizer::clean` | `fn clean(raw: &str) -> String` | 去不可见字符/控制符、归一化换行与空白（含全角空格 U+3000）、全角 ASCII→半角；语言无关 |
| `TextNormalizer::merge_lines` | `fn merge_lines(lines: &[OcrLine]) -> String` | 按 `reading_order` 排序，Y 间距 ≤ 平均行高 × 0.75 合并为同段；段落间 `\n` |
| `TextNormalizer::fingerprint` | `fn fingerprint(text: &str) -> u64` | FNV-1a 64 位指纹；空白/换行/零宽字符不敏感 |
| `TextNormalizer::split_paragraphs` | `fn split_paragraphs(text: &str, max_len: usize) -> Vec<String>` | 按换行分段并限制每段字符数；`max_len = 0` 表示不限长 |
| `TextNormalizer::split_paragraphs_default` | `fn split_paragraphs_default(text: &str) -> Vec<String>` | 使用 `DEFAULT_MAX_PARAGRAPH_LEN` 切分 |
| `TextNormalizer::validate_length` | `fn validate_length(text: &str, max_len: usize) -> Result<(), TextError>` | 长度守卫，超限返回 `TooLong(len)` |
| `is_duplicate` | `fn is_duplicate(a: &str, b: &str) -> bool` | 两段文本指纹一致即视为重复 |
| `japanese::normalize_punctuation` | `fn normalize_punctuation(text: &str) -> String` | 日文标点规范化：`，`→`、`，`．`→`。`，`､`→`、`，`｡`→`。`，`～`→`〜` |
| `TextError` | enum | `TooLong(usize)` 文本超限；`Failed(String)` 预留 |
| `DEFAULT_MAX_PARAGRAPH_LEN` | `usize = 2000` | 默认每段最大字符数 |
| `MERGE_LINE_GAP_RATIO` | `f32 = 0.75` | 段落合并的行间距阈值系数（位于 `normalizer` 模块） |

核心语义：`clean` 刻意保留 `，` / `．` / `～` 三个全角字符不转换（语言相关，见 §5）；`split_paragraphs` 切分优先级为句读（。！？…；.!?;）→ 空白 → 字符硬切，窗口落在词尾时直接用满窗。完整字段与签名见 `docs/modules/06-text.md`。

## 5. 行为契约

- **错误语义**：唯一可失败入口是 `validate_length`，失败返回 `TextError::TooLong(实际字符数)`，属**校验类错误、不可重试**——正确做法是先 `split_paragraphs` 再逐段校验；其余入口全部不可失败（空输入返回空结果而非错误）。`TextError::Failed` 当前无构造路径，为冻结规格预留。
- **并发模型**：全部函数为纯函数，`TextNormalizer` 是无状态单元结构体，天然 `Send + Sync`、无内部锁；多线程并发调用安全。
- **取消语义**：不适用——无异步、无 `CancellationToken`；如需取消请在上层（pipeline）进行。
- **资源生命周期**：无文件句柄、会话、模型等资源；调用方不承担任何 close / drop 义务，返回值均为 owned 数据。
- **边界条件**：空/纯空白输入 → `clean` 返回 `""`、`merge_lines` 返回 `""`、`split_paragraphs` 返回 `[]`、`is_duplicate("", " ")` 返回 `true`；`max_len = 0` 视为不限长；超长文本线性处理，但无空白且无句读的段落会硬切字符边界（可能拆词）；`merge_lines` 的多边形退化（高度为 0）时按默认 8px 间距阈值判定段落；指纹为非加密 64 位哈希，存在理论碰撞可能。

## 6. 集成注意事项

| 坑 | 正确做法 |
|----|----------|
| `merge_lines` 依赖 OCR 的 `reading_order`；若全为 0 则按输入顺序合并，段落顺序可能不符合视觉顺序 | OCR Provider 生成 `reading_order`（top→bottom、left→right）；无法保证时接受"按输入序"并在模块文档中记录 |
| `clean` 不处理日文标点，`，` 会原样保留 | 源语言为日文时在 `clean` 之后调用 `japanese::normalize_punctuation`；中文文本**不要**调用后者（会把 `，` 改成 `、` 改变原意） |
| `split_paragraphs` 把每个 `\n` 都当段落边界，多行段落（如诗歌）会被拆散 | 只喂 `merge_lines` 或已按段落组织好的文本，不要喂含内部换行的原始长文本 |
| 指纹对空白不敏感但词边界敏感：`"A\nB"` 与 `"A B"` 相同，`"A B"` 与 `"AB"` 不同 | 去重比较前统一走同一 `merge_lines` + `clean` 流程，不要混用原始与清洗后的文本 |
| `validate_length` 按传入文本原样计字符数，不计清洗后长度 | 先 `clean` 再 `validate_length`，或直接用 `split_paragraphs` 保证长度 |

## 7. 设计决策记录

| 决策 | 理由 | 备选方案 |
|------|------|----------|
| `clean` 语言无关，日文标点独立为 `japanese::normalize_punctuation` | `clean` 无语言上下文，无条件把 `，`→`、` 会破坏中文语义（规格要求"不改变原意"） | clean 内置日文规则（中文文本被破坏）；clean 增加 `Language` 参数（偏离冻结的公开签名） |
| 指纹内联 FNV-1a，不引入 `twox-hash` / `fnv` | 低复杂度纯逻辑模块，避免新增外部依赖的引入/审查/体积成本；FNV-1a 64 位速度与碰撞率满足去重场景 | xxHash（需新增依赖，收益不抵成本） |
| `merge_lines` 以 `reading_order` 为主序 + Y 间距分组 | 与 core 的 `OcrResult::from_lines` 排序语义一致，同时满足规格"Y 坐标接近合并为同段" | 纯空间排序（忽略 reading_order，与 core 契约冲突）；纯 reading_order 不分组（无法形成段落） |
| `split_paragraphs` 换行即段落边界 | 与 `merge_lines` 输出（段落间 `\n`）直接对接，行为确定可测 | 仅空白行分段（`merge_lines` 输出无空白行，会退化为整段一刀切） |
| `TextError::Failed` 保留但不构造 | 规格冻结的错误契约（两变体），`TooLong` 已有真实构造路径，`Failed` 留给未来可失败路径 | 删除 `Failed`（违反冻结契约，下游 `matches!` 无法匹配该变体） |

## 8. 已知限制

**设计使然（非缺陷）**：

| 限制 | 缓解 / 规避 |
|------|-------------|
| 日文标点规范化不自动启用、不做语言检测 | 调用方按 `Language` 决定是否调用 `japanese::normalize_punctuation` |
| 行合并空格启发式对罕见拆行不完美（如 `3.` + `14`） | 以 `1,` + `000` 已正确处理为基线；OCR 引擎应尽量避免在数字中间断行 |
| 指纹 64 位、非加密；对词边界变化敏感 | 仅用于"文本是否变化"判断，不做安全用途；统一预处理后再比较 |
| 无空白/句读的超长段落硬切可能拆词或拆组合字符 | 默认 `max_len = 2000` 远大于常见句长，实际极少触发硬切 |
| `split_paragraphs` 对"超长文本 + 极小 max_len"最坏 O(n²)（每轮重数剩余字符） | 实际 `max_len ≥ 100`、输入为段落级规模；极端场景可预计算字符索引优化 |

**未实现**：无。06 模块规格的验收项已全部实现；验证 CLI 非规格要求，故未提供 `examples/`。

## 9. 构建与测试

```powershell
cargo check -p vtrans-text
cargo test -p vtrans-text
cargo clippy -p vtrans-text --all-targets
cargo fmt -p vtrans-text -- --check
```

测试组成：71 单元测试（`src/*.rs` 内 `#[cfg(test)]`）、5 集成测试（`tests/flow.rs`，模拟流水线完整调用链）、13 rustdoc doctest；覆盖率（llvm-cov）≈99% 行 / 100% 函数。

## 10. 详细规格

参见 `docs/modules/06-text.md`。
