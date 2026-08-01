# VTrans README 写作提示词

> 本文档是交给模块开发 Agent 的 README 写作指令。使用时将 `{{...}}` 占位符替换为目标模块的具体值。
> 与 `docs/AGENT_DEV_PROMPT.md`、`docs/AGENT_REVIEW_PROMPT.md` 配套使用。

---

## 你的任务

你正在实现模块 `{{MODULE_NUMBER}}`（`{{MODULE_NAME}}`，分支 `{{BRANCH_NAME}}`）。完成代码和测试后，为 `{{CRATE_PATH}}/README.md` 编写或重写完整文档。

这份 README 有两个目标读者，**两类读者都必须覆盖**：

1. **消费方 Agent**（其他模块的开发 Agent，例如 pipeline 的开发者要使用 OCR 和 translation）——他们在不阅读源码的情况下，必须能知道：怎么实例化、怎么调用、生命周期如何管理、有什么前置条件、会怎么失败。
2. **负责人 Agent**（未来接手或维护本模块的 Agent，以及审查 Agent）——他们必须能从 README 中知道：模块边界、设计决策、行为约定、已知限制、扩展方式。

## 必读上下文

动笔前先阅读：

1. `docs/modules/{{MODULE_NUMBER}}-{{MODULE_SLUG}}.md` — 本模块规格（公开 API、错误类型、验收标准）
2. `crates/vtrans-core/src/` — 上游共享类型和 trait
3. 你的上游依赖 crate 的 `README.md` — 了解消费方视角的写法（作为参考，不是模板）
4. `docs/ARCHITECTURE.md` 第 5 节 — 横切标准（日志、错误、测试、代码风格）

## README 结构

以下每个章节都是必需的，按顺序书写。每章标注了主要读者。

### 1. 模块概述（两类读者）

一句话说明模块职责，紧接着用 3-5 行说明**边界**：本模块做什么、明确不做什么。

示例（vtrans-config 风格）：
> 管理应用配置的 schema、持久化、迁移和校验。
> 边界：不管理 API Key（属于 vtrans-security）；不支持热重载（由应用层负责）。

**禁止**：从源码复制粘贴 crate 顶部注释。概述必须是给陌生人看的。

### 2. 依赖关系（消费方 / 负责人）

- 上游 crate：列出并说明**本模块使用了它们的哪些核心概念**（例如"使用 vtrans-core 的 `Language`，依赖其 serde 表示 `auto`/`zh-CN`/`ja`/`en`"）。
- 外部 crate：列出主要依赖，一句话说明用途。
- 下游消费方：列出哪些模块会依赖本模块（查询 `docs/ARCHITECTURE.md` 的依赖表），并说明**这些消费方需要本模块提供什么**。

### 3. 快速上手（消费方，最重要章节）

提供 **3-5 步可编译的最小使用示例**。必须满足：

- 使用**真实 API**，不是伪代码，不省略类型。
- 包含实例化、关键方法调用、错误处理三个环节。
- 展示所有权和生命周期：谁创建、谁持有、何时 drop。
- 如果 API 是异步的，展示 `tokio` 运行方式；如果涉及 `CancellationToken`，展示取消示例。
- 示例代码放在 fenced code block，标注 `rust`。

示例（vtrans-config 风格）：

```rust
use vtrans_config::ConfigManager;

fn main() -> Result<(), vtrans_config::ConfigError> {
    let config_dir = std::env::temp_dir().join("vtrans-demo");
    let manager = ConfigManager::new(&config_dir)?;

    // 首次加载自动创建默认配置并写入文件
    let config = manager.load()?;
    println!("target: {}", config.translation.target_language);

    // 更新配置：闭包内修改，自动保存并校验
    manager.update(|c| c.capture.interval_ms = 1000)?;
    Ok(())
}
```

**禁止**：示例中只展示类型签名而不展示调用。如果模块没有公开入口（例如只实现 trait 的模块），示例必须展示**如何通过 trait 使用**，包括 mock 或真实实现的最小场景。

### 4. 公开 API 概要（消费方）

- 用表格列出所有公开类型、trait、函数，每行一句话说明用途。
- 对每个核心类型，给出**字段/方法签名**，并用 `///` 风格注释补充语义。
- 说明 serde 表示（如果该类型会跨 JSON/IPC 边界）。
- **不要**把 `docs/modules/NN-*.md` 的全部签名复制过来。README 保持"够用"，完整规格引用到模块文档。

### 5. 行为契约（消费方，必须显式声明）

列出调用方必须知道的**非显而易见的约定**，每条约 1-2 行。至少覆盖：

- **错误语义**：每个公开入口可能的失败方式；哪些错误可重试，哪些不可。
- **并发模型**：`Send`/`Sync` 约束；内部锁；多线程调用是否安全。
- **取消语义**：`CancellationToken` 在哪个阶段生效，取消后返回什么错误。
- **资源生命周期**：谁负责关闭（session、file handle、model 等）；drop 时发生什么。
- **边界条件**：空输入、超大输入、零尺寸 region 等行为。

### 6. 集成注意事项（消费方）

明确写出**消费方在集成时容易踩的坑**，例如：

- "`ConfigManager::update` 要求配置文件已存在，必须先调用 `load`"
- "`CapturedImage` 不实现 `Serialize`，图像不能通过 Tauri IPC 传输"
- "`init_logging` 只能调用一次，重复调用返回 `Err`"
- "模型加载是重操作，必须在后台线程执行，不能阻塞 UI"

每条坑必须给出一行"正确做法"。

### 7. 设计决策记录（负责人）

用「决策 / 理由 / 备选方案」三段式记录本模块实现中的重要决策，**每项 2-4 行**。例如：

| 决策 | 理由 | 备选方案 |
|------|------|----------|
| 日志按小时轮转 | tracing-appender 不支持按大小轮转 | 引入第三方 rolling 库（评估体积后放弃） |
| `AppConfig::update` 需要先 load | 保证读-改-写不丢更新 | 无锁直接读写（并发不安全） |
| 错误变体从 vtrans-core 导入 | trait 签名引用它们，跨 crate 保持一致 | 各 crate 自建错误（会导致 trait 无法编译） |

只记录**有意义的决策**，不要记录平凡选择（如"用 Vec 不用 HashMap"）。

### 8. 已知限制（负责人 / 审查）

- 明确列出未实现但规格中提到的功能，标注「待后续 Phase」。
- 明确列出已知的性能、正确性或兼容性限制。
- 每项限制标注缓解方式或规避方法。

### 9. 构建与测试（两类读者）

给出完整的命令块：

```powershell
cargo check -p {{MODULE_NAME}}
cargo test -p {{MODULE_NAME}}
cargo clippy -p {{MODULE_NAME}} --all-targets
cargo fmt -p {{MODULE_NAME}} -- --check
```

以及（如适用）验证 CLI 的运行示例。

### 10. 详细规格引用（两类读者）

末尾用一行指向模块规格文档：

```markdown
## 详细规格
参见 `docs/modules/{{MODULE_NUMBER}}-{{MODULE_SLUG}}.md`。
```

## 写作标准

1. **不阅读源码能使用**：把 README 给一个只看 README 的同事，他能写出可运行的最小代码。这是验收标准。
2. **代码示例必须真实**：示例代码必须能通过 `cargo test` 的 doctest（或至少是编译通过的代码片段）。写完后运行 `cargo test -p {{MODULE_NAME}} --doc` 验证。
3. **中英文一致**：全文件用中文叙述，标识符、命令、路径用英文原样。不要中英混杂。
4. **不重复规格**：完整 API 规格、测试计划引用到 `docs/modules/`，README 不复制整段。
5. **表格优先**：依赖、API、决策、限制使用 Markdown 表格，段落只用于概述和契约解释。
6. **篇幅控制**：100-200 行。超过 250 行说明你在重复规格文档。

## 禁止事项

1. 禁止从 `lib.rs` 粘贴 `pub use` 列表作为 API 章节。
2. 禁止只写签名不写示例。
3. 禁止写"详见代码"这种空洞引用。
4. 禁止在 README 中出现绝对路径（如 `C:\...`、`/home/...`），一律用相对路径或 `$HOME` 占位。
5. 禁止包含凭据、密钥、内部 URL 或敏感信息。
6. 禁止把未实现的功能写成已实现。

## 验收检查（交付前自检）

- [ ] 一个只看 README 的 Agent 能写出调用本模块的最小代码
- [ ] 示例代码通过 doctest
- [ ] 行为契约覆盖：错误、并发、取消、资源、边界
- [ ] 集成注意事项包含至少 2 条"坑 + 正确做法"
- [ ] 设计决策记录至少 3 项，每项含理由和备选方案
- [ ] 已知限制区分"待实现"与"设计使然"
- [ ] 篇幅在 100-200 行
- [ ] 无绝对路径、无敏感信息、无未实现功能冒充

## 与审查的衔接

审查 Agent 会按 `docs/AGENT_REVIEW_PROMPT.md` 的 D13 检查项核对 README：是否包含职责、依赖、API 概要、构建命令、已知限制。你的 README 满足本提示词全部要求后，该项自然通过。

---

## 快速实例化

| 参数 | 模块 02 | 模块 03 | 模块 06 | 模块 08 |
|------|---------|---------|---------|---------|
| `{{MODULE_NUMBER}}` | 02 | 03 | 06 | 08 |
| `{{MODULE_NAME}}` | vtrans-config | vtrans-security | vtrans-text | vtrans-models |
| `{{MODULE_SLUG}}` | config | security | text | models |
| `{{BRANCH_NAME}}` | feat/02-config | feat/03-security | feat/06-text | feat/08-models |
| `{{CRATE_PATH}}` | crates/vtrans-config | crates/vtrans-security | crates/vtrans-text | crates/vtrans-models |

| 参数 | 模块 04 | 模块 05 | 模块 07 | 模块 09 | 模块 10 |
|------|---------|---------|---------|---------|---------|
| `{{MODULE_NUMBER}}` | 04 | 05 | 07 | 09 | 10 |
| `{{MODULE_NAME}}` | vtrans-capture | vtrans-ocr | vtrans-translation | vtrans-pipeline | vtrans-app |
| `{{MODULE_SLUG}}` | capture | ocr | translation | pipeline | app |
| `{{BRANCH_NAME}}` | feat/04-capture | feat/05-ocr | feat/07-translation | feat/09-pipeline | feat/10-app |
| `{{CRATE_PATH}}` | crates/vtrans-capture | crates/vtrans-ocr | crates/vtrans-translation | crates/vtrans-pipeline | crates/vtrans-app |
