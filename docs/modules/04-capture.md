# 模块 04：vtrans-capture 屏幕采集

| 属性 | 值 |
|------|-----|
| Crate | `vtrans-capture` |
| 分支 | `feat/04-capture` |
| 上游依赖 | `vtrans-core` |
| 层级 | 2 |
| 复杂度 | 高 |
| 阶段 | Phase 2 |

## 职责

获取显示器和窗口信息，实现单次截图和持续捕获会话，根据物理像素坐标裁剪用户选择的区域，处理多显示器、缩放比例和负坐标，输出统一的 RGBA/BGRA 图像帧。

## 公开 API

实现 `vtrans_core::CaptureSource` 和 `CaptureSession` trait。具体实现类型不公开，通过 `new()` 返回 trait 对象。

```rust
/// Windows 屏幕采集器
pub struct WindowsCaptureSource { /* ... */ }

impl WindowsCaptureSource {
    pub fn new() -> Result<Self, CaptureError>;

    /// 获取所有显示器信息
    pub fn list_monitors(&self) -> Vec<MonitorInfo>;
}

pub struct MonitorInfo {
    pub id: String,
    pub name: String,
    pub width: u32,       // 物理像素
    pub height: u32,
    pub x: i32,           // 虚拟桌面坐标（可能为负）
    pub y: i32,
    pub scale_factor: f32, // DPI / 96
    pub is_primary: bool,
}

/// 将逻辑坐标（DIP）转换为物理像素坐标
pub fn logical_to_physical(x: f32, scale: f32) -> i32;
```

## 错误类型

```rust
[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("monitor not found: {0}")]
    MonitorNotFound(String),
    #[error("graphics capture init failed: {0}")]
    InitFailed(String),
    #[error("region out of bounds: {region:?}")]
    OutOfBounds { region: ScreenRegion },
    #[error("frame grab failed: {0}")]
    FrameGrabFailed(String),
    #[error("session stopped")]
    SessionStopped,
    #[error("dpi awareness failed: {0}")]
    DpiAwarenessFailed(String),
}
```

## 内部文件结构

```text
crates/vtrans-capture/
├── Cargo.toml
├── README.md
├── src/
│   ├── lib.rs              # re-export
│   ├── source.rs           # WindowsCaptureSource (CaptureSource impl)
│   ├── session.rs          # 捕获会话 (CaptureSession impl)
│   ├── monitor.rs          # MonitorInfo, 显示器枚举
│   ├── coordinates.rs      # DPI/多显示器坐标转换
│   └── graphics_capture.rs # Windows Graphics Capture API 封装
└── tests/
    ├── coordinates_test.rs
    └── fixtures/
```

## 测试计划

| 测试项 | 类型 | 说明 |
|--------|------|------|
| logical_to_physical 转换 | 单元 | 150% DPI 下 100 DIP = 150 px |
| 负坐标处理 | 单元 | 副显示器 x=-1920 正确识别 |
| 多显示器区域裁剪 | 单元 | 区域跨越显示器边界 |
| MonitorInfo 解析 | 集成 | list_monitors 返回正确信息 |
| 单次截图 | 集成 | capture_once 返回非空图像 |
| 持续会话帧 | 集成 | next_frame 返回多帧 |
| 会话停止 | 集成 | stop 后 next_frame 返回 None |
| 区域越界 | 单元 | 返回 OutOfBounds |

## 验收标准

- [ ] 可列出所有显示器及 DPI 信息
- [ ] 单次截图返回正确裁剪的 RGBA 图像
- [ ] 持续会话可获取连续帧
- [ ] DPI 坐标转换正确
- [ ] 平台相关代码限制在本 crate 内
- [ ] clippy 零警告，unsafe 有 SAFETY 注释
- [ ] README.md 完整

## 开发注意事项

- 使用 Windows Graphics Capture API（GraphicsCaptureItem + Direct3D11CaptureFramePool）
- 应用启用 Per-Monitor DPI Awareness V2
- 固定区域实时模式复用捕获会话，不每次重建
- 选区坐标统一转换为物理像素后再裁剪
- HDR 屏幕需正确转换为 8-bit SDR
- 窗口最小化、锁屏、显示器断开时暂停或安全重建会话
- 所有 unsafe 代码块必须有 // SAFETY: 注释
