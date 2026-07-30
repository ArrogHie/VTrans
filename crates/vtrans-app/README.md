# vtrans-app

应用层模块。定义 Tauri Commands/Events、管理 AppState、注册全局快捷键。

## 职责

- AppState：组装所有模块的具体实现并注入 Pipeline
- Tauri Commands：前端调用的入口
- Events：后端到前端的事件推送
- 全局快捷键注册

## 依赖

全部 vtrans-* crate

## 构建

```powershell
cargo build -p vtrans-app
cargo test -p vtrans-app
```

## 详细规格

参见 docs/modules/10-app.md
