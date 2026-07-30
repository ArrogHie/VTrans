# vtrans-text

文本标准化模块。清除异常空格、合并 OCR 行、计算文本指纹、按段落切分。

## 职责

- TextNormalizer：clean, merge_lines, fingerprint, split_paragraphs
- is_duplicate：判断两段文本是否实质相同

## 依赖

vtrans-core

## 构建

```powershell
cargo build -p vtrans-text
cargo test -p vtrans-text
```

## 详细规格

参见 docs/modules/06-text.md
