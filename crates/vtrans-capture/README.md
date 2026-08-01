# vtrans-capture

`vtrans-capture` 是 `VTrans` 的屏幕采集模块，负责枚举显示器、处理 DPI
与多显示器坐标，并通过 Windows Graphics Capture 提供单次截图和持续捕获会话。

## 职责

- `WindowsCaptureSource` 实现 `vtrans_core::CaptureSource` trait。
- `WindowsCaptureSession` 实现 `vtrans_core::CaptureSession` trait。
- 使用 Win32 `EnumDisplayMonitors` / `GetMonitorInfoW` 枚举物理显示器。
- 使用 `GetDpiForMonitor` 获取每台显示器的 DPI 缩放因子。
- 启动进程时请求 Per-Monitor DPI Awareness V2。
- 将 Direct3D 11 捕获帧复制到 CPU 内存，输出 `CapturedImage`（BGRA8）。
- 会话在屏幕无新帧时复用上一帧，避免实时流程误判会话结束。

## 依赖

### 上游 crate

- `vtrans-core`：`CaptureSource` / `CaptureSession` trait、`ScreenRegion`、
  `CapturedImage`、`CaptureError` 等共享类型。

### 外部 crate

- `async-trait`：异步 trait 实现。
- `tokio`：异步轮询与超时。
- `tracing`：结构化日志。
- `windows`：Win32、Direct3D 11、WinRT Graphics Capture API。

## 公开 API

```rust
pub struct WindowsCaptureSource { /* ... */ }

impl WindowsCaptureSource {
    pub fn new() -> Result<Self, CaptureError>;
    pub fn list_monitors(&self) -> Vec<MonitorInfo>;
}

pub struct MonitorInfo {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub scale_factor: f32,
    pub is_primary: bool,
}

// vtrans_capture::coordinates
pub fn logical_to_physical(x: f32, scale: f32) -> i32;
pub fn physical_to_logical(x: f32, scale: f32) -> f32;
pub fn region_to_physical(region: &ScreenRegion, scale: f32) -> ScreenRegion;
pub fn is_region_in_bounds(region: &ScreenRegion, monitor_width: u32, monitor_height: u32) -> bool;
pub fn clip_region_to_bounds(region: &ScreenRegion, monitor_width: u32, monitor_height: u32) -> ScreenRegion;
pub fn to_monitor_relative(region: &ScreenRegion, monitor_x: i32, monitor_y: i32) -> ScreenRegion;
```

`WindowsCaptureSource` 通过 `CaptureSource::capture_once` 和
`CaptureSource::start_session` 暴露具体采集能力；持续会话通过
`CaptureSession::next_frame` 和 `CaptureSession::stop` 管理。

## 构建与测试

```powershell
cargo build -p vtrans-capture
cargo test -p vtrans-capture
cargo clippy -p vtrans-capture --all-targets -- -D warnings
cargo fmt --all -- --check
```

集成测试位于 `tests/capture_integration.rs`，需要交互式 Windows
桌面会话；它会实际枚举显示器、截取小区域并验证会话停止语义。

## 已知限制

- 只支持 Windows；非 Windows 目标不会编译。
- `list_monitors` 在构造时枚举一次；显示器热插拔后需要重新创建
  `WindowsCaptureSource`。
- 捕获帧固定输出 BGRA8；HDR 内容由 Graphics Capture 的
  `B8G8R8A8UIntNormalized` 帧池转换为 SDR 表示。
- 会话在首次取帧前若 5 秒无帧会以 `Ok(None)` 结束；取到首帧后，
  屏幕无变化时复用最后一帧。

## 详细规格

参见 `docs/modules/04-capture.md` 和 `docs/ARCHITECTURE.md`。
