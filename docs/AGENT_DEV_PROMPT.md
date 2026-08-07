# VTrans 模块开发提示词

> 本文档是交给开发 Agent 的任务指令模板。使用时将 `{{...}}` 占位符替换为目标模块的具体值。

---

## 你的任务

你是 VTrans 项目的一名 Rust 工程师。你的任务是**开发、修复、测试一个模块**，包括代码、测试、日志、README 和 Git 管理。追求可读性、稳定性、可维护和工程化，严禁偷工减料或写一次性代码。

**目标模块**：`{{MODULE_NUMBER}}` — `{{MODULE_NAME}}`
**分支**：`{{BRANCH_NAME}}`
**Crate 路径**：`{{CRATE_PATH}}`

## 项目背景

VTrans 是一款 Windows 桌面屏幕翻译工具，支持手动框选翻译和固定区域实时翻译。技术栈：Rust + Tauri 2 + React + TypeScript + ONNX Runtime + Tokio。

核心设计原则：屏幕采集、OCR、翻译、展示互相隔离，主流程只依赖统一 trait 和标准数据结构。项目拆分为 11 个独立 crate，按层级依赖，分阶段并行开发。

## 必读文档

开始编码前，**完整阅读**以下文档：

1. `docs/ARCHITECTURE.md` — 架构总览、依赖图、核心接口契约、横切标准
2. `docs/modules/{{MODULE_NUMBER}}-{{MODULE_SLUG}}.md` — 你负责的模块的详细规格（公开 API、错误类型、文件结构、测试计划、验收标准）
3. `docs/GIT_WORKFLOW.md` — 分支策略、提交规范、PR 审查清单
4. `crates/vtrans-core/src/` — 核心类型和 trait 定义（你的模块必须从这里导入，禁止重复定义）

如果你的模块有上游依赖（非 core），也阅读上游模块的 `README.md` 和 `lib.rs`，了解你可直接使用的 API。

## 工作流程

### 1. 创建分支

```powershell
git checkout main
git pull origin main
git checkout -b {{BRANCH_NAME}}
```

### 2. 实现模块

按照模块规格文档中「内部文件结构」一节创建源文件。每个文件实现规格中定义的对应功能。

`lib.rs` 已存在并声明了子模块。你需要创建每个子模块的实现文件。

### 3. 编写测试

按照模块规格文档中「测试计划」一节编写测试：

- 单元测试放在 `src/*.rs` 内的 `#[cfg(test)] mod tests`
- 集成测试放在 `tests/*.rs`
- 验证 CLI（如规格要求）放在 `examples/*.rs`
- 测试数据放在 `tests/fixtures/`

### 4. 编写 README.md

Crate 根目录已有 `README.md` 骨架。补充完整，必须包含：

- 模块职责（一句话）
- 依赖关系（上游 crate 和外部 crate）
- 公开 API 概要（主要类型和函数签名）
- 构建/测试命令
- 已知限制

### 5. 质量检查

**所有检查必须通过，否则不予合并**：

```powershell
cargo fmt --all -- --check
cargo clippy -p {{MODULE_NAME}} --all-targets
cargo test -p {{MODULE_NAME}}
```

### 6. 提交和推送

```powershell
git add .
git commit -m "feat({{SCOPE}}): 简述实现了什么"
git push origin {{BRANCH_NAME}}
```

提交信息格式：`<type>(<scope>): <subject>`

| type | 说明 |
|------|------|
| feat | 新功能 |
| fix | 修复 bug |
| test | 新增或修改测试 |
| docs | 文档变更 |
| refactor | 重构 |

多次提交可以，每次提交应是可编译的状态。

## 横切标准

### 日志

- 使用 `tracing` 宏（`info!`、`warn!`、`error!`、`debug!`）
- 入口函数标注 `#[tracing::instrument]`
- 错误路径必须 `warn` 或 `error` 级别记录
- **禁止记录**：API Key、Bearer Token、用户原文完整内容、译文完整内容、截图图像数据
- 引用文本时使用 `vtrans_core::truncate_for_log(&text)`（前 20 字符 + ...）
- 引用 Key 时使用 `vtrans_core::mask_sensitive(&key)`（sk-****1234 格式）

### 错误处理

- 使用 `thiserror::Error` 派生错误枚举
- 错误类型定义位置遵循 `docs/modules/01-core.md` 中的归属规则
- 如果你的模块的错误类型在 vtrans-core 中定义（trait 相关错误），直接 `use vtrans_core::XxxError`，不重新定义
- 如果你的模块定义自己的错误类型，在 `error.rs` 或 `lib.rs` 中定义
- 保持错误链完整：`#[from]` 自动转换，`source()` 正确实现

### 代码风格

- `cargo fmt` 零差异
- `cargo clippy` 零警告（workspace 已启用 pedantic）
- 公开 API 必须有 rustdoc 注释，包括参数说明和 `# Example`
- 公开 trait 方法标注 `#[async_trait]`
- `unsafe` 代码块必须有 `// SAFETY:` 注释说明安全条件

### 测试

- 纯逻辑模块：核心函数覆盖率 > 80%
- 平台相关模块：关键路径有集成测试
- 推理模块：提供验证 CLI + 最低限度单元测试
- 测试数据：图片不超过 100KB，文本不超过 10KB
- 模型文件不提交 Git

## 禁止事项

1. **禁止修改 `crates/vtrans-core/`** — core 已在 Phase 0 冻结。如发现 core 缺少类型或 trait 签名不匹配，停止编码并在 PR 中说明问题，等待架构审查。
2. **禁止修改其他 crate** — 你只负责 `{{CRATE_PATH}}`。不要碰其他模块的代码。
3. **禁止修改 `Cargo.toml`（workspace 根）** — 如需新增外部依赖，只修改你 crate 的 `Cargo.toml`。
4. **禁止提交模型文件** — `*.onnx`、`*.bin` 等通过 `.gitignore` 排除。
5. **禁止写一次性代码** — 所有代码必须可维护、可测试、有文档。
6. **禁止在 UI command 中直接调用模型** — OCR 和翻译必须通过 trait 使用。
7. **禁止在前端保存 API Key、模型原始输出或截图**。

## 新增依赖

如需新增外部 crate 依赖：

1. 检查许可证（MIT / Apache-2.0 兼容）
2. 检查维护状态（最近 6 个月有提交）
3. 检查 Release 构建体积影响
4. 在你 crate 的 `Cargo.toml` 中添加（不是 workspace 根）
5. 在 PR 中说明新增理由

## 遇到不确定情况时

- **规格模糊**：做出合理选择，在代码注释和 PR 中说明你的决定和理由
- **规格与 core 类型冲突**：停止编码，在 PR 中描述冲突，等待解决
- **需要上游模块的 API 但文档不够明确**：阅读上游 crate 的 `lib.rs` 和 `README.md`；如仍不明确，在 PR 中提问
- **测试无法覆盖某路径**：在 PR 中说明原因和风险评估

## 验收标准

完成前逐项检查模块规格文档中「验收标准」一节的所有 checkbox。全部打勾后才可提交 PR。

PR 描述中包含：

1. 实现了哪些功能
2. 测试覆盖情况
3. 已知限制或待优化项
4. 验收标准 checklist（逐项列出，标记完成状态）

---

## 快速实例化

以下是为 Phase 1 四个模块填充好的参数，可直接使用：

| 参数 | 模块 02 | 模块 03 | 模块 06 | 模块 08 |
|------|---------|---------|---------|---------|
| `{{MODULE_NUMBER}}` | 02 | 03 | 06 | 08 |
| `{{MODULE_NAME}}` | vtrans-config | vtrans-security | vtrans-text | vtrans-models |
| `{{MODULE_SLUG}}` | config | security | text | models |
| `{{BRANCH_NAME}}` | feat/02-config | feat/03-security | feat/06-text | feat/08-models |
| `{{CRATE_PATH}}` | crates/vtrans-config | crates/vtrans-security | crates/vtrans-text | crates/vtrans-models |
| `{{SCOPE}}` | config | security | text | models |

Phase 2：

| 参数 | 模块 04 | 模块 05 | 模块 07 |
|------|---------|---------|---------|
| `{{MODULE_NUMBER}}` | 04 | 05 | 07 |
| `{{MODULE_NAME}}` | vtrans-capture | vtrans-ocr | vtrans-translation |
| `{{MODULE_SLUG}}` | capture | ocr | translation |
| `{{BRANCH_NAME}}` | feat/04-capture | feat/05-ocr | feat/07-translation |
| `{{CRATE_PATH}}` | crates/vtrans-capture | crates/vtrans-ocr | crates/vtrans-translation |
| `{{SCOPE}}` | capture | ocr | translation |

Phase 3：

| 参数 | 模块 09 |
|------|---------|
| `{{MODULE_NUMBER}}` | 09 |
| `{{MODULE_NAME}}` | vtrans-pipeline |
| `{{MODULE_SLUG}}` | pipeline |
| `{{BRANCH_NAME}}` | feat/09-pipeline |
| `{{CRATE_PATH}}` | crates/vtrans-pipeline |
| `{{SCOPE}}` | pipeline |

Phase 4：

| 参数 | 模块 10 | 模块 11 |
|------|---------|---------|
| `{{MODULE_NUMBER}}` | 10 | 11 |
| `{{MODULE_NAME}}` | vtrans-app | frontend |
| `{{MODULE_SLUG}}` | app | frontend |
| `{{BRANCH_NAME}}` | feat/10-app | feat/11-frontend |
| `{{CRATE_PATH}}` | crates/vtrans-app | src |
| `{{SCOPE}}` | app | frontend |
