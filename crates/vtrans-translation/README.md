# vtrans-translation

翻译引擎模块：为 VTrans 提供 API 和本地 ONNX 两种 `TranslationProvider` 实现，支持取消、超时、有限次数重试和语言对校验。

## 模块职责

- `ApiTranslationProvider`：通用 HTTP/JSON API 翻译，采用 OpenAI-compatible chat completions 请求格式。
- `LocalTranslationProvider`：从模型清单加载 ONNX 编码器-解码器模型与 Hugging Face `tokenizer.json`，在 CPU 上贪婪生成译文。
- 取消：所有请求响应 `CancellationToken`，本地推理通过 `RunOptions::terminate` 配合 `spawn_blocking` 中止。
- 超时与重试：API 请求按次设置超时，瞬态错误按指数退避（1s/2s/4s，封顶 8s）重试，`401`/`429` 不静默吞掉。
- Prompt 只要求输出译文，不解释内容。

## 依赖关系

### 上游 crate

- `vtrans-core`：`TranslationProvider` trait、`TranslationRequest` / `TranslationResult` / `TranslationError` / `Language`。
- `vtrans-models`：`ModelManifest`、`ModelManager`、`TranslationModelGroup`、`ModelEntry`、`verify_entry`。

### 外部 crate

| crate | 用途 | 许可证 |
|-------|------|--------|
| `reqwest` | HTTP 客户端（rustls-tls） | MIT/Apache-2.0 |
| `tokio` / `tokio-util` | 异步运行时、超时与取消令牌 | MIT |
| `ort` | ONNX Runtime 推理 | MIT |
| `tokenizers` | Hugging Face tokenizer JSON 解析与编码/解码 | Apache-2.0 |
| `serde` / `serde_json` | API 请求/响应序列化 | MIT/Apache-2.0 |
| `tracing` | 结构化日志 | MIT |
| `thiserror` | 错误类型派生（本模块使用 core 的 `TranslationError`） | MIT/Apache-2.0 |
| `async-trait` | 异步 trait | MIT/Apache-2.0 |

## 公开 API 概要

```rust
pub struct ApiTranslationProvider { /* ... */ }
impl ApiTranslationProvider {
    pub fn new(
        endpoint: &str,
        model: &str,
        api_key: &str,
        timeout: Duration,
        max_retries: u32,
    ) -> Self;
    pub fn with_retry_policy(self, retry_policy: RetryPolicy) -> Self;
}

pub struct LocalTranslationProvider { /* ... */ }
impl LocalTranslationProvider {
    pub fn from_manifest(manifest: &ModelManifest) -> Result<Self, TranslationError>;
    pub fn from_manifest_dir(manifest: &ModelManifest, models_dir: &Path)
        -> Result<Self, TranslationError>;
    pub fn from_manager(manager: &ModelManager) -> Result<Self, TranslationError>;
}

impl TranslationProvider for ApiTranslationProvider { /* id = "api" */ }
impl TranslationProvider for LocalTranslationProvider { /* id = "local-onnx" */ }

pub fn validate_language_pair(
    source: Language,
    target: Language,
    supported: &[(Language, Language)],
) -> Result<(), TranslationError>;

pub fn parse_response(body: &str) -> Result<String, TranslationError>;

pub fn build_system_prompt(source: Language, target: Language) -> String;
pub fn build_translation_prompt(text: &str, source: Language, target: Language) -> String;

pub struct RetryPolicy;
impl RetryPolicy {
    pub fn new(max_retries: u32) -> Self;
    pub fn with_limits(self, initial_backoff: Duration, max_backoff: Duration) -> Self;
}
```

错误类型直接使用 `vtrans_core::TranslationError`，本 crate 不重新定义。

## 构建与测试

```powershell
cargo build -p vtrans-translation
cargo test -p vtrans-translation
cargo clippy -p vtrans-translation --all-targets
cargo fmt --all -- --check
```

验证 CLI（手动）：

```powershell
cargo run --example translation_verify -- `
  --text "hello" --source en --target ja `
  --api-endpoint https://api.example.com/v1/chat/completions `
  --api-model translator --api-key $env:VTRANS_API_KEY

cargo run --example translation_verify -- `
  --text "hello" --source en --target ja `
  --models src-tauri/resources/models
```

## 日志与敏感数据

- 记录 `provider_id`、`model_id`、`source`/`target` 语言码、`elapsed_ms` 和译文长度，不记录原文或译文全文。
- API Key 在 `Debug` 输出中显示为 `****`，构造后仅保存在进程内存；应用层应从 `vtrans-security` 的 `CredentialManager` 读取后再传入。

## 已知限制

- API Provider 当前固定发送 OpenAI-compatible chat completions JSON；其他 API 格式需要新增适配器或可配置请求模板。
- 本地 Provider 要求模型输出最后一个解码位置的 logits，输入包含 `input_ids`、`decoder_input_ids`，可选 `attention_mask`；其他模型结构需扩展 `ModelIo` 探测逻辑。
- 本地推理使用贪婪解码，manifest 中的 `num_beams` 与 `max_batch_size` 目前仅校验非零，未实际启用 beam search 或批处理。
- 每次解码步把完整生成序列送入模型，未实现 KV cache；生成长度较长时延迟较高。
- `max_length` 会截断源 token 与生成序列，超长文本不做分块。
- 本地模型加载失败返回 `TranslationError::ModelLoad`，不会自动回退到 API Provider。
- 验证 CLI 的 API Key 从命令行参数或 `VTRANS_API_KEY` 环境变量读取，仅用于手动验证；正式应用必须走 `CredentialManager`。

## 详细规格

参见 `docs/modules/07-translation.md` 和 `docs/ARCHITECTURE.md`。