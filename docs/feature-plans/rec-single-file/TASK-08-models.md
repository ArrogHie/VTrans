## 模块开发说明：08 vtrans-models — rec 模型单份化 增量

### AGENT_DEV_PROMPT 参数

- MODULE_NUMBER: 08
- MODULE_NAME: vtrans-models
- MODULE_SLUG: models
- CRATE_PATH: crates/vtrans-models
- SCOPE: models（含 `scripts/ppocrv6/`、`src-tauri/resources/models/`、`docs/DEVELOPMENT.md` §4、`docs/modules/08-models.md`、`crates/vtrans-models/README.md`，按 GIT_WORKFLOW 定义）
- BRANCH_NAME: `feat/08-rec-single-file`

### 功能上下文

- 功能目标：rec_ja / rec_en / rec_multi 三个槽位指向同一份 ONNX 文件，删除两份冗余拷贝（2 × 21,159,378 B ≈ 42.3 MB），运行时只加载一个 rec session
- 本模块承担：两份 manifest.json 的 rec path 收敛、`backfill_manifest.py` 部署单文件化、`setup_ppocrv6.ps1` 注释同步、本地冗余文件清理、文档同步
- 上游已提供：05 已合并 main，`PaddleOcrProvider` 按 path 相等共享 rec session（`rec_en_shared` / `rec_multi_shared` → `Arc::clone`），本次**无需运行时代码改动**
- 已确认决策：单份文件命名为 `ocr/rec.onnx`；三槽位 id 保留（`ppocr-rec-v6` / `ppocr-rec-v6-en` / `ppocr-rec-v6-multi`）；字典与 preprocess_params 不变；模型自动获取/打包不在此任务范围

### 任务要求

- 范围：仅限本模块与上述 SCOPE；禁止修改其他 crate（含 vtrans-core / vtrans-ocr / vtrans-translation）
- manifest.json（两份，`src-tauri/resources/models/manifest.json` 与 `crates/vtrans-models/resources/manifest.json`）：
  - rec_ja / rec_en / rec_multi 三槽位 `path` 全部改为 `ocr/rec.onnx`；`id` / `sha256` / `size_bytes` 保持不变
  - 其余条目（det、dicts、preprocess_params、translation）不动
- `scripts/ppocrv6/backfill_manifest.py`：
  - `targets` 只部署一份 rec（`rec.onnx` → `--rec` 产物），不再复制 rec_en / rec_multi
  - 回填时三槽位写同一 path（`ocr/rec.onnx`），id 映射保留
  - **幂等收敛**：部署前若发现旧文件 `rec_en.onnx` / `rec_multi.onnx` 残留，删除并打印提示，保证重复执行后仍为单份状态
  - 输出信息同步（如打印 deployed 文件名清单）
- `scripts/ppocrv6/setup_ppocrv6.ps1`：头部注释「产物」段改为 `ocr/rec.onnx`（注明三槽位共享）；其余流程不动
- 本地磁盘清理（本 Agent 执行，模型文件不入 git）：
  - 将 `src-tauri/resources/models/ocr/rec_ja.onnx` 重命名为 `ocr/rec.onnx`
  - 删除 `src-tauri/resources/models/ocr/rec_en.onnx` 与 `rec_multi.onnx`
  - 以 `vtrans-verify-models` 通过为验收证据
- 测试要求（建议，不强改既有夹具）：
  - 增补/调整 vtrans-models 单测覆盖「多个槽位指向同一 path 时校验通过、VerifyReport 计数正确」
  - 既有 integrity / manager / ocr provider_load 测试使用合成多文件路径，语义仍合法，无需强改
- 文档要求：
  - `docs/DEVELOPMENT.md` §4 布局：`rec_ja.onnx` / `rec_en.onnx` / `rec_multi.onnx` 三行收敛为一行 `rec.onnx`，注明三槽位共享同一文件
  - `docs/modules/08-models.md` 模型条目表：三槽位「文件」列改为 `ocr/rec.onnx`，注明单文件共享；下方说明文字同步
  - `crates/vtrans-models/README.md`：示例若展示多份 rec 文件，同步为单文件三槽位同 path；`manifest.rs` 文档示例（合成路径）可保留，如顺手更新须保证 doctest 通过
- 提交规范：`feat(models): ...`，可多次提交，每次可编译

### 横切标准提醒

- 日志：不记录敏感信息；路径引用用 `display()`；删除/部署动作有结构化日志
- 错误：仍归属 `ModelError` 体系；不新增错误变体；manifest 解析失败路径不变
- 测试与风格：cargo fmt / clippy / test 零警告零失败；公开 API 有 rustdoc

### 完成定义（DoD）

- [ ] 质量门禁通过：`cargo fmt --all -- --check`；`cargo clippy -p vtrans-models --all-targets`；`cargo test -p vtrans-models`
- [ ] `cargo run --bin vtrans-verify-models -- --models src-tauri/resources/models` 输出 `all model files are valid`
- [ ] 磁盘 `ocr/` 仅一份 rec 模型（`rec.onnx`）；`rec_en.onnx` / `rec_multi.onnx` 不存在
- [ ] 两份 manifest.json 三槽位 `path` 均指向 `ocr/rec.onnx`，sha256 / size 一致
- [ ] 脚本可复现：重复执行 backfill 后仍为单份 rec，无旧文件残留
- [ ] 文档同步（DEVELOPMENT.md §4 / docs/modules/08-models.md / README）
- [ ] 未修改其他 crate 与 vtrans-core
- [ ] PR 描述含实现说明、测试覆盖、验收 checklist
