## 模块开发说明：10 vtrans-app — 翻译模型升级 / 语言统一 增量

### AGENT_DEV_PROMPT 参数

- MODULE_NUMBER: 10
- MODULE_NAME: vtrans-app
- MODULE_SLUG: app
- CRATE_PATH: crates/vtrans-app（另含 `src-tauri/tauri.conf.json` 打包资源声明，属本任务）
- SCOPE: app
- BRANCH_NAME: feat/10-new-translate-model

### 功能上下文

- 功能目标：OCR 语言与翻译源语言强制统一（后端权威联动）；本地 Provider 组装切换到 Native 双引擎；native dll 随包分发；Provider 运行时 id 契约更新（A2）
- 决策状态（已确认 2026-08-07）：A1 质量档位纳入本次（消费 `translation.quality`）；A2 本地实现 id 定为 `"local-native"`
- 本模块承担的部分：`set_ocr_language` / `set_source_language` 双向联动；`build_translation_provider` 改用 `NativeTranslationProvider` 并传入质量档位；`AppStatus.translation_provider` 实现 id；`tauri.conf.json` bundle resources；Provider id 白名单/契约测试同步
- 上游已提供：02 的 `translation.quality` 与跨字段校验；07 的 `NativeTranslationProvider`（`from_manager` + `with_quality`）；08 的 manifest v2

### 任务要求

- 范围：仅限本模块（`crates/vtrans-app`）+ `src-tauri/tauri.conf.json`；禁止修改其他 crate；禁止修改 vtrans-core
- 行为变更（约束性定义）：
  - `set_ocr_language`：写入 `config.ocr.language` **并同步** `config.translation.source_language`（两个字段相等）
  - `set_source_language`：写入 `config.translation.source_language` **并同步** `config.ocr.language`
  - 纯函数保持可单测：`apply_ocr_language(config, lang)` / `apply_source_language(config, lang)` 各自同步两个字段；`save_settings` 整包保存时若两字段不一致，由 02 的 validation 拒绝（`AppError::Config`）
  - `build_translation_provider`：`"local"` 分支改为 `NativeTranslationProvider::from_manager(&model_manager)?.with_quality(parse quality)?`；quality 解析非法值 → `AppError::Config(Validation)`（防御性，正常路径已由 02 校验）
  - `AppStatus.translation_provider` 实现 id：A2 建议 `"local-onnx"` → `"local-native"`；`crates/vtrans-app/README.md` 的 Provider id 契约段同步；`tests/contracts.rs` 如有 id 断言同步
  - `tauri.conf.json`：`bundle.resources` 声明 `resources/native/translation_bridge.dll`（07 已构建到 `src-tauri/resources/native/`）；打包后 dll 与主程序同目录可被动态加载（07 的 FFI 负责查找）
- 约束（非实现代码）：
  - 配置标识符白名单仍为 `"api" | "local"`（`validate_translation_provider_id` 不变）
  - 加载本地 Provider 保持 `spawn_blocking`（native 引擎创建是重操作）；Provider 长驻共享，不每次重建
  - `load_local_models` 校验逻辑不变（08 的 verify 已覆盖新条目）
- 测试要求：
  - `apply_ocr_language` / `apply_source_language` 双向同步单测（改一个字段两个字段都变、其余字段不动）
  - quality 解析：`"fast"`/`"balanced"` 接受、非法值拒绝；Provider 组装函数分支单测（mock ModelManager 不可行时至少覆盖 quality 解析与 provider id 校验纯函数）
  - `validate_translation_provider_id` / `update_translation_provider_config` 既有测试回归
  - 手工验证项：README「手工验证项」补 1 条（本地双引擎加载 + ja→zh 翻译）
- 文档要求：crate README（联动语义、Provider id 契约、打包资源、手工验证项）；`docs/modules/10-app.md` 同步（Commands 行为变更、Provider id 契约、验收标准）

### 横切标准提醒

- 日志：联动与 Provider 切换 `info!`（记录语言 code / provider id）；不记录原文、译文、Key
- 错误：复用 `AppError` 映射（`Config` / `Translation` / `Model`）；无新增变体
- 测试与风格：fmt / clippy 零警告；纯函数 rustdoc

### 完成定义（DoD）

- [ ] 质量门禁通过：`cargo fmt --all -- --check`；`cargo clippy -p vtrans-app --all-targets`；`cargo test -p vtrans-app`
- [ ] 联动命令、quality 组装、Provider id 契约单测全绿；workspace check 通过
- [ ] `tauri.conf.json` 已声明 native dll 资源（构建期可校验）
- [ ] 未修改其他 crate 与 vtrans-core
- [ ] PR 描述含实现说明、测试覆盖、验收 checklist
