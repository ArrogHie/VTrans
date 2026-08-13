# 模块开发说明：文档同步 — 多框实时翻译契约补齐（任务 D1）

## 背景
多框实时翻译（2026-08-13 整合入 main）涉及 02-config、06-text、09-pipeline、10-app 四个模块。各 crate README 已更新，但 `docs/modules/` 与 `docs/ARCHITECTURE.md` 未同步。用户确认派单补齐。

## 任务拆分（按模块派给对应开发 Agent，可并行）

### D1-02 config 文档同步
- MODULE_SLUG: config；CRATE_PATH: crates/vtrans-config
- 更新 `docs/modules/02-config.md`：
  - AppConfig 新增字段 `translation_boxes: Vec<TranslationBoxConfig>`（id/region/color）、`max_boxes`（默认 8）、`warning_threshold`（默认 4，0 禁用）
  - schema 版本 v5→v6 迁移说明（`CURRENT_CONFIG_VERSION = 6`）
  - 颜色调色板与 `next_box_id`/`next_box_color` 分配规则
- 提交规范：`docs(config): sync multi-box config contract to module spec`

### D1-06 text 文档同步
- MODULE_SLUG: text；CRATE_PATH: crates/vtrans-text
- 更新 `docs/modules/06-text.md`：
  - `BoxFingerprintCache` per-box 去重 API（`crates/vtrans-text/src/box_dedup.rs` 公开方法）
  - 与单框指纹去重的关系（框间指纹隔离）
- 提交规范：`docs(text): sync per-box fingerprint dedup to module spec`

### D1-09 pipeline 文档同步
- MODULE_SLUG: pipeline；CRATE_PATH: crates/vtrans-pipeline
- 更新 `docs/modules/09-pipeline.md`：
  - `TranslationBox`、`BoxedTranslationResult`、`BoxStatus`、`MultiBoxConfig`、`MultiBoxPipeline` 公开 API 清单（add_box/remove_box/update_box/start_all/stop_all/stop_box/subscribe_results/box_status/box_count）
  - 每框独立 tokio task、独立 CancellationToken、帧差检测与指纹去重、有界通道（broadcast）、错误隔离模型
  - 注意：F1 增量（`BoxedTranslationResult.original_text`）落地后文档需再次更新，以当时 main 状态为准
- 提交规范：`docs(pipeline): sync multi-box pipeline API to module spec`

### D1-10 app + ARCHITECTURE 文档同步
- MODULE_SLUG: app；CRATE_PATH: crates/vtrans-app
- 更新 `docs/modules/10-app.md`：
  - 8 个新增 Command：`add_translation_box` / `remove_translation_box` / `update_translation_box` / `list_translation_boxes` / `start_multi_realtime` / `stop_multi_realtime` / `stop_box` / `open_result_window`（签名与前端 camelCase 参数说明）
  - 7 个新增 Event：`multibox://result` / `box-added` / `box-removed` / `box-updated` / `status` / `warning`、`translation://single-result`（payload 形状）
  - AppState 多框字段与 forwarder 任务、热键语义（Alt+Shift+R/S 控制单框实时，多框仅 UI 按钮——用户已确认的设计决策）
- 更新 `docs/ARCHITECTURE.md`：
  - 模块 09/10 职责描述补充多框能力；IPC 契约节补充多框 Command/Event 清单；已知限制节补充「多框结果不含原文（后续迭代 F1 补齐）」
- 提交规范：`docs(app): sync multi-box commands/events to module spec` + `docs(arch): sync multi-box IPC contract`

## 通用约束
- 只改 docs/ 下文档，禁止改任何源码/测试。
- 以代码为事实来源（commands.rs / events.rs / multibox.rs / schema.rs），文档与代码冲突时按代码写并回馈统筹。
- 每张单据独立分支（按 GIT_WORKFLOW 命名），同层可并行；完成后由统筹 Review 并合并。
- 文档中不出现 API Key、完整原文/译文等敏感信息。

## 完成定义（DoD）
- [ ] 对应 docs/modules/NN-*.md（及 ARCHITECTURE.md）已同步多框契约
- [ ] 与 main 代码逐项核对无出入（命令/事件名、字段名、默认值、版本号）
- [ ] 无源码/测试改动
- [ ] 提交规范符合 `docs(scope): ...` 格式
