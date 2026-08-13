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

清除异常空格和不可见字符，合并属于同一段的 OCR 行，保留必要换行，日文标点规范化，计算文本指纹避免重复翻译，限制单次发送长度并按段落切分；并提供多框实时翻译使用的 per-box 指纹去重缓存（框间状态隔离）。不修改专有名词或改变原意。

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

/// 多框实时翻译的 per-box 指纹去重缓存（线程安全，`Send + Sync`）
/// 从 crate 根 re-export（`vtrans_text::BoxFingerprintCache`）
#[derive(Debug, Default)]
pub struct BoxFingerprintCache { /* Mutex<HashMap<u32, u64>> */ }

impl BoxFingerprintCache {
    pub fn new() -> Self;

    /// 记录 `box_id` 的最新文本指纹并返回是否与上一帧重复。
    /// 命中重复时跳过翻译；文本本身不落日志（debug 级只记 box_id）。
    pub fn is_duplicate(&self, box_id: u32, text: &str) -> bool;

    /// 重置 `box_id` 的去重状态（如框区域更新后）
    pub fn clear_box(&self, box_id: u32);

    /// 移除 `box_id` 的条目（如框被删除后回收内存）
    pub fn remove_box(&self, box_id: u32);

    /// 重置全部框的去重状态（如整场会话重启后）
    pub fn clear_all(&self);
}
```

### per-box 去重语义（与单框指纹去重的关系）

- **算法相同**：`is_duplicate` 使用与 `TextNormalizer::fingerprint` / `is_duplicate`
  一致的 FNV-1a 指纹，空白（空格/换行/零宽字符）先归一化再哈希——帧间 OCR
  抖动不破坏去重，真实字词变化才会产生新指纹。
- **状态隔离**：每个 `box_id`（`u32`，对应 `vtrans_config::TranslationBoxConfig.id`）
  独立保存**最近一个**指纹；同一文本出现在两个不同框时各自视为新文本、
  各自翻译（每框 overlay 需要独立结果）。
- **只存最近指纹**：文本 A→B→A 时第三帧不是重复（存储的是 B），会重新翻译，
  保证 overlay 始终反映当前屏幕内容；空文本/纯空白文本之间互为重复。
- **并发**：内部 `Mutex` 保护 `HashMap<u32, u64>`，多个框任务可共享单个
  `Arc<BoxFingerprintCache>` 并发调用；Mutex 被毒化时 panic。

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
    ├── box_dedup.rs     # BoxFingerprintCache（多框 per-box 去重）
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
| per-box 去重 | 单元 | 同框同文本重复、异框同文本不重复、A→B→A 不重复、空白不敏感、零宽字符忽略 |
| 框生命周期清理 | 单元 | clear_box 只重置目标框、remove_box 回收条目、clear_all 全清、对不存在框为 no-op |
| 并发安全 | 单元 | 多线程共享 Arc 并发调用不 panic；同框并发首帧恰有一次非重复 |
| 段落切分 | 单元 | 超长文本按 max_len 切分 |
| 日文标点 | 单元 | 全角逗号/句号规范化 |

## 验收标准

- [ ] clean 去除不可见字符和异常空格
- [ ] merge_lines 正确合并同段落 OCR 行
- [ ] fingerprint 相同文本一致、不同文本不同
- [ ] split_paragraphs 不超过 max_len
- [ ] 日文标点规范化正确
- [x] `BoxFingerprintCache` per-box 去重与框间隔离（`box_dedup.rs` 单测全绿，含并发用例）
- [ ] 单元测试覆盖率 > 80%
- [ ] README.md 完整

## 开发注意事项

- 指纹使用 xxHash 或 FNV-1a，足够快且无碰撞风险
- 合并逻辑考虑行间距：Y 坐标接近的行合并为同一段
- 不修改专有名词（不做形态分析或词形还原）
- max_len 默认 2000 字符，可配置
- `BoxFingerprintCache` 只存每框最近一个指纹（非历史），框删除时调
  `remove_box` 避免随增删框缓慢泄漏；文本内容永不落日志
- 纯逻辑模块，无 IO 依赖，适合高覆盖率单元测试

## 增量记录

### 多框增量：per-box 指纹去重（分支 `feat/multibox-text`）

对应功能计划 `docs/features/multi-box-realtime/PLAN.md`。

- 新增 `box_dedup.rs`：`BoxFingerprintCache`（crate 根 re-export），
  `HashMap<u32, u64>` + `Mutex` 实现线程安全的 per-box 去重。
- 复用既有 FNV-1a 指纹算法（`fingerprint_text`），与单框
  `TextNormalizer::fingerprint` / `is_duplicate` 同一算法——仅状态按
  `box_id` 隔离，避免一框文本影响另一框的去重判断。
- 公开方法：`new` / `is_duplicate` / `clear_box` / `remove_box` /
  `clear_all`（另有 `Default`）。
- 未修改既有清洗/合并/切分逻辑与其它 crate；单元测试覆盖框间隔离、
  A→B→A 重译、空白/零宽不敏感、生命周期清理与并发访问。
