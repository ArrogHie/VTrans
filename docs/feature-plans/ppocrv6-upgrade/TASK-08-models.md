## 模块开发说明：08 vtrans-models — PP-OCRv6 模型清单与准备 增量

### AGENT_DEV_PROMPT 参数

- MODULE_NUMBER: 08
- MODULE_NAME: vtrans-models
- MODULE_SLUG: models
- CRATE_PATH: crates/vtrans-models
- SCOPE: models（含 `scripts/download_models.ps1`、`src-tauri/resources/models`、`docs/DEVELOPMENT.md` §4、`docs/modules/08-models.md`、`crates/vtrans-models/README.md`，按 GIT_WORKFLOW 定义）
- BRANCH_NAME: `feat/08-ppocrv6-models`

### 功能上下文

- 功能目标：OCR 模型升级 PP-OCRv4 → PP-OCRv6 Small，统一 rec 槽位，解锁 `auto` / `zh-CN`；彻底弃用 v4
- 本模块承担：manifest schema 扩展、新 manifest.json（v6 哈希/大小）、**完整可复现的下载/转换/检查/基准/回填脚本**、模型准备文档
- 上游已提供：无（本任务先行）；参考 `docs/PP-OCRv6_small_ONNX_Rust_TS_接入指南.md`（未跟踪，需 `git add` 纳入版本库）
- 已确认决策：方案 B（仓库脚本包含转换步骤）；rec_ja / rec_en / rec_multi 指向同一 v6 rec 模型与同一字典；**不做与 v4 的对比测试**

### 任务要求

- 范围：仅限本模块与上述 SCOPE；禁止修改其他 crate；禁止修改 vtrans-core
- 新增公开 API（约束性定义，命名可微调）：
  - `PreprocessParams` 新增**可选字段**（serde default，保持 manifest version 1 向后兼容）：
    - `box_threshold: Option<f32>`（默认 0.45）
    - `max_candidates: Option<usize>`（默认 3000）
    - `min_box_size: Option<f32>`（默认 3.0）
    - `rec_input_height: Option<u32>`（默认 48）
    - `rec_input_width: Option<u32>`（默认 320）
    - `rec_append_space: Option<bool>`（默认 true）
    - `rec_blank_index: Option<usize>`（默认 0）
  - 语义与默认值必须与指南 §10.1 一致；缺省字段反序列化后取上述默认
- manifest.json 更新：
  - det：`PP-OCRv6_small_det` ONNX（约 9.6 MB，以转换产物为准）
  - rec_ja / rec_en / rec_multi：**同一** `PP-OCRv6_small_rec` ONNX（约 20.4 MB，以转换产物为准）
  - dicts：`ppocrv6_dict.txt`，三个 key（`ja` / `en` / `auto`）指向同一文件；若模型包内嵌 `inference.yml` 字符表，以模型包为准并在文档记录
  - preprocess_params：det_threshold 0.2、unclip_ratio 1.4、mean/std 0.485/0.456/0.406 与 0.229/0.224/0.225（通道顺序以 Python 基准确认为准，先按指南 BGR 记录）；新增 det/rec 字段取指南 §10.1 默认值
  - translation 组保持不变；SHA-256 / size_bytes 由实际转换产物回填，禁止占位值
- 脚本（方案 B，`scripts/` 内新增或改造）：
  1. 下载：`PP-OCRv6_small_det_infer.tar` / `PP-OCRv6_small_rec_infer.tar`（官方 bcebos 地址）+ `ppocrv6_dict.txt`（官方仓库）
  2. 解压 + PaddleX paddle2onnx 转换：det / rec 分别转换；opset 以实际转换结果为准，不假定 7
  3. ONNX 检查：记录真实输入/输出节点名、dtype、shape、opset；禁止硬编码节点名（指南 §5.3）
  4. Python 基准：对固定测试图生成 det/rec 中间产物与 JSON（指南 §14），供 05 对照；基准输出与 Rust 侧后续回归一致（v6 自身正确性断言，不涉及 v4 对比）
  5. 回填 manifest：根据实际产物自动计算 SHA-256 / size_bytes 并写入 manifest.json（或输出待回填值）
  - 开发机要求：Python 3.10 或 3.11、PaddlePaddle 3.0+、`paddlex[ocr]`、paddle2onnx 插件、`onnx`、`onnxruntime`、`opencv-python-headless`（写进脚本注释与文档，Windows 转换问题可用 WSL2）
- 约束：
  - 模型文件不提交 Git（`.gitignore` 已有 `*.onnx` 规则）；字典文件建议提交，便于完整性校验
  - 类数一致性：`output C == dict 行数 + blank + space`，不一致必须报错并输出诊断（指南 §9.4）
  - 不保留 v4 下载项、v4 哈希与 v4 专有参数；不引入 v4 对比测试或基线数据
- 测试要求：schema 扩展反序列化（缺省值生效）、旧 manifest（无新字段）仍可解析、SHA-256 校验路径不变
- 文档要求：README 同步（schema 变更、脚本用法、开发机依赖）；`docs/modules/08-models.md` 同步（新增字段表）；`docs/DEVELOPMENT.md` §4 同步（v6 全流程步骤）
- 提交规范：`feat(models): ...`，可多次提交，每次可编译

### 横切标准提醒

- 日志：不记录模型路径之外的敏感信息；哈希校验失败走 `ModelError::HashMismatch` 结构化输出
- 错误：归属 `ModelError`；新字段解析失败走 serde 默认，不引入新错误变体（如确需新增，先与协调确认）
- 测试与风格：cargo fmt / clippy / test 零警告零失败；公开 API 有 rustdoc

### 完成定义（DoD）

- [ ] `cargo fmt --all -- --check`；`cargo clippy -p vtrans-models --all-targets`；`cargo test -p vtrans-models`
- [ ] 新 manifest（v6）可被 `vtrans-verify-models` 校验（模型就绪时全部通过）
- [ ] 旧 manifest（v4 字段）反序列化不回归（schema 向后兼容）
- [ ] 下载 → 转换 → 检查 → 基准 → 回填全流程脚本在文档说明的开发机上可复现
- [ ] 未修改其他 crate 与 vtrans-core
- [ ] PR 描述含实现说明、测试覆盖、验收 checklist
