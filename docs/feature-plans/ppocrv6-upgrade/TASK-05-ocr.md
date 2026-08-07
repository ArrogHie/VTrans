## 模块开发说明：05 vtrans-ocr — PP-OCRv6 识别适配 增量

### AGENT_DEV_PROMPT 参数

- MODULE_NUMBER: 05
- MODULE_NAME: vtrans-ocr
- MODULE_SLUG: ocr
- CRATE_PATH: crates/vtrans-ocr
- SCOPE: ocr（含 `docs/modules/05-ocr.md`、`crates/vtrans-ocr/README.md`、`crates/vtrans-ocr/tests`、`crates/vtrans-ocr/examples/ocr_verify.rs`，按 GIT_WORKFLOW 定义）
- BRANCH_NAME: `feat/05-ppocrv6-ocr`（依赖 08 合并到 main 后从 main 拉分支）

### 功能上下文

- 功能目标：OCR 模型升级 PP-OCRv4 → PP-OCRv6 Small；统一 rec 槽位；`auto` / `zh-CN` 解锁；彻底弃用 v4
- 本模块承担：det/rec 预处理与后处理适配 v6、字典装载与类数校验、回归测试
- 上游已提供：08 合并后的 `PreprocessParams` 扩展字段（box_threshold / max_candidates / min_box_size / rec_input_height / rec_input_width / rec_append_space / rec_blank_index）与新 manifest（rec_ja / rec_en / rec_multi 同一 v6 rec 模型，字典 ja / en / auto 同一文件）
- 已确认决策：不保留 v4 兼容路径；**不进行任何与 v4 的对比测试**（含英文）；日文/竖排质量不纳入验收；验收以 v6 固定测试集断言 + Python 基准对照为准

### 任务要求

- 范围：仅限本模块；禁止修改其他 crate；禁止修改 vtrans-core
- 行为变更（约束性定义，实现细节由开发 Agent 定）：
  1. 识别预处理：输入高度 32 → 48（manifest `rec_input_height`）；宽度上限 320（`rec_input_width`）；归一化 `(x/255-0.5)/0.5` 不变；若 ONNX 为固定宽度则右侧补 0 到 320（以 inspect 结果为准）
  2. 长行分片：分片宽度/重叠逻辑保留，高度参数随 48 调整；分片回归测试保持通过
  3. 检测预处理：若 ONNX 固定 `[1,3,640,640]` → 直接 resize 到 640×640，ratio_x / ratio_y 分开保存（指南 §6.2）；若动态宽高 → 复现 DetResizeForTest（目标边为 32 的倍数）；以 inspect 结果为准，不臆造
  4. DB 后处理：新增 box_threshold（0.45）分数过滤、max_candidates（3000）候选上限、min_box_size（最短边约 3px）过滤；unclip_ratio 随 manifest（1.4）；坐标还原两段映射（output map → det input → 原图）保持正确
  5. 通道顺序：以 Python 基准对照结果为准；指南默认 BGR，若基准证明模型接受 RGB 则保持现状并记录决策
  6. 字典：以实际 ONNX 元数据 / 模型包字符表为准，否则用 `ppocrv6_dict.txt`；类数一致性（`output C == dict.len()`）在**加载期**校验 fail-fast，解码期校验保留；错误信息含输出 shape、字典行数、是否追加空格、blank index（指南 §9.4）
  7. 输入/输出节点名继续从 session metadata 读取，不硬编码（现有实现已如此，保持）
  8. **弃用 v4**：移除 v4 专有分支与 v4 文档描述；manifest 不再引用 v4 文件；不实现 v4 回退；不建立 v4 对比基线
- 约束：
  - `OcrProvider` trait / `OcrOptions` / `OcrResult` / `OcrError` 零改动
  - 不跨 IPC 传图；日志不记录完整文本（保留 truncate 语义）
  - `auto` / `zh-CN` 走 rec_multi 槽位（08 已配置，指向 v6 rec）
- 测试要求：
  - 单测：新参数默认值解析与传递、box_threshold 过滤、max_candidates 上限、min_box_size 过滤、48 高预处理 shape、固定 320 补零、类数不一致报错
  - 集成（默认 ignore，需模型）：`tests/long_line_regression.rs` 更新期望、`tests/provider_load.rs`、`examples/ocr_verify.rs` 手动验证（英文固定测试集 + 中文 auto/zh-CN）
  - 语言路由单测保持（auto / ja / en / zh-CN 均可用）
  - 与 Python 基准（08 提供）逐文件对照：det 输入/输出误差、CTC 文本一致（指南 §14）
- 文档要求：README 同步（识别高度 48、分片、字典来源、已知限制更新——移除「32 高硬编码」限制，新增「v6 日文/竖排质量未专项验收」与「已弃用 v4」）；`docs/modules/05-ocr.md` 同步
- 提交规范：`feat(ocr): ...`，可多次提交，每次可编译

### 横切标准提醒

- 日志：DEBUG 记录 shape / 参数 / 耗时，禁止完整文本；错误路径 warn/error
- 错误：复用 `OcrError` 现有变体（Preprocess / Postprocess / OrtRuntime / InvalidManifest），不新增变体（如确需新增，先与协调确认）
- 测试与风格：fmt / clippy / test 零警告零失败；rustdoc 完整

### 完成定义（DoD）

- [ ] `cargo fmt --all -- --check`；`cargo clippy -p vtrans-ocr --all-targets`；`cargo test -p vtrans-ocr`
- [ ] 新增单测全部通过；旧用例无回归
- [ ] 用 v6 模型跑通 `ocr_verify`（英文测试图 + 中文 auto/zh-CN），与 Python 基准文本一致
- [ ] 类数不一致场景 fail-fast 报错信息符合指南 §9.4
- [ ] 未修改其他 crate 与 vtrans-core
- [ ] PR 描述含实现说明、测试覆盖、验收 checklist
