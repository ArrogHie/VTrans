# vtrans-core

## 1. 模块概述

定义 VTrans 全项目共享的数据结构、Provider trait、错误类型与日志工具，是所有其他 crate 的依赖基础。

边界：
- 提供类型定义与校验，不实现屏幕采集、OCR、翻译业务（分别属于 `vtrans-capture`、`vtrans-ocr`、`vtrans-translation`）
- 不管理配置、凭据、模型文件（分别属于 `vtrans-config`、`vtrans-security`、`vtrans-models`）
- 不编排捕获-识别-翻译流程（属于 `vtrans-pipeline`）
- 不接触 Tauri IPC 与前端（属于 `vtrans-app` 和 `src/`）
- 定义 `CaptureError`、`OcrError`、`TranslationError`，但产生这些错误的实现在各下游 crate

## 2. 依赖关系

| 类别 | 项 | 用途 |
|------|----|------|
| 上游 crate | 无 | 层级 0，依赖图根节点 |
| 外部 | `serde` / `serde_json` | 跨 IPC 边界的序列化与反序列化 |
| 外部 | `async-trait` | 异步 trait 方法 |
| 外部 | `thiserror` | 错误枚举派生 |
| 外部 | `tracing` / `tracing-subscriber` / `tracing-appender` | 结构化日志与轮转文件输出 |
| 外部 | `tokio-util` | `CancellationToken` 协作式取消 |

下游消费方（来自 `docs/ARCHITECTURE.md` 依赖表）：`vtrans-config`、`vtrans-security`、`vtrans-capture`、`vtrans-ocr`、`vtrans-text`、`vtrans-translation`、`vtrans-models`、`vtrans-pipeline`、`vtrans-app`。

这些消费方需要本模块提供：

- `Language`（serde 表示 `auto`/`zh-CN`/`ja`/`en`）、`ScreenRegion`、`OcrResult`、`TranslationRequest` 等跨模块类型
- `OcrProvider`、`TranslationProvider`、`CaptureSource`、`CaptureSession` 四个 trait
- `CaptureError`、`OcrError`、`TranslationError`（trait 的返回类型）
- `init_logging`、`mask_sensitive`、`truncate_for_log`（日志红线标准）
## 3. 快速上手

以下示例依赖 `vtrans-core`、`async-trait`、`tokio-util`、`tokio`（`rt-multi-thread` + `macros`）。

```rust
use vtrans_core::error::OcrError;
use vtrans_core::logging::init_logging;
use vtrans_core::traits::OcrProvider;
use vtrans_core::types::{
    CapturedImage, Language, OcrOptions, OcrResult, PixelFormat, ScreenRegion,
};
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

/// 最小 provider 实现：未取消时返回空结果。
struct EchoOcr;

#[async_trait]
impl OcrProvider for EchoOcr {
    fn id(&self) -> &'static str {
        "echo-ocr"
    }

    async fn recognize(
        &self,
        _image: &CapturedImage,
        _region: &ScreenRegion,
        _options: &OcrOptions,
        cancel: CancellationToken,
    ) -> Result<OcrResult, OcrError> {
        if cancel.is_cancelled() {
            return Err(OcrError::Cancelled);
        }
        Ok(OcrResult::empty())
    }

    fn supported_languages(&self) -> &[Language] {
        &[Language::English, Language::ChineseSimplified]
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 初始化日志；guard 必须活到进程结束，否则异步日志可能丢失
    let _guard = init_logging(&std::env::temp_dir().join("vtrans-logs"), "info")?;

    // 2. 构造并校验输入
    let region = ScreenRegion::new("monitor-0", 0, 0, 1920, 1080);
    region.validate()?;
    let image = CapturedImage::new(1920, 1080, PixelFormat::Bgra8, vec![0; 1920 * 1080 * 4])?;
    image.check_format(PixelFormat::Bgra8)?;

    // 3. 通过 trait 调用；预取消的 token 立即返回 Cancelled
    let provider = EchoOcr;
    let ok = provider
        .recognize(
            &image,
            &region,
            &OcrOptions::new(Language::English),
            CancellationToken::new(),
        )
        .await?;
    assert!(ok.lines.is_empty());

    let cancel = CancellationToken::new();
    cancel.cancel();
    let err = provider
        .recognize(&image, &region, &OcrOptions::default(), cancel)
        .await
        .unwrap_err();
    assert!(matches!(err, OcrError::Cancelled));
    Ok(())
}
```

## 4. 公开 API 概要

| 项 | 用途 |
|----|------|
| `Language` | 语言标识，IPC 序列化为 `auto`/`zh-CN`/`ja`/`en` |
| `ScreenRegion` | 显示器上的矩形区域，坐标为物理像素 |
| `PixelFormat` | 像素格式，序列化为 `rgba8`/`bgra8` |
| `CapturedImage` | 捕获帧（含原始像素），**不实现 `Serialize`** |
| `OcrLine` / `OcrResult` / `OcrOptions` | OCR 单行、结果与识别参数 |
| `TranslationRequest` / `TranslationResult` | 翻译请求与结果 |
| `PipelineMode` / `PipelineStatus` | 流水线模式与状态，序列化为 `single`/`live` 与状态字符串 |
| `CoreError` | 类型校验与序列化错误 |
| `CaptureError` / `OcrError` / `TranslationError` | provider 返回错误（由下游实现产生） |
| `OcrProvider` / `TranslationProvider` / `CaptureSource` / `CaptureSession` | 统一 provider trait |
| `init_logging` / `mask_sensitive` / `truncate_for_log` | 日志初始化与脱敏工具 |

核心方法签名：

```rust
// 校验：失败返回 CoreError，输入修正后可重试
pub fn ScreenRegion::validate(&self) -> Result<(), CoreError>;
pub fn CapturedImage::check_format(&self, expected: PixelFormat) -> Result<(), CoreError>;
pub fn CapturedImage::validate(&self) -> Result<(), CoreError>;
// 构造：校验尺寸与数据长度，返回 Result 而非 panic
pub fn CapturedImage::new(
    width: u32, height: u32, format: PixelFormat, data: Vec<u8>,
) -> Result<Self, CoreError>;
// 日志工具
pub fn init_logging(log_dir: &Path, level: &str) -> Result<WorkerGuard, std::io::Error>;
pub fn mask_sensitive(s: &str) -> String;   // "sk-****1234"
pub fn truncate_for_log(s: &str) -> String; // 前 20 字符 + "..."
```

trait 完整签名与错误变体清单见 `docs/modules/01-core.md`，本 README 不重复。

## 5. 行为契约

| 维度 | 约定 |
|------|------|
| 错误语义 | `CoreError::InvalidRegion`（尺寸或数据长度非法，输入修正后可重试）；`FormatMismatch`（格式不符，可重试）；`Serialization`（JSON 数据问题）；各 provider 的 `Cancelled` 不可重试，`RateLimited` 可退避重试 |
| 并发模型 | 所有 trait 要求 `Send + Sync`；数据类型本身 `Send`；`init_logging` 是进程级全局单次操作 |
| 取消语义 | 调用方创建 `CancellationToken` 并传入 provider；预取消的 token 让 provider 立即返回对应 `Cancelled` 错误；取消生效点由实现决定，实现必须在返回前检查 |
| 资源生命周期 | `init_logging` 返回的 `WorkerGuard` 必须保持存活；`CaptureSession` 由调用方调用 `stop`；模型句柄由下游 provider 实现管理 |
| 边界条件 | region/image 零尺寸 → `InvalidRegion`；数据长度不匹配 → `InvalidRegion`；超大图像尺寸触发溢出保护 → `InvalidRegion`；`mask_sensitive` 输入 ≤8 字符 → `****`；`truncate_for_log` 截断至 20 字符加 `...` |

## 6. 集成注意事项

| 坑 | 正确做法 |
|----|----------|
| `CapturedImage` 不能序列化，图像无法经 Tauri IPC 传输 | 图像留在 Rust 侧，IPC 只传文本、状态、缩略图 |
| `init_logging` 只能成功调用一次，重复调用返回 `Err` | 在 app 入口调用一次并持有 guard 到进程结束 |
| provider 方法不响应取消时，长任务无法中断 | 在 trait 实现中用 `select!` 或 `is_cancelled()` 检查 |
| 日志直接记录完整原文、译文、密钥或图像 | 一律使用 `mask_sensitive` 和 `truncate_for_log` |
| `ScreenRegion` 坐标是物理像素且相对显示器 | DPI 换算由 `vtrans-capture` 负责，调用方不要自行缩放 |
| provider 的 `id()` 返回不稳定的字符串 | `id()` 必须是固定的 `&'static str`，用于事件和 UI 标识 |

## 7. 设计决策记录

| 决策 | 理由 | 备选方案 |
|------|------|----------|
| 错误类型集中在 core 定义 | trait 签名跨 crate 引用同一类型，保证契约一致 | 各 crate 自建错误（trait 无法编译） |
| `CapturedImage` 不实现 `Serialize` | 防止图像数据经 JSON 泄漏到 IPC | 实现序列化（违反安全红线） |
| 日志按小时轮转、保留 5 个文件 | tracing-appender 不支持按大小轮转 | 引入第三方 rolling 库（体积与复杂度，放弃） |
| `mask_sensitive` 按字符处理 | 字节切片对多字节 UTF-8 输入会 panic | 字节截断（日志路径崩溃风险） |
| `init_logging` 重复调用返回 `Err` | 全局 subscriber 只能设置一次，失败比 panic 可恢复 | 直接 `.init()`（重复调用 panic） |
| `ort` pin 到 `2.0.0-rc.13` | 仓库骨架的 `ort = "2"` 在 crates.io 无匹配版本，阻塞全部编译 | 等待 stable（无法构建） |

## 8. 已知限制

| 类型 | 限制 | 缓解/规避 |
|------|------|-----------|
| 待后续 Phase | 具体 OCR/翻译/采集 provider 实现在本模块之外 | 由 04/05/07 模块按 trait 实现 |
| 设计使然 | 全局日志只能初始化一次 | app 入口单点初始化 |
| 设计使然 | `CapturedImage` 不可序列化 | 图像留在 Rust 侧，IPC 传元数据 |
| 设计使然 | 日志按小时轮转而非按大小 | 桌面应用日志量下可接受 |
| 兼容性 | `ort` 使用 RC 版本，API 可能变化 | stable 发布后由 build 修复更新 |
| 性能 | `mask_sensitive` / `truncate_for_log` 为 O(n) 遍历 | 输入均为短文本，日志路径开销可忽略 |
| 正确性 | `mask_sensitive` 按 Unicode scalar 切分，emoji 组合序列可能被拆开 | 仅影响日志展示，不影响脱敏安全性 |

## 9. 构建与测试

```powershell
cargo check -p vtrans-core
cargo test -p vtrans-core
cargo clippy -p vtrans-core --all-targets
cargo fmt -p vtrans-core -- --check
```

`cargo test` 包含单元测试、集成测试（`tests/logging_init.rs`）与文档测试。本模块无 CLI，无验证命令。

## 10. 详细规格引用

参见 `docs/modules/01-core.md`。