# vtrans-capture

屏幕采集模块。使用 Windows Graphics Capture 实现单次截图和持续捕获。

## 职责

- WindowsCaptureSource：实现 CaptureSource trait
- 多显示器信息、DPI 坐标转换
- 区域裁剪、持续会话管理

## 依赖

vtrans-core

## 构建

```powershell
cargo build -p vtrans-capture
cargo test -p vtrans-capture
```

## 详细规格

参见 docs/modules/04-capture.md
