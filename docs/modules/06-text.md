# 模块 06：vtrans-text 文本标准化

| 属性 | 值 |
|------|-----|
| Crate | `vtrans-text` |
| 分支 | `feat/06-text` |
| 上游依赖 | `vtrans-core` |
| 层级 | 1 |
| 复杂度 | 低 |
| 阶段 | Phase 1 |

## 职责

清除异常空格和不可见字符，合并属于同一段的 OCR 行，保留必要换行，日文标点规范化，计算文本指纹避免重复翻译，限制单次发送长度并按段落切分。不修改专有名词或改变原意。

## 公开 API

```rust
/// 文本标准化器
pub struct TextNormalizer;

impl TextNormalizer {
    /// 清除异常空格、不可见字符，规范化标点
    pub fn clean(raw: &str) -> String;

    /// 合并 OCR 行为段落，保留必要换行
    pub fn merge_lines(lines: &[OcrLine]) -> String;

    /// 计算文本指纹（用于去重判断）
    pub fn fingerprint(text: &str) -> u64;

    /// 按段落切分，限制每段最大长度
    pub fn split_paragraphs(text: &str, max_len: usize) -> Vec<String>;
}

/// 判断两段文本是否实质相同（指纹一致）
pub fn is_duplicate(a: &str, b: &str) -> bool;
```

## 错误类型

```rust
[derive(Debug, thiserror::Error)]
pub enum TextError {
    #[error("text too long: {0} chars")]
    TooLong(usize),
    #[error("normalization failed: {0}")]
    Failed(String),
}
```

## 内部文件结构

```text
crates/vtrans-text/
├── Cargo.toml
├── README.md
└── src/
    ├── lib.rs          # re-export
    ├── normalizer.rs    # TextNormalizer 实现
    ├── fingerprint.rs   # 指纹计算
    ├── japanese.rs      # 日文标点规范化规则
    └── paragraph.rs     # 段落切分逻辑
```

## 测试计划

| 测试项 | 类型 | 说明 |
|--------|------|------|
| 清除零宽字符 | 单元 | U+200B 等 removed |
| 全角空格转半角 | 单元 | 日文文本处理 |
| 合并相邻行 | 单元 | 同段落行合并为一段 |
| 保留换行 | 单元 | 段落间换行保留 |
| 指纹稳定性 | 单元 | 相同文本指纹一致 |
| 指纹区分性 | 单元 | 微小差异指纹不同 |
| 去重判断 | 单元 | is_duplicate 正确返回 |
| 段落切分 | 单元 | 超长文本按 max_len 切分 |
| 日文标点 | 单元 | 全角逗号/句号规范化 |

## 验收标准

- [ ] clean 去除不可见字符和异常空格
- [ ] merge_lines 正确合并同段落 OCR 行
- [ ] fingerprint 相同文本一致、不同文本不同
- [ ] split_paragraphs 不超过 max_len
- [ ] 日文标点规范化正确
- [ ] 单元测试覆盖率 > 80%
- [ ] README.md 完整

## 开发注意事项

- 指纹使用 xxHash 或 FNV-1a，足够快且无碰撞风险
- 合并逻辑考虑行间距：Y 坐标接近的行合并为同一段
- 不修改专有名词（不做形态分析或词形还原）
- max_len 默认 2000 字符，可配置
- 纯逻辑模块，无 IO 依赖，适合高覆盖率单元测试
