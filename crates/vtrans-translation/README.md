# vtrans-translation

翻译引擎模块：提供 API 与本地双引擎两种 `TranslationProvider` 实现，统一支持取消、超时、有限重试和语言对校验。

## 1. 模块概述（两类读者）

本模块只负责把一段文本从源语言翻译成目标语言，并返回标准化的 `TranslationResult`。

边界：

- 做：翻译请求校验、Prompt 构建、HTTP 请求与重试、本地双引擎（Bergamot en→zh + CTranslate2 INT8 ja→zh）加载与翻译、取消与超时。
- 不做：不管理 API Key（凭据由 `vtrans-security` 保管，应用层读取后传入）；不下载或分发模型文件（属 `vtrans-models` 与 `scripts/translation/`）；不做文本清洗、指纹去重和段落切分（属 `vtrans-text`）；不采集屏幕或识别文字（属 `vtrans-capture`、`vtrans-ocr`）。

自 v0.3.0 起，本地翻译从「单 ONNX 模型 + tokenizer」升级为「双引擎原生实现」（决策 A3：旧 ONNX 路径 `local_onnx.rs` 已彻底删除，不保留双维护）。本地运行时 id 从 `"local-onnx"` 改为 `"local-native"`（决策 A2）。

## 2. 依赖关系（消费方 / 负责人）

**上游 crate**

| crate | 本模块使用的核心概念 |
|-------|----------------------|
| `vtrans-core` | `TranslationProvider` trait、`TranslationRequest` / `TranslationResult`、`TranslationError`、`Language` |
| `vtrans-models` | `ModelManager`、manifest v2 双引擎条目、`en_zh_paths()` / `ja_zh_paths()` 路径解析 |

**外部 crate / 原生依赖**

| 依赖 | 用途 |
|------|------|
| `reqwest` | API 请求发送与响应读取（rustls-tls） |
| `tokio` / `tokio-util` | 异步 trait、超时与 `CancellationToken` |
| `libloading` | 动态加载 `translation_bridge.dll`（无链接期依赖） |
| `serde` / `serde_json` | API 请求体/响应解析、`TranslationQuality` serde |
| `tracing` | 结构化日志，不记录原文、译文或密钥 |
| 原生 `translation_bridge.dll` | Bergamot v0.4.5 + CTranslate2 4.8.1 + SentencePiece 封装（见 `native/translation_bridge/`） |

**下游消费方**

`vtrans-pipeline` 通过 `TranslationProvider` 调用本模块；`vtrans-app` 在 `AppState` 中组装 Provider、注入 API Key，并从 `AppConfig.translation.quality` 读取质量档位后调用 `with_quality`。

## 3. 快速上手（消费方）

**API Provider**

```rust,no_run
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use vtrans_core::traits::TranslationProvider;
use vtrans_core::types::{Language, TranslationRequest};
use vtrans_translation::ApiTranslationProvider;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("VTRANS_API_KEY")?; // 应用层从 CredentialManager 读取
    let provider = ApiTranslationProvider::new(
        "https://api.example.com/v1/chat/completions",
        "translator-model",
        &api_key,
        Duration::from_secs(30),
        2,
    );
    let request = TranslationRequest::new("おはよう", Language::Japanese, Language::ChineseSimplified);
    let result = provider.translate(&request, CancellationToken::new()).await?;
    println!("{}", result.translated_text);
    Ok(())
}
```

**Native Provider（双引擎）**

```rust,no_run
use std::path::Path;

use vtrans_models::ModelManager;
use vtrans_translation::{NativeTranslationProvider, TranslationQuality};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 阻塞重操作：应用层应在 spawn_blocking 中执行一次，结果放入
    // Arc<dyn TranslationProvider> 共享。
    let manager = ModelManager::from_manifest_dir(Path::new("src-tauri/resources/models"))?;
    let provider = NativeTranslationProvider::from_manager(&manager)?
        .with_quality(TranslationQuality::Balanced)?; // 默认 Fast
    println!("provider id: {}", provider.id());
    Ok(())
}
```

生命周期：`ApiTranslationProvider` 创建成本低，建议长驻共享；`NativeTranslationProvider` 构造时会校验 manifest、定位并加载 `translation_bridge.dll`、加载两个引擎与 SentencePiece 词表，属于阻塞重操作，必须放在后台线程且只创建一次（指南 §15）。两者 drop 时自动释放资源。

## 4. 公开 API 概要（消费方）

| 类型 / 函数 | 用途 |
|-------------|------|
| `ApiTranslationProvider` | OpenAI-compatible chat completions 翻译 |
| `NativeTranslationProvider` | 本地双引擎翻译（Bergamot en→zh + CTranslate2 ja→zh） |
| `TranslationQuality` | 质量档位 `Fast` / `Balanced`（serde `"fast"` / `"balanced"`） |
| `NativeTranslator` | 原生引擎句柄封装（`Send + Sync`，Drop 自动释放） |
| `RetryPolicy` | 指数退避与可重试错误判定 |
| `validate_language_pair` | 校验 `(source, target)` 是否在 `supported_pairs` 内 |
| `parse_response` | 从 JSON 响应提取译文 |
| `build_system_prompt` / `build_translation_prompt` | 构建"只输出译文"的 Prompt |

```rust
pub enum TranslationQuality { Fast, Balanced }          // serde: "fast" / "balanced"

impl NativeTranslationProvider {
    pub fn from_manager(manager: &ModelManager) -> Result<Self, TranslationError>;
    pub fn with_quality(self, quality: TranslationQuality) -> Result<Self, TranslationError>;
    pub const fn quality(&self) -> TranslationQuality;
    pub fn model_id(&self) -> &str;
}

impl TranslationProvider for NativeTranslationProvider {
    fn id(&self) -> &'static str;              // "local-native"
    fn supported_pairs(&self) -> &[(Language, Language)];
    async fn translate(&self, request, cancel) -> Result<TranslationResult, TranslationError>;
}
```

本地支持的语言对：`(en, zh-CN)` 与 `(ja, zh-CN)`。`Auto` 源语言由上层（pipeline）解析为具体语言后传入，Provider 拒绝 `Auto` 与不支持对（`TranslationError::UnsupportedPair`）。

## 5. 行为契约（消费方）

- 错误语义：`UnsupportedPair` 调用前返回，不可重试；`ApiRequest`、`Timeout`、`RateLimited` 可重试，`429` 使用指数退避而非立即重试；`Unauthorized`、`ParseResponse`、`Cancelled` 不重试；`ModelLoad`、`Inference` 为终止性错误。
- 错误码映射（原生桥错误码 → `TranslationError`，指南 §21）：1 invalid argument → `Inference`；2 unsupported language → `UnsupportedPair`；3 model not loaded → `ModelLoad`；4 tokenizer → `ModelLoad`；5 inference → `Inference`；6 encoding → `Inference`；7 version mismatch → `ModelLoad`。不新增 core 错误变体。
- 并发模型：两个 Provider 都是 `Send + Sync`，可跨线程共享；原生引擎在桥内用互斥锁串行化，线程上限固定（Bergamot 2、CTranslate2 intra 2 / inter 1，决策 B5），不随核心数膨胀。
- 取消语义（决策 B3）：API 请求的取消覆盖发送与响应体读取；原生推理是**不可中断阻塞**调用，`translate` 在 `spawn_blocking` 中执行，仅在调用前与调用后检查 `CancellationToken`。取消后返回 `TranslationError::Cancelled`；进行中的原生推理无法中断（见已知限制）。
- 质量档位：`Fast` 默认；Bergamot beam 1 / CTranslate2 beam 1。`Balanced`：Bergamot beam 2 / CTranslate2 beam 4。桥内 `translation_set_quality` 切换时若目标为 Balanced 会重建 Bergamot 模型，因此只在 Provider 组装期调用一次。
- 边界条件：空文本返回 `Inference`；含 NUL 的输入返回 `Inference`；`target = Auto` 永远拒绝；本地 `source = Auto` 永远拒绝（由上层解析）。超长文本按 `max_input_tokens = 256` 由桥内截断（`max_input_length`），消费方应先用 `vtrans-text` 分块。

## 6. 集成注意事项（消费方）

- 坑：API Key 直接传给构造器，若硬编码会泄露。正确做法：由 `vtrans-app` 从 `CredentialManager` 读取后注入，本 crate 的 `Debug` 只输出 `****`。
- 坑：每次翻译都新建 `NativeTranslationProvider` 会重复定位 DLL、加载模型与 SHA-256 校验。正确做法：启动时后台创建一次，放入 `Arc<dyn TranslationProvider>` 共享。
- 坑：本地模型缺失、哈希不匹配、DLL 缺失或 ABI 版本不匹配只返回 `ModelLoad`，不会自动回退 API。正确做法：作为显式错误展示，不静默切换。
- 坑：`translation_bridge.dll` 构建产物输出到 `src-tauri/resources/native/`（随包资源由 10 任务在 `tauri.conf.json` 声明）；Rust 侧查找顺序为「模型目录同级 `native/` → 模型目录内 `native/` → 可执行文件旁 `native/` → 可执行文件旁 `resources/native/`」。
- 坑：不要在 async 上下文直接调用 `from_manager` / `with_quality`（阻塞重操作）。正确做法：`tokio::task::spawn_blocking` 中执行。

## 7. 设计决策记录（负责人）

| 决策 | 理由 | 备选方案 |
|------|------|----------|
| 错误类型直接使用 `vtrans_core::TranslationError` | trait 签名引用该类型，跨 crate 保持一致 | 各 crate 自建错误（会导致 trait 无法编译） |
| 本地引擎通过 C ABI 桥封装（`native/translation_bridge/`） | 避免 Rust 直接绑定大量 C++ 模板类型；C 面保持最小 | bindgen 全量绑定（维护成本高） |
| `libloading` 动态加载 DLL | 无链接期依赖，DLL 缺失时编译仍可通过、运行时明确报 `ModelLoad` | 静态链接（打包复杂度高） |
| `Auto` 源由上层解析，Provider 拒绝 `Auto` | 桥内没有语言检测能力；拒绝而非猜测，保证契约明确 | Provider 内做启发式（指南明确不推荐） |
| 取消语义退化为调用前后检查（B3） | 原生推理不可中断，`spawn_blocking` 保持 async 调用方不被阻塞 | 尝试 terminate（原生 API 不支持） |
| 质量档位只暴露 Fast/Balanced | 不向普通用户暴露 beam 等专业参数（指南 §7） | 直接暴露 beam（易误用） |

## 8. 已知限制（负责人 / 审查）

**待后续 Phase**

| 限制 | 缓解方式 |
|------|----------|
| 原生推理进行中无法中断（B3） | 调用前后检查 token；进行中的请求完成后丢弃结果 |
| 翻译批处理未实现（指南 §17） | 当前逐段单次调用；批处理登记为后续优化 |
| 超长文本在桥内按 `max_input_length=256` 截断 | 消费方用 `vtrans-text` 先做标点感知分块（09 pipeline） |
| ja-zh 质量基准首次建立 | `tests/fixtures/regression_samples.json` 固定样本 + `--ignored` 集成测试 |

**设计使然**

| 限制 | 说明 |
|------|------|
| 本地模型加载失败不自动回退 API | 避免静默切换导致成本或语义意外 |
| 只支持 en→zh-CN / ja→zh-CN | 双引擎各自只有一对；`zh-CN` 源语言本地不支持（API 仍可） |
| 桥内互斥锁串行化全部原生调用 | 线程预算固定（B5），牺牲并发换取 CPU 不膨胀 |
| Balanced 切换重建 Bergamot 模型 | beam 是模型构造期参数；只在组装期调用一次 |
| DLL 构建产物不入库 | `.gitignore` 忽略 `src-tauri/resources/native/`，由 `build.ps1` 产出 |

## 9. 构建与测试（两类读者）

```powershell
cargo check -p vtrans-translation
cargo test -p vtrans-translation
cargo test -p vtrans-translation --doc
cargo clippy -p vtrans-translation --all-targets
cargo fmt -p vtrans-translation -- --check
```

原生 DLL 构建（需要预构建的 Bergamot / CTranslate2 / SentencePiece 产物，见 `native/translation_bridge/README.md`）：

```powershell
.\native\translation_bridge\build.ps1 -DepsRoot D:\deps\translation
```

验证 CLI（API 模式）：

```powershell
cargo run -p vtrans-translation --example translation_verify -- `
  --text "hello" --source en --target ja `
  --api-endpoint https://api.example.com/v1/chat/completions `
  --api-model translator --api-key $env:VTRANS_API_KEY
```

验证 CLI（本地双引擎模式，需模型 + DLL）：

```powershell
cargo run -p vtrans-translation --example translation_verify -- `
  --text "hello" --source en --target zh-CN `
  --models src-tauri/resources/models --quality balanced
```

真实模型回归（`--ignored`）：

```powershell
cargo test -p vtrans-translation --test native_provider -- --ignored
```

## 详细规格

参见 `docs/modules/07-translation.md`。
