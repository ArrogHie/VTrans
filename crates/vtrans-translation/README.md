# vtrans-translation

翻译引擎模块：提供 OpenAI、DeepL、Google v2、Azure Translator、百度 5 种云端 Provider 与本地 ONNX Provider，统一支持取消、超时、有限重试和语言对校验。

## 1. 模块概述（两类读者）

本模块只负责把一段文本从源语言翻译成目标语言，并返回标准化的 `TranslationResult`。

边界：
- 做：翻译请求校验、Provider 请求构建/响应解析/错误分类/语言映射、HTTP 请求与重试、ONNX 模型加载与生成结果解码、取消与超时。
- 不做：不管理 API Key（凭据由 `vtrans-security` 保管，应用层读取后传入）；不下载或分发模型文件（属 `vtrans-models`）；不做文本清洗、指纹去重和段落切分（属 `vtrans-text`）；不采集屏幕或识别文字（属 `vtrans-capture`、`vtrans-ocr`）。

## 2. 依赖关系（消费方 / 负责人）

**上游 crate**

| crate | 本模块使用的核心概念 |
|-------|----------------------|
| `vtrans-core` | `TranslationProvider` trait、`TranslationRequest` / `TranslationResult`、`TranslationError`、`Language`；`Language` 使用其 serde 表示 `auto` / `zh-CN` / `ja` / `en` |
| `vtrans-models` | `ModelManifest`、`ModelManager`、`TranslationModelGroup`、`ModelEntry` 与 `verify_entry` 的 SHA-256 校验 |

**外部 crate**

| crate | 用途 |
|-------|------|
| `reqwest` | API 请求发送与响应读取（rustls-tls） |
| `tokio` / `tokio-util` | 异步 trait、超时与 `CancellationToken` |
| `ort` | ONNX Runtime 会话加载与 CPU 推理 |
| `tokenizers` | 解析 Hugging Face `tokenizer.json` 并编码/解码 |
| `serde` / `serde_json` | API 请求体与响应解析 |
| `md-5` / `hex` | 百度请求 MD5 签名 |
| `tracing` | 结构化日志，不记录原文、译文或密钥 |

**下游消费方**

`vtrans-pipeline` 通过 `TranslationProvider` 调用本模块；`vtrans-app` 在 `AppState` 中按配置组装对应 Provider，并从 `CredentialManager` 注入对应凭据。

## 3. 快速上手（消费方）
**OpenAI Provider**

```rust,no_run
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use vtrans_core::traits::TranslationProvider;
use vtrans_core::types::{Language, TranslationRequest};
use vtrans_translation::OpenAiProvider;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("VTRANS_API_KEY")?; // 应用层从 CredentialManager 读取
    let provider = OpenAiProvider::new(
        "https://api.example.com/v1/chat/completions",
        "translator-model",
        &api_key,
        Duration::from_secs(30),
        2,
    );
    let cancel = CancellationToken::new();
    let request = TranslationRequest::new("おはよう", Language::Japanese, Language::ChineseSimplified);
    match provider.translate(&request, cancel.clone()).await {
        Ok(result) => println!("{}", result.translated_text),
        Err(vtrans_core::TranslationError::Cancelled) => eprintln!("cancelled"),
        Err(error) => return Err(error.into()),
    }
    Ok(())
}
```

**其它云端 Provider**

DeepL、Google v2、Azure Translator、百度 的构造方式与 OpenAI 类似，仅凭据与端点不同：

```rust,ignore
use vtrans_translation::{DeepLProvider, GoogleV2Provider, AzureTranslatorProvider, BaiduProvider};

// DeepL: DeepL-Auth-Key 头
let deepl = DeepLProvider::new("https://api-free.deepl.com/v2/translate", "deepl-key", timeout, retries);

// Google v2: key 查询参数
let google = GoogleV2Provider::new("https://translation.googleapis.com/language/translate/v2", "google-key", timeout, retries);

// Azure: Ocp-Apim-Subscription-Key 头 + region
let azure = AzureTranslatorProvider::new("https://api.cognitive.microsofttranslator.com/translate", "eastasia", "azure-key", timeout, retries);

// 百度: user 透传 appid + secret，MD5 签名由 build_request 完成
let baidu = BaiduProvider::new("https://fanyi-api.baidu.com/api/trans/vip/translate", "app-id", "secret", timeout, retries);
```

**Local Provider**

```rust,no_run
use std::path::Path;

use tokio_util::sync::CancellationToken;
use vtrans_core::traits::TranslationProvider;
use vtrans_core::types::{Language, TranslationRequest};
use vtrans_models::ModelManager;
use vtrans_translation::LocalTranslationProvider;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manager = ModelManager::from_manifest_dir(Path::new("src-tauri/resources/models"))?;
    let provider = LocalTranslationProvider::from_manager(&manager)?;
    let request = TranslationRequest::new("hello", Language::English, Language::Japanese);
    let result = provider.translate(&request, CancellationToken::new()).await?;
    println!("{}", result.translated_text);
    Ok(())
}
```

生命周期：云端 Provider 创建成本低，建议长驻共享；`LocalTranslationProvider` 构造时会校验 SHA-256 并加载 ONNX 会话和 tokenizer，属于重操作，必须在后台线程执行且只创建一次。所有 Provider drop 时自动释放资源，没有需要调用方手动关闭的句柄。

## 4. 公开 API 概要（消费方）

| 类型 / 函数 | 用途 |
|-------------|------|
| `OpenAiProvider` | OpenAI-compatible chat completions 翻译（id `openai`） |
| `DeepLProvider` | DeepL v2 翻译（id `deepl`） |
| `GoogleV2Provider` | Google Cloud Translation v2（id `google`） |
| `AzureTranslatorProvider` | Azure Translator（id `azure`） |
| `BaiduProvider` | 百度通用翻译（id `baidu`） |
| `LocalTranslationProvider` | 本地 ONNX 编码器-解码器翻译 |
| `TranslationProviderAdapter` | 多 Provider 抽象层（request/parser/error/language mapper） |
| `AuthStrategy` | 鉴权抽象（Bearer / DeepL-Auth-Key / header / query / BaiduMD5） |
| `ParsedTranslation` | 解析后的多段译文中间结构 |
| `RetryPolicy` | 指数退避与可重试错误判定 |
| `validate_language_pair` | 校验 `(source, target)` 是否在 `supported_pairs` 内 |
| `build_system_prompt` / `build_translation_prompt` | 构建"只输出译文"的 Prompt |

```rust
impl OpenAiProvider {
    pub fn new(endpoint: &str, model: &str, api_key: &str, timeout: Duration, max_retries: u32) -> Self;
    pub fn with_retry_policy(self, retry_policy: RetryPolicy) -> Self;
}
impl DeepLProvider {
    pub fn new(endpoint: &str, api_key: &str, timeout: Duration, max_retries: u32) -> Self;
    pub fn with_retry_policy(self, retry_policy: RetryPolicy) -> Self;
}
impl GoogleV2Provider {
    pub fn new(endpoint: &str, api_key: &str, timeout: Duration, max_retries: u32) -> Self;
    pub fn with_retry_policy(self, retry_policy: RetryPolicy) -> Self;
}
impl AzureTranslatorProvider {
    pub fn new(endpoint: &str, region: &str, api_key: &str, timeout: Duration, max_retries: u32) -> Self;
    pub fn with_retry_policy(self, retry_policy: RetryPolicy) -> Self;
}
impl BaiduProvider {
    pub fn new(endpoint: &str, app_id: &str, secret: &str, timeout: Duration, max_retries: u32) -> Self;
    pub fn with_retry_policy(self, retry_policy: RetryPolicy) -> Self;
}
impl LocalTranslationProvider {
    pub fn from_manifest(manifest: &ModelManifest) -> Result<Self, TranslationError>;
    pub fn from_manifest_dir(manifest: &ModelManifest, models_dir: &Path) -> Result<Self, TranslationError>;
    pub fn from_manager(manager: &ModelManager) -> Result<Self, TranslationError>;
}
impl TranslationProvider for OpenAiProvider {
    fn id(&self) -> &'static str; // "openai"
    fn supported_pairs(&self) -> &[(Language, Language)];
    async fn translate(&self, request: &TranslationRequest, cancel: CancellationToken) -> Result<TranslationResult, TranslationError>;
}
impl TranslationProvider for LocalTranslationProvider {
    fn id(&self) -> &'static str; // "local-onnx"
    // supported_pairs 与 translate 签名同上
}
```

`ApiTranslationProvider` 作为 `OpenAiProvider` 的向后兼容别名保留（id 为 `openai`）。`Language` 使用 core 的 serde 表示；`TranslationRequest` 和 `TranslationResult` 可跨 JSON/IPC 传输，不含截图等二进制数据。完整签名与测试计划参见 `docs/modules/07-translation.md`。

### Provider 能力表

| Provider | 鉴权 | 中文 | 日文 | 英文 | auto 源 |
|----------|------|------|------|------|---------|
| OpenAI | `Authorization: Bearer` | GPT 模型 | GPT 模型 | GPT 模型 | 支持（prompt 层） |
| DeepL | `DeepL-Auth-Key` 头 | `ZH` | `JA` | `EN-US` | 支持（省略 source） |
| Google v2 | `key` 查询参数 | `zh-CN` | `ja` | `en` | 支持 |
| Azure | `Ocp-Apim-Subscription-Key` 头 | `zh-Hans` | `ja` | `en` | 支持 |
| 百度 | `appid + salt + MD5` 签名 | `zh` | `jp` | `en` | 支持（`auto`） |

## 5. 行为契约（消费方）

- 错误语义：`UnsupportedPair` 调用前返回，不可重试；各 Provider 通过 `map_error` 将 HTTP 状态/业务错误码映射到 `TranslationError`；`retry_decision` 按 Provider 分类决定是否重试（如 DeepL 403/456 不重试、429/500/529 重试；百度 54003/54005 限流重试、52003/54001 鉴权不重试）；若服务端给 `Retry-After`，则 `sleep(max(retry_after, local_backoff))`。
- 并发模型：所有 Provider 都是 `Send + Sync`，可跨线程共享；本地推理通过 `Mutex` 串行化 ONNX `Session::run` 和 tokenizer，并发调用安全但会排队。
- 取消语义：API 请求的取消覆盖发送与响应体读取；本地推理在每步生成前后检查 token，并调用 `RunOptions::terminate` 中断正在执行的 ONNX run。取消后返回 `TranslationError::Cancelled`。
- 资源生命周期：Provider 拥有全部资源，drop 时释放；模型加载是阻塞重操作，不要在 UI 线程或 async 上下文中直接构造。
- 边界条件：空文本本地路径返回 `Inference`；源 token 超过 `max_length` 会被截断；`target = Auto` 永远拒绝；本地 `source = Auto` 只有 manifest 显式声明 `(Auto, target)` 时才接受。
- 多段响应：DeepL/Google/Azure/百度 的多段响应被解析为 `ParsedTranslation.segments` 并以 `\n` 拼接，不丢段。

## 6. 集成注意事项（消费方）

- 坑：API Key 直接传给构造器，若硬编码会泄露。正确做法：由 `vtrans-app` 从 `CredentialManager` 读取后注入，本 crate 的 `Debug` 只输出 `****`。
- 坑：每次翻译都新建 `LocalTranslationProvider` 会重复 SHA-256 和 ONNX 加载。正确做法：启动时后台创建一次，放入 `Arc<dyn TranslationProvider>` 共享。
- 坑：本地模型缺失或哈希不匹配只返回 `ModelLoad`，不会自动回退 API。正确做法：作为显式错误展示，不静默切换。
- 坑：`supported_pairs` 只用于校验，本地 Provider 不向模型注入语言信息。正确做法：选用支持多语言输出或语言条件化的模型，并保证 manifest 声明与实际一致。
- 坑：超长文本不会自动分块。正确做法：先用 `vtrans-text` 切段，再逐段翻译。

## 7. 设计决策记录（负责人）

| 决策 | 理由 | 备选方案 |
|------|------|----------|
| 错误类型直接使用 `vtrans_core::TranslationError` | trait 签名引用该类型，跨 crate 保持一致 | 各 crate 自建错误（会导致 trait 无法编译） |
| 多 Provider 通过 `TranslationProviderAdapter` 抽象 | 鉴权/请求/解析/错误/语言差异封装在适配器，共享发送器无 `if provider` 分支 | 在发送函数硬编码分支（违背可插拔目标） |
| 鉴权抽象为 `AuthStrategy` | 避免在共享发送器硬编码 Bearer；百度签名在 Provider 内完成 | 各 Provider 各自散落鉴权代码 |
| 百度签名在 `build_request` 内完成 | Secret 只参与签名，不进入共享发送器或日志 | 共享层维护 MD5（泄露面大） |
| `map_baidu` 返回非 `Option<String>` | 百度 `auto` 也有具体代码，无需 `None` | 无意义地包 `Option` |
| 本地取消采用 `spawn_blocking` + `RunOptions::terminate` | ONNX run 是阻塞 API，只检查 token 无法中断长推理 | 仅前后检查 token（长 run 无法取消） |
| logits 取最后一个 `vocab_size` 切片 | 兼容带批次维度的输出 shape | 按 `(seq_len - 1) * vocab_size` 偏移（多批次会取错行） |

## 8. 已知限制（负责人 / 审查）

**待后续 Phase**

| 限制 | 缓解方式 |
|------|----------|
| `max_batch_size` 仅校验非零，未实现批处理 | 屏幕翻译逐段单次调用，批处理收益有限 |
| 逐 token 路径未实现 KV cache，每步把完整生成序列送入模型 | 生成型路径由图内单次 run 完成，逐 token 仅作兼容回退 |
| 本地模型语言条件化需要扩展 `ModelManifest` schema | 由 `vtrans-models` 与架构评审确认后增加语言 token 配置 |
| 长文本不分块 | 消费方切块：`vtrans-pipeline` 已按整段单次调用 + 超长切块策略实现 |

**设计使然**

| 限制 | 说明 |
|------|------|
| 本地模型加载失败不自动回退云端 | 避免静默切换导致成本或语义意外 |
| 仅支持 Hugging Face `tokenizer.json` | 与 `vtrans-models` 的 manifest 约定一致 |
| 腾讯 TMT 不接入 | 官方已撤下 `TextTranslate`，默认 Provider 不含腾讯 |
| Google 第一版仅 v2 + API Key | 未实现 v3 服务账号 OAuth；403 限流通过 body 判定 |
| 百度限流错误码 54003/54005 重试、鉴权 52003/54001 不重试 | 遵循百度错误码语义 |
| 本地 Provider 不注入 `source` / `target` | 语言行为由模型自身决定，manifest 仅声明能力 |
| beam search 由 ONNX 图内实现 | 生成型接口将 `num_beams` 传入图内，客户端不做 beam search 逻辑 |
| 逐 token 路径仅做 greedy decoding | 兼容旧模型；生成型路径为推荐接口 |

## 9. 构建与测试（两类读者）

```powershell
cargo check -p vtrans-translation
cargo test -p vtrans-translation
cargo test -p vtrans-translation --doc
cargo clippy -p vtrans-translation --all-targets
cargo fmt -p vtrans-translation -- --check
```

验证 CLI（API 模式）：

```powershell
cargo run -p vtrans-translation --example translation_verify -- `
  --text "hello" --source en --target ja `
  --api-endpoint https://api.example.com/v1/chat/completions `
  --api-model translator --api-key $env:VTRANS_API_KEY
```
验证 CLI（本地模式，需先按 manifest 放置模型）：

```powershell
cargo run -p vtrans-translation --example translation_verify -- `
  --text "hello" --source en --target ja `
  --models src-tauri/resources/models
```

## 详细规格

参见 `docs/modules/07-translation.md`。
