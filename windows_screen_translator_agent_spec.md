# Windows 屏幕翻译工具：项目规划与开发规格

> 文档用途：交给开发 Agent 进行任务拆解、实现与验收  
> 目标平台：Windows 10/11 x64  
> 技术栈：Rust + Tauri 2 + TypeScript  
> 当前阶段：MVP

## 1. 项目目标

开发一款 Windows 桌面翻译工具，支持：

1. 手动框选屏幕区域并翻译。
2. 持续读取固定屏幕区域，在内容变化时自动 OCR 和翻译。
3. 使用本地 OCR 模型识别日语、英语。
4. 支持中文、日语、英语互译。
5. 翻译引擎可切换 API 或本地模型。
6. 通过独立结果窗口展示原文和译文。

## 2. 当前范围

### 必须实现

- Windows 单平台。
- 单次截屏翻译。
- 固定区域实时翻译。
- 全局快捷键。
- 日语、英语 OCR。
- 中、日、英语言选择与自动源语言判断。
- API 翻译适配器。
- 本地翻译模型适配器。
- 可置顶、可拖动、可调整大小的结果窗口。
- OCR/翻译取消、超时、错误提示。
- 基础设置持久化。

### 暂不实现

- 覆盖或替换屏幕原文。
- 翻译历史记录。
- 云端 OCR。
- 移动端和 macOS/Linux。
- 用户账号与同步。
- 图片文件批量翻译。
- 自动更新与插件市场。

## 3. 推荐技术选型

- 应用框架：Tauri 2。
- 前端：React + TypeScript + Vite。
- Rust 异步运行时：Tokio。
- Windows API：`windows` crate。
- 屏幕采集：Windows Graphics Capture。
- 图像处理：`image` crate，必要时增加 OpenCV。
- 本地推理：ONNX Runtime，Rust 侧使用 `ort`。
- 序列化：Serde。
- 配置：JSON，存储到 Tauri AppConfig 目录。
- API 密钥：Windows Credential Manager，不写入明文配置。
- 日志：`tracing` + 滚动日志文件。
- 错误模型：`thiserror`，应用边界使用 `anyhow`。

## 4. 总体架构

```text
Tauri Frontend
├── Main Window：模式、语言、引擎、设置
├── Region Selector：透明全屏选区
└── Result Window：原文、译文、状态
          │ Commands / Events
          ▼
Rust Application Layer
├── AppState
├── Translation Pipeline
├── Task Cancellation
└── Configuration
          │
          ├── Screen Capture
          ├── Image Preprocess
          ├── OCR Provider
          ├── Text Normalizer
          ├── Translation Provider
          └── Result Publisher
```

核心原则：屏幕采集、OCR、翻译、展示互相隔离，主流程只依赖统一接口和标准数据结构。

## 5. 核心模块

### 5.1 屏幕采集 `capture`

职责：

- 获取显示器和窗口信息。
- 处理单次截图。
- 建立持续捕获会话。
- 根据物理像素坐标裁剪用户选择的区域。
- 处理多显示器、缩放比例和负坐标。
- 输出统一的 RGBA/BGRA 图像帧。

实现建议：

- 使用 Windows Graphics Capture 获取显示器帧。
- 固定区域实时模式不必每次重新创建捕获会话。
- 应用启用 Per-Monitor DPI Awareness V2。
- 选区坐标统一转换为物理像素后再传给 Rust。
- 第一阶段仅处理 SDR；HDR 屏幕需正确转换为 8-bit SDR 图像。

### 5.2 选区窗口 `region_selector`

实现为透明、无边框、置顶的 Tauri 窗口：

- 覆盖目标显示器或虚拟桌面。
- 鼠标拖动生成矩形区域。
- 显示边框和尺寸。
- `Esc` 取消，`Enter` 确认。
- 返回 `monitor_id + x + y + width + height`。
- 选区期间暂停当前实时任务。

### 5.3 OCR `ocr`

定义统一接口：

```rust
#[async_trait::async_trait]
pub trait OcrProvider: Send + Sync {
    async fn recognize(
        &self,
        image: OcrImage,
        options: OcrOptions,
    ) -> Result<OcrResult, OcrError>;
}
```

建议模型结构：

- 文本检测：轻量级 PP-OCR 检测模型，ONNX 格式。
- 英文识别：英文轻量识别模型。
- 日文识别：日文轻量识别模型。
- 可选升级：使用同时覆盖中文、英文、日文的 PP-OCRv5 多语言识别模型。
- 模型、字符字典、预处理参数必须通过模型清单配置，不写死在业务代码中。

OCR 流程：

```text
图像裁剪
→ 缩放与归一化
→ 文本检测
→ 文本框排序
→ 透视裁剪/旋转
→ 文字识别
→ CTC 解码
→ 置信度过滤
→ 合并文本行
```

要求：

- 支持日文横排和常见竖排文本。
- 支持英文。
- 保留文字框、文本、置信度和阅读顺序。
- OCR 模型只初始化一次。
- 默认 CPU 推理；DirectML 作为后续优化项。
- 首次启动检查模型资源完整性和 SHA-256。

### 5.4 文本标准化 `text_normalizer`

职责：

- 清除异常空格和不可见字符。
- 合并属于同一段的 OCR 行。
- 保留必要换行。
- 日文标点规范化。
- 计算文本指纹，避免重复翻译。
- 限制单次发送长度并按段落切分。

不要在此模块修改专有名词或改变原意。

### 5.5 翻译 `translation`

统一接口：

```rust
#[async_trait::async_trait]
pub trait TranslationProvider: Send + Sync {
    fn id(&self) -> &'static str;

    async fn translate(
        &self,
        request: TranslationRequest,
    ) -> Result<TranslationResult, TranslationError>;
}
```

语言枚举：

```rust
pub enum Language {
    Auto,
    ChineseSimplified,
    Japanese,
    English,
}
```

提供两个实现：

#### API Translator

- 第一阶段实现一个通用 HTTP/JSON Provider。
- 可增加 OpenAI-compatible Provider。
- API URL、模型名、超时可配置。
- API Key 从 Windows Credential Manager 读取。
- 请求必须支持取消、超时和有限次数重试。
- Prompt 明确要求只返回译文，不解释内容。

#### Local Translator

- 使用 ONNX Runtime。
- Provider 不绑定具体模型名称。
- 模型清单定义模型路径、Tokenizer、支持语言、最大长度和推理参数。
- MVP 可采用一个覆盖中/日/英的多语言模型。
- 本地模型加载失败时给出明确错误，不自动切换到联网 API。
- 模型下载和许可证检查可留到后续版本；MVP 可要求用户手动放置模型。

## 6. 实时翻译流水线

```text
Capture Session
    ↓
Crop Selected Region
    ↓
Frame Difference Check
    ↓
Bounded Channel
    ↓
OCR Worker
    ↓
Text Fingerprint Check
    ↓
Translation Worker
    ↓
Tauri Event
    ↓
Result Window
```

关键策略：

- 默认采样间隔：500 ms，可配置为 250–2000 ms。
- 先进行低成本图像差异检测，区域无明显变化则跳过 OCR。
- Channel 容量设为 1，只保留最新帧。
- 新任务到达时取消未完成的旧翻译任务。
- OCR 文本指纹不变时不重复翻译。
- 实时模式任意时刻最多运行一个 OCR 和一个翻译任务。
- 窗口最小化、锁屏、显示器断开时暂停或安全重建会话。

## 7. 前后端通信

### Commands

```text
start_region_selection
capture_once
start_live_translation
stop_live_translation
update_live_region
set_ocr_language
set_translation_provider
load_local_models
save_settings
get_app_status
```

### Events

```text
capture_status_changed
ocr_started
ocr_completed
translation_started
translation_completed
pipeline_error
model_loading_progress
live_session_stopped
```

大图像不要通过 JSON/Base64 在前后端频繁传输。图像应留在 Rust 侧，前端只接收文本、状态和必要的缩略图。

## 8. 核心数据结构

```rust
pub struct ScreenRegion {
    pub monitor_id: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub struct OcrLine {
    pub text: String,
    pub confidence: f32,
    pub polygon: [[f32; 2]; 4],
}

pub struct OcrResult {
    pub lines: Vec<OcrLine>,
    pub merged_text: String,
    pub detected_language: Option<Language>,
    pub elapsed_ms: u64,
}

pub struct TranslationRequest {
    pub text: String,
    pub source: Language,
    pub target: Language,
}

pub struct TranslationResult {
    pub translated_text: String,
    pub provider_id: String,
    pub elapsed_ms: u64,
}

pub enum PipelineMode {
    SingleCapture,
    LiveRegion,
}
```

## 9. UI 页面

### 主窗口

- 模式：单次翻译 / 实时翻译。
- 按钮：选择区域、开始、暂停、停止。
- OCR：自动、日语、英语。
- 源语言：自动、中文、日语、英语。
- 目标语言：中文、日语、英语。
- 翻译引擎：API / 本地。
- 当前状态与简要错误信息。
- 设置入口。

### 结果窗口

- 原文区域。
- 译文区域。
- 复制原文、复制译文。
- 重新翻译。
- 暂停/继续实时识别。
- 可置顶、拖动、缩放。
- 不实现覆盖原区域和历史记录。

### 建议快捷键

- `Alt + Shift + A`：单次框选翻译。
- `Alt + Shift + R`：选择并启动实时区域翻译。
- `Alt + Shift + S`：停止实时翻译。
- `Esc`：取消选区。

快捷键冲突时必须允许用户修改。

## 10. 项目目录建议

```text
src/
├── components/
├── windows/
├── stores/
├── services/
├── types/
└── main.tsx

src-tauri/src/
├── app/
│   ├── commands.rs
│   ├── events.rs
│   └── state.rs
├── capture/
│   ├── mod.rs
│   ├── windows_graphics_capture.rs
│   └── coordinates.rs
├── ocr/
│   ├── mod.rs
│   ├── provider.rs
│   ├── paddle_onnx.rs
│   ├── preprocess.rs
│   └── postprocess.rs
├── translation/
│   ├── mod.rs
│   ├── provider.rs
│   ├── api.rs
│   └── local_onnx.rs
├── pipeline/
│   ├── mod.rs
│   ├── live.rs
│   ├── single.rs
│   └── dedup.rs
├── config/
├── models/
├── security/
├── error.rs
├── lib.rs
└── main.rs

src-tauri/resources/models/
├── manifest.json
├── ocr/
└── translation/
```

## 11. 配置示例

```json
{
  "capture": {
    "interval_ms": 500,
    "difference_threshold": 0.03
  },
  "ocr": {
    "language": "auto",
    "minimum_confidence": 0.55
  },
  "translation": {
    "provider": "api",
    "source_language": "auto",
    "target_language": "zh-CN",
    "timeout_seconds": 30
  },
  "result_window": {
    "always_on_top": true
  }
}
```

## 12. 非功能要求

- 空闲状态 CPU 占用接近 0。
- 实时模式避免无限队列和内存增长。
- 单次截屏到 OCR 结果目标小于 1 秒，具体取决于硬件和区域大小。
- OCR/翻译线程不得阻塞 Tauri UI 线程。
- 崩溃后不保留屏幕图像。
- 默认不保存截图、OCR 文本和译文。
- 日志不得记录 API Key 和完整敏感文本。
- 所有外部请求明确显示联网状态。
- Release 构建关闭不必要的 Tauri capability。

## 13. 开发阶段

### Phase 1：应用骨架

- 初始化 Tauri 2 + React + TypeScript。
- 创建主窗口、选区窗口、结果窗口。
- 建立 Commands、Events、AppState。
- 完成设置读写和日志。

### Phase 2：单次截屏 OCR

- 实现多显示器选区。
- 实现 Windows 屏幕捕获和区域裁剪。
- 接入英文、日文 ONNX OCR。
- 在结果窗口展示 OCR 原文。

### Phase 3：API 翻译

- 完成 TranslationProvider。
- 实现通用 API Provider。
- 完成中/日/英语言校验和结果显示。
- 增加超时、取消、错误提示。

### Phase 4：实时区域翻译

- 建立持续捕获会话。
- 实现帧差检测、限流、去重和只保留最新任务。
- 完成暂停、恢复、停止和区域重选。

### Phase 5：本地翻译

- 接入 ONNX 本地翻译模型。
- 实现模型清单和模型生命周期管理。
- 增加模型加载状态、内存不足和不兼容提示。

### Phase 6：稳定性与发布

- DPI、多显示器、HDR、锁屏和显示器热插拔测试。
- 性能分析和内存泄漏检查。
- 生成 Windows 安装包。
- 完成第三方许可证清单。

## 14. MVP 验收标准

- Windows 10/11 可安装并启动。
- 可在任意显示器框选区域。
- 可识别清晰的日文和英文屏幕文字。
- 可完成中、日、英任意目标语言翻译。
- 实时模式只在画面或文本变化时触发翻译。
- 连续运行 30 分钟无明显内存增长、崩溃或任务堆积。
- 停止按钮能在短时间内终止捕获、OCR 和翻译。
- API Key 不出现在配置文件和日志中。
- 默认不保存截图及翻译内容。
- API 与本地翻译可通过配置切换，业务流水线无需修改。

## 15. 测试要求

### 单元测试

- DPI 和多显示器坐标转换。
- 图像裁剪边界。
- 文本指纹与重复检测。
- OCR 行排序和文本合并。
- 语言组合校验。
- 配置迁移和默认值。
- API 错误映射。

### 集成测试

- 选区 → 截图 → OCR → 翻译 → 展示。
- 快速连续框选时旧任务被取消。
- 实时区域静止时不重复调用 OCR/翻译。
- API 超时、断网和限流。
- 模型缺失、损坏和加载失败。
- 显示器断开后的恢复。

### 测试素材

建立固定测试集，至少包括：

- 日文横排、竖排、游戏字幕、漫画对话框。
- 英文网页、代码编辑器、小字号字幕。
- 高 DPI、深色背景、低对比度、半透明文字。
- 中日英混合文本。

## 16. Agent 执行规则

1. 先完成最小垂直链路，不同时开发所有模块。
2. 每个 Phase 提交可运行版本和验收说明。
3. 所有平台相关代码限制在 `capture` 或 `windows` 模块。
4. OCR 和翻译必须通过 trait 使用，禁止在 UI command 中直接调用模型。
5. 长任务必须可取消，队列必须有界。
6. 不在前端保存 API Key、模型原始输出或截图。
7. 新增依赖前检查许可证、维护状态和 Release 构建体积。
8. 模型文件不直接提交 Git；使用 manifest、下载脚本或 Git LFS。
9. 未明确的模型格式、Tokenizer 或输出张量必须先建立独立验证程序，再接入主应用。
10. 优先保证正确性和稳定性，再优化推理速度。

## 17. 首批开发任务

- [ ] 创建 Tauri 2 项目和三窗口结构。
- [ ] 定义核心类型、Provider trait 和错误类型。
- [ ] 实现全局快捷键与选区交互。
- [ ] 验证 Windows Graphics Capture 的单帧捕获。
- [ ] 完成 DPI/多显示器坐标转换测试。
- [ ] 建立独立 OCR 模型验证 CLI。
- [ ] 验证英文与日文模型的预处理、字典和解码。
- [ ] 将 OCR CLI 代码迁入 `ocr` 模块。
- [ ] 实现单次截图翻译完整链路。
- [ ] 实现实时捕获、去重和取消机制。
- [ ] 接入 API TranslationProvider。
- [ ] 建立本地翻译模型验证 CLI。
- [ ] 接入 Local TranslationProvider。
- [ ] 完成安装包、日志和许可证清单。

## 18. 需要尽早验证的风险

- PaddleOCR 模型转换为 ONNX 后的算子兼容性。
- 日文竖排文本的检测、排序和识别质量。
- ONNX Tokenizer 与 Rust 实现的一致性。
- 本地翻译模型的体积、内存占用和 CPU 延迟。
- Windows 多显示器不同缩放比例下的选区坐标。
- HDR 屏幕捕获后的颜色和对比度。
- Tauri 透明全屏窗口的焦点、快捷键和多屏行为。

在上述风险未验证前，不要过早制作复杂 UI。
