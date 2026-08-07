## 模块开发说明：07 vtrans-translation — 翻译模型升级 增量（主责）

### AGENT_DEV_PROMPT 参数

- MODULE_NUMBER: 07
- MODULE_NAME: vtrans-translation
- MODULE_SLUG: translation
- CRATE_PATH: crates/vtrans-translation（另含新增 `native/translation_bridge/` 与 `licenses/` 登记，属本任务产出）
- SCOPE: translation
- BRANCH_NAME: feat/07-new-translate-model

### 功能上下文

- 功能目标：本地翻译从「单 ONNX 模型」升级为「Bergamot en→zh + CTranslate2 INT8 ja→zh」双引擎原生实现（接入指南 §5/§6/§10/§11/§15/§21/§25）
- 决策状态（已确认 2026-08-07）：A2 本地运行时 id 定为 `"local-native"`；A3 彻底删除旧 ONNX 路径（`local_onnx.rs` 及其测试），不保留双维护
- 本模块承担的部分：
  1. `native/translation_bridge/`：C++17 统一 C ABI bridge（封装 Bergamot + CTranslate2 + SentencePiece），含 CMake 构建与 Windows 构建脚本
  2. Rust FFI 绑定 + `NativeTranslationProvider`（实现 `TranslationProvider`，`id` 见 A2）
  3. 删除旧 ONNX 单模型路径（A3：`local_onnx.rs` 及其测试；`ort` 依赖仍由 vtrans-ocr 使用，不得从 workspace 移除）
  4. 验证 CLI 更新、许可证登记（B6）
- 上游已提供：08 的 manifest v2（双引擎条目 + 路径解析辅助）；02 的 `translation.quality`（`"fast"|"balanced"`）

### 任务要求

- 范围：仅限本模块 + 新增 `native/translation_bridge/` + `licenses/`；禁止修改其他 crate；禁止修改 vtrans-core；禁止修改 workspace 根 Cargo.toml（07 的 Cargo.toml 可加 `libloading` 等自身依赖）
- 新增公开 API（约束性定义）：
  ```rust
  /// 本地多引擎翻译 Provider（Bergamot en-zh + CTranslate2 ja-zh）
  pub struct NativeTranslationProvider { /* ... */ }
  impl NativeTranslationProvider {
      /// 从 manifest v2 加载双引擎；阻塞重操作，须在 spawn_blocking 中调用
      pub fn from_manager(manager: &ModelManager) -> Result<Self, TranslationError>;
      /// 可选：显式指定质量档位（缺省读取 AppConfig.translation.quality）
      pub fn with_quality(self, quality: TranslationQuality) -> Result<Self, TranslationError>;
  }
  pub enum TranslationQuality { Fast, Balanced }  // serde: "fast" / "balanced"
  ```
  - `impl TranslationProvider for NativeTranslationProvider`：`id()` 见 A2（建议 `"local-native"`）；`supported_pairs()` 返回 `[(en, zh-CN), (ja, zh-CN)]`；`translate()` 按 `request.source` 路由（en → Bergamot、ja → CTranslate2；`Auto` 由上层解析后传入，Provider 拒绝 `Auto` 与不支持对 → `TranslationError::UnsupportedPair`）
- C ABI bridge（以 starter `native/translation_bridge.h` 为基准，可扩展但必须保持四个核心函数语义）：
  - `translation_create(enzh_dir, jazh_dir) -> TranslationEngine`（失败返回 NULL；应用生命周期内常驻一个引擎，指南 §15）
  - `translation_translate(engine, source_lang, utf8_input, &utf8_output) -> int`（`"en"|"ja"`；成功返回 0；错误码映射：1 invalid argument / 2 unsupported language / 3 model not loaded / 4 tokenizer / 5 inference / 6 encoding / 7 version mismatch）
  - `translation_free_string(ptr)` / `translation_destroy(engine)`
  - 内部：Bergamot（`beam-size` 按质量档位：fast 1 / balanced 2，`gemm-precision=int8shiftAlphaAll`，`workspace=128`，`mini-batch-words=1024`）；CTranslate2（fast beam 1 / balanced beam 4，`max_input_tokens=256`，inter 1 / intra 2）；SentencePiece 编解码用模型自带 source/target spm（不得混用其他模型 spm，指南 §6.4/§20）
  - 全部文本以 UTF-8 传递，禁止系统 ACP/ANSI 中转（指南 §20）
- Rust FFI：优先 `libloading` 动态加载 `translation_bridge.dll`（路径解析：模型目录同级 `native/` 或随包资源目录；运行时找不到 dll → `TranslationError::ModelLoad` 明确报错）；绑定代码 `unsafe extern "C"` + `// SAFETY:` 注释；`NativeTranslator` 结构 `Send + Sync` 封装与 Drop 释放
- 行为变更：
  - `LocalTranslationProvider` / `local_onnx.rs` 删除（A3）；`lib.rs` re-export 更新；README 同步
  - 取消语义（B3）：原生调用为不可中断阻塞 → `spawn_blocking` + 调用前检查 token + 调用完成后检查；超时/取消返回 `TranslationError::Cancelled`；README 登记「进行中的原生推理无法中断」已知限制
  - 日志：`provider_id`、`model_id`、`source/target`、`elapsed_ms`、`text_len`；**禁止**原文/译文完整内容（`truncate_for_log`）
- 约束（非实现代码）：
  - 线程上限（B5）：Bergamot 2 线程、CTranslate2 intra 2 / inter 1、总并发不随核心数膨胀
  - 锁版本：Bergamot v0.4.5、CTranslate2 4.8.1、SentencePiece（版本固定）；构建步骤写入 `native/translation_bridge/README.md`
  - 许可证：`licenses/` 登记 Bergamot（MPL-2.0）、CTranslate2（MIT）、MarianMT/SentencePiece（Apache-2.0）NOTICE（B6）
  - 产物约定：构建输出 `translation_bridge.dll` 到 `src-tauri/resources/native/`（打包声明由 10 任务在 `tauri.conf.json` 完成）
- 测试要求：
  - 单测：错误码→`TranslationError` 映射（0-7）、`Auto`/不支持对拒绝、quality 档位 → beam 参数映射、FFI 空指针/编码错误路径
  - 集成（默认 ignore，需真实模型）：`examples/translation_verify.rs` 更新为双引擎模式（`--text "..." --source en|ja --models <dir>`），验证 en→zh 与 ja→zh 各至少 1 条固定样本（质量回归样本按指南 §27 建 `tests/fixtures/`，文本 ≤10KB）
  - 既有 API Provider 测试全量回归（`api.rs` / `retry.rs` / `prompt.rs` / `validate.rs` 不动）
- 文档要求：crate README 全面更新（双引擎架构、生命周期、取消语义、线程约束、已知限制）；`docs/modules/07-translation.md` 同步（公开 API、错误映射、测试计划、验收标准）；`native/translation_bridge/README.md`（构建步骤、依赖版本、许可证）

### 横切标准提醒

- 日志：`#[tracing::instrument]`；错误路径 `warn!`/`error!`；敏感数据红线（原文/译文/Key 禁止完整记录）
- 错误：复用 `vtrans_core::TranslationError` 变体（`ModelLoad` / `Inference` / `UnsupportedPair` / `Cancelled` / `InvalidArgument` 语义映射到现有变体），不新增变体；错误链完整
- 测试与风格：fmt / clippy 零警告；`unsafe` 全部 `// SAFETY:`；公开 API rustdoc 含 `# Example`

### 完成定义（DoD）

- [ ] 质量门禁通过：`cargo fmt --all -- --check`；`cargo clippy -p vtrans-translation --all-targets`；`cargo test -p vtrans-translation`
- [ ] `native/translation_bridge/` 构建脚本可在 Windows 复现出 dll；FFI 绑定 + Provider 编译通过
- [ ] 真实模型（en-zh + ja-zh）集成验证：`translation_verify` 双引擎路径各样本通过；体积与 manifest 校验通过
- [ ] 旧 `local_onnx.rs` 已删除（A3）；`ort` 依赖未被误删
- [ ] 未修改其他 crate 与 vtrans-core；未提交模型二进制
- [ ] PR 描述含实现说明、构建步骤、测试覆盖、验收 checklist
