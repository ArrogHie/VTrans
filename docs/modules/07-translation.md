# 模块 07：vtrans-translation 翻译引擎

| 属性 | 值 |
|------|-----|
| Crate | `vtrans-translation` |
| 分支 | `feat/07-translation` |
| 上游依赖 | `vtrans-core`, `vtrans-models` |
| 层级 | 2 |
| 复杂度 | 高 |
| 阶段 | Phase 2 |

## 职责

实现 TranslationProvider trait，提供 API 翻译和本地 ONNX 翻译两种实现。支持取消、超时和有限次数重试。Prompt 明确要求只返回译文不解释内容。

## 公开 API

实现 `vtrans_core::TranslationProvider` trait。

```rust
/// 通用 HTTP/JSON API 翻译器
pub struct ApiTranslationProvider { /* ... */ }

impl ApiTranslationProvider {
    pub fn new(
        endpoint: &str,
        model: &str,
        api_key: &str,
        timeout: Duration,
        max_retries: u32,
    ) -> Self;
}

/// 本地 ONNX 翻译器
pub struct LocalTranslationProvider { /* ... */ }

impl LocalTranslationProvider {
    pub fn from_manifest(manifest: &ModelManifest) -> Result<Self, TranslationError>;
}

/// 校验语言对是否被支持
pub fn validate_language_pair(
    source: Language,
    target: Language,
    supported: &[(Language, Language)],
) -> Result<(), TranslationError>;
```

## 错误类型

> **定义位置**：`TranslationError` 定义在 `vtrans-core` 中（因为 `TranslationProvider` trait 需要引用它）。本模块从 `vtrans-core` 导入，不重新定义。

```rust
[derive(Debug, thiserror::Error)]
pub enum TranslationError {
    #[error("unsupported language pair: {source:?} -> {target:?}")]
    UnsupportedPair { source: Language, target: Language },
    #[error("api request failed: {0}")]
    ApiRequest(String),
    #[error("api timeout after {0:?}")]
    Timeout(Duration),
    #[error("api rate limited")]
    RateLimited,
    #[error("api unauthorized: check api key")]
    Unauthorized,
    #[error("model load failed: {0}")]
    ModelLoad(String),
    #[error("inference failed: {0}")]
    Inference(String),
    #[error("cancelled")]
    Cancelled,
    #[error("response parse error: {0}")]
    ParseResponse(String),
}
```

## 内部文件结构

```text
crates/vtrans-translation/
├── Cargo.toml
├── README.md
├── src/
│   ├── lib.rs              # re-export
│   ├── api.rs               # ApiTranslationProvider
│   ├── local_onnx.rs        # LocalTranslationProvider
│   ├── prompt.rs            # Prompt 模板构建
│   ├── retry.rs             # 重试逻辑
│   └── validate.rs          # 语言对校验
├── examples/
│   └── translation_verify.rs
└── tests/
```

## 测试计划

| 测试项 | 类型 | 说明 |
|--------|------|------|
| 语言对校验 | 单元 | Auto 源语言合法，不支持对返回错误 |
| Prompt 构建 | 单元 | 只要求译文，不含解释 |
| 超时映射 | 单元 | 超时返回 Timeout 错误 |
| 401 映射 | 单元 | HTTP 401 返回 Unauthorized |
| 429 映射 | 单元 | HTTP 429 返回 RateLimited |
| 重试逻辑 | 单元 | max_retries 次后放弃 |
| 取消传播 | 单元 | CancellationToken 触发后返回 Cancelled |
| 响应解析 | 单元 | JSON 响应提取译文 |
| 验证 CLI | 手动 | examples/translation_verify 对测试文本翻译 |

## 验收标准

- [ ] API Provider 可翻译中/日/英
- [ ] Local Provider 可加载 ONNX 模型并翻译
- [ ] 超时正确返回 Timeout
- [ ] 取消正确返回 Cancelled
- [ ] 401 返回 Unauthorized
- [ ] 重试不超过 max_retries 次
- [ ] API Key 从 CredentialManager 读取，不写明文
- [ ] Local 模型加载失败给出明确错误，不自动切换 API
- [ ] README.md 完整

## 开发注意事项

- API Provider 使用 reqwest，默认 rustls-tls
- 请求超时用 tokio::time::timeout 包装
- CancellationToken 用 tokio_util::sync::CancellationToken
- 重试使用指数退避（1s, 2s, 4s），429 不立即重试
- Local Provider 使用 ort crate，与 OCR 共用 runtime
- Prompt 模板：固定前缀 + 原文，明确要求"只输出译文"
- 日志记录 provider_id、elapsed_ms、source/target（不记录完整原文和译文）

## 本地 ONNX 接口契约

`LocalTranslationProvider` 在加载时探测模型的 I/O 形态，自动选择推理路径。支持两种接口，**生成型为主**，逐 token 兼容保留防回归。

### 生成型（Generation，推荐）

整图生成接口，beam search 由 ONNX 图内实现，单次 `session.run` 完成全部解码。

**输入张量**

| 名称 | 类型 | 形状 | 说明 |
|------|------|------|------|
| `input_ids` | int64 | `[1, src_len]` | 源语言 token ids |
| `attention_mask` | int64 | `[1, src_len]` | 全 1（无 padding） |
| `num_beams` | int64 | `[1]` | beam 数，0/1 退化为 greedy |
| `min_length` | int64 | `[1]` | 固定传 0 |
| `max_length` | int64 | `[1]` | 取 manifest `max_length` |
| `length_penalty` | float32 | `[1]` | 固定传 1.0 |
| `repetition_penalty` | float32 | `[1]` | 固定传 1.0 |

输入名通过子串匹配探测（如 `num_beams`、`max_length`），不要求精确名。

**输出张量**

| 名称 | 类型 | 形状 | 说明 |
|------|------|------|------|
| `sequences` | int64 | `[batch*beams, seq_len]` 或 `[1, seq_len]` | 生成序列 |

输出名取包含 `sequences` 的张量，或唯一输出。取第一条序列解码。

**解码流程**：从 tokenizer 的 `eos_id` 截断 → 剥离 pad/bos 等特殊 token → `tokenizer.decode` → trim → 空则返回 `Inference("decoder produced empty translation")`。

### 逐 token 型（Stepwise，兼容）

Decoder-loop 接口，每步喂入 `decoder_input_ids` 读取 `logits`，客户端做 greedy argmax。

**输入张量**：`input_ids`、`attention_mask`、`decoder_input_ids`
**输出张量**：`logits`（取最后一行 argmax）

### I/O 探测规则

1. 优先探测生成型：输入含 `num_beams`/`min_length`/`max_length`/`length_penalty`/`repetition_penalty` 且输出含 `sequences`（或唯一输出）。
2. 回退逐 token 型：输入含 `decoder_input_ids` 且输出含 `logits`（或唯一输出）。
3. 两者均不匹配时，错误信息列出两种形态的期望输入名清单。
4. 探测结果在 `debug!` 日志中记录，包含 `model_kind`、各张量名。

### 取消语义

生成型 `session.run` 是长阻塞操作，通过 `spawn_blocking` + `RunOptions::terminate()` 实现协作取消。`CancellationToken` 触发时调用 `terminate()` 中断 ONNX run，返回 `TranslationError::Cancelled`。

### manifest 参数映射

| manifest 字段 | ONNX 输入 | 说明 |
|---------------|----------|------|
| `inference_params.num_beams` | `num_beams` | 0 或 1 传 1（greedy），≥2 启用 beam search |
| `max_length` | `max_length` | 解码最大长度，与图内 `max_length` 语义对齐 |

`min_length`、`length_penalty`、`repetition_penalty` 固定传默认值（0、1.0、1.0），显式喂齐避免依赖图内默认。
