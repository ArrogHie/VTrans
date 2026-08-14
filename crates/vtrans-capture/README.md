# vtrans-capture

## 模块概述

`vtrans-capture` 负责 Windows 屏幕采集：枚举显示器与 DPI 信息，把用户选中的屏幕区域截取为 `CapturedImage`（BGRA8），并提供单次截图和持续捕获会话。

边界：
- 负责：显示器枚举、DPI 与多显示器坐标换算、按物理像素裁剪、会话生命周期与资源释放。
- 不负责：OCR、翻译、帧差检测和流水线编排（属于 `vtrans-pipeline`）。
- 不负责：配置读取（`vtrans-config`）与凭据管理（`vtrans-security`）。
- 不跨 IPC 传输图像：`CapturedImage` 不实现 `Serialize`，图像只保留在 Rust 侧。
- 仅支持 Windows；非 Windows 目标无法编译，其他平台由应用层降级处理。

## 依赖关系

| 方向 | 模块 | 关系 |
|------|------|------|
| 上游 | `vtrans-core` | 使用 `CaptureSource` / `CaptureSession` trait、`ScreenRegion`、`CapturedImage`、`CaptureError`；`ScreenRegion` 的 serde 字段为 `monitor_id` / `x` / `y` / `width` / `height`，坐标是物理像素且相对显示器左上角 |
| 外部 | `async-trait` | 生成 `Send` 兼容的异步 trait 实现 |
| 外部 | `tokio` | 提供异步超时与轮询等待 |
| 外部 | `tracing` | 结构化日志，记录错误路径与生命周期事件 |
| 外部 | `windows` | Win32 显示器枚举、Direct3D 11、WinRT Graphics Capture API |
| 下游 | `vtrans-pipeline` | 通过 `CaptureSource` / `CaptureSession` trait 获取图像帧 |
| 下游 | `vtrans-app` | 创建并持有 `WindowsCaptureSource`，向流水线和前端提供显示器信息 |

## 快速上手

下面的最小示例完整保存在 `examples/capture_demo.rs`，可在真实 Windows 桌面会话中直接运行。核心流程：创建采集源 -> 选择区域 -> 单次截图 -> 持续会话 -> 显式停止。

```rust
use std::time::Duration;

use vtrans_capture::WindowsCaptureSource;
use vtrans_core::traits::CaptureSource;
use vtrans_core::types::ScreenRegion;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 创建采集源：枚举显示器并初始化 D3D11 / WinRT。
    let source = WindowsCaptureSource::new()?;

    // 2. 选择主显示器左上角 640x480 区域（物理像素，相对显示器）。
    let primary = source
        .list_monitors()
        .into_iter()
        .find(|m| m.is_primary)
        .ok_or("no primary monitor")?;
    let region = ScreenRegion::new(primary.id, 0, 0, 640, 480);

    // 3. 单次截图；OutOfBounds 表示区域需要修正，其他错误可重试或重建 source。
    let image = match source.capture_once(&region).await {
        Ok(image) => image,
        Err(vtrans_core::CaptureError::OutOfBounds { .. }) => {
            println!("region out of bounds, adjust and retry");
            return Ok(());
        }
        Err(err) => return Err(err.into()),
    };
    println!("single capture: {}x{}", image.width, image.height);

    // 4. 持续会话由调用方持有；stop 或 drop 会关闭 Graphics Capture 资源。
    let mut session = source.start_session(&region).await?;
    let frame = tokio::time::timeout(Duration::from_secs(5), session.next_frame())
        .await
        .map_err(|_| "timed out waiting for first frame")??;
    let frame = frame.ok_or("capture session ended before first frame")?;
    println!("session frame: {}x{}", frame.width, frame.height);

    // 5. 显式停止；停止后 next_frame 返回 CaptureError::SessionStopped。
    session.stop().await?;
    Ok(())
}
```

生命周期要点：`source` 由调用方创建并持有，内部持有 D3D11 设备；`session` 由 `start_session` 返回，同样由调用方持有，调用 `stop` 后必须重建；`CapturedImage` 是拥有独立 `Vec<u8>` 的数据，可克隆或移交给 OCR。

## 公开 API 概要

| API | 用途 |
|-----|------|
| `WindowsCaptureSource::new()` | 枚举显示器并初始化 D3D11 / WinRT，返回 `Result<Self, CaptureError>` |
| `WindowsCaptureSource::list_monitors()` | 返回所有显示器信息快照 |
| `CaptureSource::capture_once(&self, &ScreenRegion)` | 单次截图并裁剪为目标区域 |
| `CaptureSource::start_session(&self, &ScreenRegion)` | 创建持续会话，返回 `Box<dyn CaptureSession>` |
| `CaptureSession::next_frame(&mut self)` | 取下一帧；无新帧且缓存未过期时复用最近帧 |
| `CaptureSession::stop(&mut self)` | 幂等停止会话并释放资源 |
| `coordinates::logical_to_physical(x, scale)` | DIP 转物理像素，四舍五入 |
| `coordinates::physical_to_logical(x, scale)` | 物理像素转 DIP |
| `coordinates::region_to_physical(&ScreenRegion, scale)` | 缩放区域的位置与尺寸 |
| `coordinates::is_region_in_bounds(&ScreenRegion, w, h)` | 区域是否完全在显示器内 |
| `coordinates::clip_region_to_bounds(&ScreenRegion, w, h)` | 将区域裁剪到显示器边界 |
| `coordinates::to_monitor_relative(&ScreenRegion, mx, my)` | 虚拟桌面坐标转显示器相对坐标 |

`MonitorInfo` 字段：

```rust
pub struct MonitorInfo {
    pub id: String,          // 设备名（如 r"\\.\DISPLAY1"），是 capture 入口的显示器匹配键
    pub name: String,        // 人类可读名称
    pub width: u32,          // 物理像素宽度
    pub height: u32,         // 物理像素高度
    pub x: i32,              // 虚拟桌面 X，副屏可能为负
    pub y: i32,              // 虚拟桌面 Y，副屏可能为负
    pub scale_factor: f32,   // DPI / 96，1.0 = 100%，1.5 = 150%
    pub is_primary: bool,    // 是否主显示器
}
```

serde 表示：`ScreenRegion` 可序列化为 JSON 且字段名与结构体一致；`CapturedImage` 故意不实现 `Serialize`；`MonitorInfo` 当前不派生 serde，跨 IPC 时由应用层自行转换。

错误类型从 `vtrans-core` 导入：`MonitorNotFound`、`InitFailed`、`OutOfBounds`、`FrameGrabFailed`、`SessionStopped`、`DpiAwarenessFailed`。完整语义见模块规格。

## 行为契约

- 错误语义：`new` 失败通常是环境问题（无显示器、D3D11 不可用），不可简单重试，应报告用户；`OutOfBounds` 是输入问题，修正区域后可重试；`FrameGrabFailed` 多为瞬时故障，可重试；`SessionStopped` 表示会话已结束，需要重新 `start_session`。
- 并发模型：`WindowsCaptureSource` 是 `Send + Sync`，多线程并发调用安全（内部 D3D11 context 由 `Mutex` 串行化）；会话对象不是 `Sync`，`next_frame` 需要 `&mut self`，同一会话必须串行调用。
- 取消语义：capture trait 不接收 `CancellationToken`；调用方可用 `tokio::time::timeout` 包裹调用实现超时，超时后会话仍可继续使用。
- 资源生命周期：`source` drop 时释放 D3D11 引用；`session` 由调用方负责 `stop`，未显式 stop 时 drop 也会关闭帧池与会话（`FrameGrabber` 实现 `Drop`）；停止后如需继续采集必须重建会话。
- 边界条件：零宽高或越界区域返回 `OutOfBounds`；区域坐标为负视为越界；静态屏幕下会话最多复用最近帧 30 秒，之后重新等待新帧；首次 5 秒无帧返回 `Ok(None)` 表示会话结束。

## 集成注意事项

- `CapturedImage` 不实现 `Serialize`，不能通过 Tauri IPC 传 JSON 或 Base64。正确做法：图像留在 Rust 侧，IPC 只传文本、状态和缩略图。
- `ScreenRegion` 坐标是物理像素且相对显示器左上角，不是虚拟桌面坐标，也不是 DIP。正确做法：前端坐标先经 `to_monitor_relative` / `region_to_physical` 转换，再传给 capture。
- 输出固定为 BGRA8。正确做法：OCR 或图像处理侧按 BGRA 布局读取，不要假设 RGB。
- `WindowsCaptureSource::new()` 在无交互桌面或远程会话可能失败。正确做法：由应用层捕获 `InitFailed` 并展示错误，不要在 UI 主线程反复构造。
- `next_frame` 在静态屏幕上会返回相同缓存帧。正确做法：消费方自己做帧差或指纹去重，避免重复 OCR（去重逻辑在 `vtrans-text` 或 `vtrans-pipeline`）。

## 设计决策记录

| 决策 | 理由 | 备选方案 |
|------|------|----------|
| `CaptureError` 从 `vtrans-core` 导入 | trait 签名引用该类型，跨 crate 保持一致 | 各 crate 自建错误（trait 无法编译） |
| D3D11 immediate context 用 `Arc<Mutex<...>>` 共享 | `ID3D11DeviceContext` 非线程安全，而 `CaptureSource` 要求 `Send + Sync` | 每个会话独立创建设备（浪费且资源受限） |
| 会话无新帧时复用最近帧，上限 30 秒 | Graphics Capture 只在新画面时产生帧，实时流程需要连续帧 | 无帧即 `Ok(None)`（实时流程误停）；无限复用（断流时持续陈旧画面） |
| `RoInitialize` 推迟到捕获线程 | 构造函数可能运行在已初始化的 STA 线程，过早调用会失败 | 构造时强制初始化 MTA（对宿主线程侵入） |
| D3D11 硬件失败后回退 WARP | 无 GPU、远程桌面等环境仍可采集 | 只使用硬件驱动（环境兼容性差） |

## 已知限制

| 类型 | 限制 | 缓解方式 |
|------|------|----------|
| 待后续 Phase | 显示器热插拔监听与自动重建会话 | 当前无帧 5 秒后返回 `Ok(None)`，由消费方检测并重建 |
| 待后续 Phase | HDR 精确颜色转换 | 帧池固定请求 `B8G8R8A8UIntNormalized`，由系统转换为 8-bit SDR |
| 待后续 Phase | `MonitorInfo` serde 支持 | 应用层自行转换后再通过 IPC 发送 |
| 设计使然 | 仅支持 Windows | 平台代码全部限制在本 crate，其他平台由应用层降级 |
| 设计使然 | WGC 显示器级捕获包含桌面合成的一切窗口（含 VTrans 自身），无逐窗口排除 API | 系统级用 `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` 标记自身窗口；实测（2026-08-14，Windows 11）该标记被本 crate 的显示器级捕获尊重，窗口从捕获帧中消失并露出背景，复测方式见 `examples/wda_probe.rs` |
| 平台相关 | `WDA_EXCLUDEFROMCAPTURE` 依赖系统版本（Win10 2004+），不同版本行为有矛盾记载 | 上线前或换机后运行 `cargo run -p vtrans-capture --example wda_probe` 实机复核，勿凭文档臆断 |
| 设计使然 | 构造时枚举显示器一次 | 热插拔后重新创建 `WindowsCaptureSource` |
| 设计使然 | 静态屏幕复用缓存帧 | 30 秒上限并配合消费方去重 |
| 设计使然 | 全帧复制到 CPU 后再裁剪 | 超大显示器开销较高，后续可评估 GPU 裁剪 |

## 构建与测试

```powershell
cargo check -p vtrans-capture
cargo test -p vtrans-capture
cargo clippy -p vtrans-capture --all-targets -- -D warnings
cargo fmt -p vtrans-capture -- --check
cargo run -p vtrans-capture --example capture_demo
cargo run -p vtrans-capture --example wda_probe
```

`tests/capture_integration.rs` 会真实枚举显示器、截取小区域并验证会话停止语义，需要交互式 Windows 桌面会话；纯逻辑测试在无桌面环境也能通过。

## 详细规格

参见 `docs/modules/04-capture.md`。
