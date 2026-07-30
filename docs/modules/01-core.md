# 模块 01：vtrans-core 核心类型与接口

| 属性 | 值 |
|------|-----|
| Crate | `vtrans-core` |
| 分支 | `feat/01-core` |
| 层级 | 0（基础层，无上游依赖） |
| 复杂度 | 低 |
| 阶段 | Phase 0 |

## 职责

定义全项目共享的核心数据结构、Provider trait、错误类型和日志初始化工具。所有其他模块必须从本 crate 导入类型，禁止重复定义。

## 依赖

无内部 crate 依赖。外部依赖：serde, serde_json, async-trait, thiserror, tracing, tracing-subscriber, tracing-appender, tokio-util (CancellationToken)。

## 公开 API

### 类型定义 (types.rs)

Language, ScreenRegion, PixelFormat, CapturedImage, OcrLine, OcrResult, OcrOptions, TranslationRequest, TranslationResult, PipelineMode, PipelineStatus

详见 ARCHITECTURE.md 第 6.1 节。所有类型派生 Debug, Clone, Serialize, Deserialize（除 CapturedImage 仅 Clone）。

### Trait 定义 (traits.rs)

OcrProvider, TranslationProvider, CaptureSource, CaptureSession

详见 ARCHITECTURE.md 第 6.2 节。所有 trait 标注 #[async_trait]，要求 Send + Sync。

### 错误类型 (error.rs)

```rust
[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("invalid screen region: {0}")]
    InvalidRegion(String),
    #[error("unsupported language: {0:?}")]
    UnsupportedLanguage(Language),
    #[error("image format mismatch: expected {expected:?}, got {actual:?}")]
    FormatMismatch { expected: PixelFormat, actual: PixelFormat },
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
```

各下游 crate 的错误类型（OcrError, TranslationError 等）在本 crate 中仅声明为类型别名或留空，具体变体由各 crate 自行定义。

### 日志初始化 (logging.rs)

```rust
pub fn init_logging(log_dir: &Path, level: &str) -> Result<WorkerGuard>;
```

初始化 tracing-subscriber：控制台 + 滚动文件（10MB 轮转，保留 5 个）。返回 WorkerGuard 确保异步刷新。各 crate 调用 tracing::info!/warn!/error! 即可，无需自行初始化。

## 内部文件结构

```text
crates/vtrans-core/
├── Cargo.toml
├── README.md
└── src/
    ├── lib.rs          # re-export 所有公开项
    ├── types.rs        # 核心数据结构
    ├── traits.rs       # Provider trait 定义
    ├── error.rs        # CoreError
    └── logging.rs     # 日志初始化工具
```

## 测试计划

| 测试项 | 类型 | 说明 |
|--------|------|------|
| Language 序列化/反序列化 | 单元 | JSON 往返一致 |
| ScreenRegion 边界校验 | 单元 | width/height = 0 时返回 InvalidRegion |
| CapturedImage 格式检查 | 单元 | 不匹配时返回 FormatMismatch |
| 日志初始化 | 集成 | 初始化后 tracing 可正常输出 |
| CancellationToken 传递 | 单元 | cancel 后 future 返回 Cancelled |

## 验收标准

- [ ] workspace 可编译（cargo check --workspace）
- [ ] 所有公开类型有 rustdoc 注释
- [ ] 单元测试通过（cargo test -p vtrans-core）
- [ ] clippy 零警告
- [ ] README.md 包含模块职责、API 概要、构建命令

## 开发注意事项

- CancellationToken 使用 tokio_util::sync::CancellationToken，所有可取消的 trait 方法接受此参数。
- OcrLine 增加 reading_order 字段，用于排序后合并文本。
- CapturedImage 不实现 Serialize，避免图像数据被意外序列化到 JSON。
- 日志初始化函数返回 WorkerGuard，调用方需保持 guard 存活，否则异步日志可能丢失。
