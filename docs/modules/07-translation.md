# 模块 07：vtrans-translation 翻译引擎

| 属性 | 值 |
|------|-----|
| Crate | `vtrans-translation` |
| 分支 | `feat/07-cloud-providers` |
| 上游依赖 | `vtrans-core`, `vtrans-models` |
| 层级 | 2 |
| 复杂度 | 高 |
| 阶段 | Phase 2 |

## 职责

实现 TranslationProvider trait，提供 API 翻译和本地 ONNX 翻译两种实现。支持取消、超时和有限次数重试。Prompt 明确要求只返回译文不解释内容。

## 公开 API

实现 `vtrans_core::TranslationProvider` trait，提供 5 种云端 Provider 与本地 ONNX Provider。

```rust
/// OpenAI-compatible chat completion 翻译器（id `"openai"`）
pub struct OpenAiProvider { /* ... */ }

impl OpenAiProvider {
    pub fn new(
        endpoint: &str,
        model: &str,
        api_key: &str,
        timeout: Duration,
        max_retries: u32,
    ) -> Self;
}

/// DeepL v2 翻译器（id `"deepl"`）
pub struct DeepLProvider { /* ... */ }

impl DeepLProvider {
    pub fn new(endpoint: &str, api_key: &str, timeout: Duration, max_retries: u32) -> Self;
}

/// Google Cloud Translation v2 翻译器（id `"google"`）
pub struct GoogleV2Provider { /* ... */ }

impl GoogleV2Provider {
    pub fn new(endpoint: &str, api_key: &str, timeout: Duration, max_retries: u32) -> Self;
}

/// Azure Translator 翻译器（id `"azure"`）
pub struct AzureTranslatorProvider { /* ... */ }

impl AzureTranslatorProvider {
    pub fn new(endpoint: &str, region: &str, api_key: &str, timeout: Duration, max_retries: u32) -> Self;
}

/// 百度通用翻译器（id `"baidu"`）
pub struct BaiduProvider { /* ... */ }

impl BaiduProvider {
    pub fn new(endpoint: &str, app_id: &str, secret: &str, timeout: Duration, max_retries: u32) -> Self;
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

/// 多 Provider 抽象层
pub trait TranslationProviderAdapter {
    fn id(&self) -> &'static str;
    fn map_source_language(&self, language: Language) -> Option<String>;
    fn map_target_language(&self, language: Language) -> Option<String>;
    fn build_request(&self, request: &TranslationRequest) -> Result<OutgoingRequest, TranslationError>;
    fn parse_response(&self, body: &str) -> Result<ParsedTranslation, TranslationError>;
    fn map_error(&self, status: StatusCode, body: &str) -> TranslationError;
    fn retry_decision(&self, status: StatusCode, body: &str, retry_after: Option<Duration>) -> RetryDecision;
}

/// 鉴权策略
pub enum AuthStrategy {
    Bearer,
    AuthorizationScheme(&'static str),
    Header(&'static str),
    Query(&'static str),
    BaiduMd5,
}

/// 解析后的多段译文中间结构
pub struct ParsedTranslation {
    pub segments: Vec<String>,
    pub detected_source: Option<Language>,
}
```

`ApiTranslationProvider` 作为 `OpenAiProvider` 的向后兼容别名保留，运行时 id 为 `"openai"`（不再用 `"api"`）。

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
│   ├── api.rs              # 兼容别名 + parse_response 兼容入口
│   ├── adapter.rs          # TranslationProviderAdapter / 共享发送与重试
│   ├── auth.rs             # AuthStrategy 鉴权抽象
│   ├── providers/
│   │   ├── mod.rs           # 5 个 Provider 汇总
│   │   ├── openai.rs        # OpenAiProvider
│   │   ├── deepl.rs         # DeepLProvider
│   │   ├── google.rs        # GoogleV2Provider
│   │   ├── azure.rs         # AzureTranslatorProvider
│   │   ├── baidu.rs         # BaiduProvider
│   │   └── language.rs      # 语言代码映射
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
| Retry-After | 集成 | 服务器 `Retry-After` 被遵守（`sleep(max(retry_after, local_backoff))`） |
| 取消传播 | 单元 | CancellationToken 触发后返回 Cancelled |
| 响应解析 | 单元 | JSON 响应提取译文 |
| 多段响应 | 单元 | DeepL/Google/Azure/百度 多段响应对齐拼接不丢段 |
| 百度 MD5 签名 | 单元 | `MD5(appid+q+salt+secret)` 与官方示例一致 |
| `auto` 源省略 | 单元 | DeepL/Google/Azure 在 auto 时省略源语言字段 |
| 错误码映射 | 单元 | 各 Provider 的 HTTP/业务错误码映射（401/403/429/500/529/百度 error_code） |
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

- 云端 Provider 使用 reqwest，默认 rustls-tls
- 请求超时用 tokio::time::timeout 包装
- CancellationToken 用 tokio_util::sync::CancellationToken
- 重试使用指数退避（1/2/4/8s），若 provider 给 `Retry-After` 则 `sleep(max(retry_after, local_backoff))`；限流错误按 provider 分类决定是否重试
- Local Provider 使用 ort crate，与 OCR 共用 runtime
- Prompt 模板：固定前缀 + 原文，明确要求"只输出译文"
- 日志记录 provider_id、elapsed_ms、source/target（不记录完整原文和译文）

## 多 Provider 架构

云端翻译通过 [`TranslationProviderAdapter`] 抽象，把 Provider 差异（鉴权、请求体、响应解析、错误分类、语言映射）封装在各自适配器中。共享发送器 `send_with_adapter` 负责超时、取消、重试与 `Retry-After` 处理，不包含任何 `if provider == ...` 分支。

### 鉴权

`AuthStrategy` 枚举覆盖：

| 策略 | 说明 | 使用方 |
|------|------|--------|
| `Bearer` | `Authorization: Bearer <key>` | OpenAI |
| `AuthorizationScheme` | `DeepL-Auth-Key: <key>` | DeepL |
| `Header` | `Ocp-Apim-Subscription-Key: <key>` | Azure |
| `Query` | `?key=<key>` | Google v2 |
| `BaiduMd5` | `appid + q + salt + secret` MD5 签名 | 百度（在 Provider `build_request` 内完成） |

### 语言代码映射

| Provider | auto | zh-CN | ja | en |
|----------|------|-------|----|----|
| OpenAI | prompt 层 | 语言名 | 语言名 | 语言名 |
| DeepL | 省略 source | `ZH` | `JA` | `EN-US` |
| Google | 省略 source | `zh-CN` | `ja` | `en` |
| Azure | 省略 from | `zh-Hans` | `ja` | `en` |
| 百度 | `auto` | `zh` | `jp` | `en` |

### 错误分类与重试

| Provider | 不重试 | 可重试 |
|----------|--------|--------|
| OpenAI | 401 | 429/500/503 |
| DeepL | 403/456 | 429/500/529 |
| Google | 401、非限流 403 | 429/500/503、限流 403（body 含 `RATE_LIMIT`/`dailyLimitExceeded`） |
| Azure | 401/403 | 429/500/503 |
| 百度 | 52003/54001/58001 | 54003/54005（限流） |

百度业务错误由 200 响应体中的 `error_code` 判定（`52003`/`54001` 鉴权 → `Unauthorized`，`54003`/`54005` 限流 → `RateLimited`，`58001` 不支持方向 → `UnsupportedPair`）。

### `ParsedTranslation` 多段拼接

DeepL/Google/Azure/百度 的多段响应被解析为 `ParsedTranslation.segments`，由共享发送器以 `\n` 拼接为最终译文，保证多段不丢。`detected_source` 在 API 报告检测语言时填充。

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
