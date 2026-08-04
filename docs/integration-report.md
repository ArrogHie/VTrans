# VTrans MVP 整合报告（integration/mvp）

> 分支：`integration/mvp`（→ `main`）
> 日期：2026-08-04
> 范围：11 个模块（vtrans-core … frontend）装配为可运行 MVP，打通单次/实时垂直链路。

## 1. 基线结果（整合前 / 整合后）

整合前基线（`main` 顶部，全部命令在仓库根执行）：

| 命令 | 结果 | 失败清单 |
|------|------|----------|
| `cargo check --workspace --all-targets` | ✅ 通过 | 无 |
| `cargo fmt --all -- --check` | ✅ 通过 | 无 |
| `cargo clippy --workspace --all-targets` | ✅ 通过 | 无 |
| `cargo test --workspace` | ✅ 通过 | 无（约 510 个测试全绿） |
| `pnpm test` | ❌ 挂起 | `vitest` watch 模式在非 TTY 下不退出 |
| `pnpm test`（等价单次：`pnpm exec vitest run`） | ✅ 通过 | 38 个前端测试 |
| `pnpm build` | ✅ 通过 | 无 |

整合后：

| 命令 | 结果 | 备注 |
|------|------|------|
| `cargo check --workspace --all-targets` | ✅ 通过 | |
| `cargo fmt --all -- --check` | ✅ 通过 | |
| `cargo clippy --workspace --all-targets` | ✅ 通过 | 含新增 bin 零警告 |
| `cargo test --workspace` | ✅ 通过 | 约 510 个测试全绿 |
| `pnpm test` | ✅ 通过 | 修复为 `vitest run`，43 个前端测试 |
| `pnpm build` | ✅ 通过 | tsc + vite 构建成功 |
| `cargo tauri build --no-bundle` | ✅ 通过 | Release exe 产出并冒烟 |
| `cargo tauri build`（含打包） | ⚠️ 部分完成 | 编译与 exe 产出成功；MSI/NSIS 打包需联网下载 WiX，本环境 GitHub 不可达（见 §6） |

## 2. 契约审计结论（Step 1）

对照 `docs/ARCHITECTURE.md` §4 Phase 0.5 冻结点逐项核对：

| 冻结点 | 结论 | 证据 |
|--------|------|------|
| 核心类型全部来自 vtrans-core，下游无重复定义 | ✅ 一致 | `rg` 确认 `ScreenRegion`/`OcrResult`/`CapturedImage`/`Language`/`TranslationRequest`/`OcrLine`/`PipelineMode`/`PipelineStatus` 及四个 trait 仅定义在 `vtrans-core` |
| serde 表示确定；`CapturedImage` 不实现 Serialize | ✅ 一致 | `types.rs`：`CapturedImage` 仅 `#[derive(Debug, Clone)]`；`Language` 序列化为 `auto`/`zh-CN`/`ja`/`en` |
| Provider trait 签名固定（含 CancellationToken） | ✅ 一致 | `traits.rs` 与契约 §6.2 逐字一致 |
| 错误类型变体完整 | ✅ 一致 | `CaptureError`/`OcrError`/`TranslationError` 变体与模块文档一致；`PipelineError`/`AppError` 自各 crate 定义且 `#[from]` 转换完整 |
| AppConfig schema 全字段 | ✅ 一致 | capture/ocr/translation/result_window/hotkeys/log_level/model_dir/version 齐备 |
| ModelManifest schema 覆盖 OCR + translation | ✅ 一致 | det/rec_ja/rec_en/rec_multi/dicts/preprocess_params + model/tokenizer/supported_pairs/max_length/inference_params 齐备 |
| PipelineDeps / PipelineConfig 形状 | ✅ 一致 | `PipelineDeps { capture, ocr, translation }` + `PipelineConfig` 与契约一致 |

**结论：冻结契约零偏差，无需修改 vtrans-core 公共接口，无「待架构评审」项。**

## 3. 装配审计结论（Step 2）

| 审计项 | 结论 |
|--------|------|
| AppState 依赖注入 | ✅ `state.rs` 组装 config/credentials/capture/ocr/translation/models/pipeline；`init_app` 顺序（配置 → 日志 → AppState → attach_handle → 快捷键）正确；`WorkerGuard` 由 `LoggingGuard` 托管 |
| Provider id 契约 | ✅ 后端白名单 `"api"`/`"local"`（`validate_translation_provider_id`）；`AppStatus.translation_provider` 返回实现 id `"api"`/`"local-onnx"`；前端 `normalizeProviderId` 映射回退正确；`applyStatus` 水合不丢失 |
| Commands 对齐 | ✅ 前端调用的 13 个命令全部注册（含 `cancel_region_selection`），参数与返回类型匹配 |
| Events 对齐 | ✅ 9 个后端事件名与 payload 与前端 `EventPayloadMap` 一致；另有 4 个前端内部协调事件 |
| Capability 最小化 | ✅ 单一 `default.json`，仅 core 默认 + event 默认 + 窗口 hide/set-always-on-top/set-focus/show/start-dragging；图像不跨 IPC（`CapturedImage` 无 Serialize） |
| 快捷键 | ✅ Alt+Shift+A / Alt+Shift+R / Alt+Shift+S 注册成功（Release 日志 `global shortcuts registered count=3`）；冲突返回 `HotkeyFailed`；改键需重启与 README 一致 |

## 4. 装配改动清单（本分支实际修改）

| 文件 | 改动 | 理由 |
|------|------|------|
| `package.json` | `test`: `vitest` → `vitest run` | 非 TTY 下 `pnpm test` 挂起（基线失败项），单次模式与 CI 语义一致 |
| `crates/vtrans-models/Cargo.toml` | 新增 `[[bin]] vtrans-verify-models` | 补齐文档引用的验证二进制 |
| `crates/vtrans-models/src/bin/verify_models.rs` | 新增完整性校验 CLI | 与 `load_local_models` 同一 `verify_integrity` 逻辑；`--models` / `$VTRANS_MODEL_DIR` 指定目录 |
| `crates/vtrans-models/README.md` | 同步验证 CLI 说明 | 消除「未提供验证 CLI」旧描述 |
| `src/types/index.ts` | 新增 `isLocalPairSupported` | 本地模型仅 en→zh-CN，UI 需提示 |
| `src/windows/MainWindow.tsx` | local + 不支持语言对时显示提示条 | 满足 MVP「源语言受 Provider 能力约束时给出明确提示」 |
| `src/test/types.test.ts` | 新增 5 个 `isLocalPairSupported` 用例 | 覆盖本地模型语言对约束 |
| `docs/DEVELOPMENT.md` | 示例命令补 `-p vtrans-ocr` / `-p vtrans-translation`；日志轮转描述改为按小时/保留 5 个 | 与实现一致（`vtrans_core::logging` 为 HOURLY + max 5） |
| `crates/vtrans-translation/README.md` | 验证 CLI 命令补 `-p vtrans-translation` | 与实现一致 |
| `crates/vtrans-capture/examples/capture_demo.rs` | **保留并提交**未跟踪示例 | 归属判定：capture 模块文档要求有独立验证入口，文件可编译、可运行；保留为模块示例（见 §6） |

## 5. 垂直链路验证记录（Step 3/4/5）

### 5.1 单次链路 — 本地翻译（真实模型）

```powershell
cargo run -p vtrans-pipeline --example pipeline_verify -- `
  --models src-tauri/resources/models --language en --target zh-CN --mode single
```

实测输出：捕获 800×600（主屏）→ OCR 880 ms / 1 行 → `local-onnx` 翻译 80 ms → 事件顺序
`CaptureStarted → OcrStarted → OcrCompleted → TranslationStarted → TranslationCompleted → Stopped` 完整。

### 5.2 单次链路 — API Provider（mock endpoint）

本机无可用 API Key（环境变量与 Credential Manager 均无 `VTrans:translation`），用本地
mock chat/completions 服务（127.0.0.1:18923）验证 API Provider 全链路：

```powershell
cargo run -p vtrans-pipeline --example pipeline_verify -- `
  --models src-tauri/resources/models --api-endpoint http://127.0.0.1:18923/v1/chat/completions `
  --api-model mock-gpt --api-key sk-test1234 --language en --target zh-CN --mode single
```

实测：OCR 607 ms / 22 行 → `api` Provider 6 ms → 返回 `MOCK:<原文>`，证明请求构造、发送、
响应解析路径正确。真实 API 的超时/重试/401/429 映射由 `vtrans-translation/tests/api_provider.rs`
集成测试覆盖（9 项，全绿）。

### 5.3 实时链路（真实模型 + 真实屏幕）

```powershell
cargo run -p vtrans-pipeline --example pipeline_verify -- `
  --models src-tauri/resources/models --language en --target zh-CN --mode live `
  --region 100,100,800,400 --interval-ms 500
```

实测（55 秒）：首帧 → OCR 462 ms / 14 行 → `local-onnx` 翻译 389 ms；此后区域静止，
连续 50+ 帧仅 `[capture] frame captured`（帧差检测过滤），**未重复触发 OCR/翻译**。
停止语义（干净退出、worker 终止 < 5 s）由 `crates/vtrans-pipeline/tests/pipeline_live.rs`
覆盖：unchanged_frames_are_skipped / unchanged_text_is_not_retranslated /
newer_frame_cancels_previous_ocr / at_most_one_ocr_and_one_translation_run_concurrently /
stop_terminates_all_workers / region_update_restarts_the_session_without_stopping /
ocr_worker_queue_stays_bounded_under_burst 等 12 项全部通过。

> 说明：无头 CLI 无法发送 Ctrl+C，实测终止方式为进程终止；干净停止由上述集成测试断言。

### 5.4 Release 应用冒烟（Step 7）

`cargo tauri build --no-bundle` 产出 `target/release/vtrans.exe`，部署模型到
`%APPDATA%\com.vtrans.app\models` 后启动：

- 进程存活，主窗口标题 `VTrans`，响应正常；`config.json` 首次运行自动生成（默认值完整）。
- 启动日志：`manifest loaded` → `WindowsCaptureSource initialized count=1` →
  ONNX det/rec_ja/rec_en 三 session 加载（合计 ~280 ms）→ `application state initialized
  ocr_provider="pp-ocr" translation_provider="api"` → `global shortcuts registered count=3`。
- 切换 `provider=local` 重启：`local translation provider initialized
  model_id=opus-mt-en-zh-int8 model_kind=Generation supported_pairs=1 max_length=512
  num_beams=4 elapsed_ms=1171`，`translation_provider="local-onnx"`；进程内存约 972 MB
  （403 MB 模型驻留，符合预期）。测试后已恢复默认 `provider=api`。
- 日志中 `failed to set per-monitor DPI awareness`（0x80070005）为非致命降级：宿主已设置
  DPI awareness，capture 继续用系统 DPI；坐标转换逻辑有单测覆盖。

### 5.5 模型与资源检查（Step 5）

- SHA-256：manifest 中 det/rec_ja/rec_en/translation model/tokenizer/dict_en/dict_ja 共 7 个
  文件全部与 manifest 哈希一致（含 403 MB 模型）。
- 新增 `vtrans-verify-models` 实测：`verified 7/7 model files … all model files are valid`。
- 本地模型仅 en→zh-CN：后端 `LocalTranslationProvider` 对不支持语言对返回
  `TranslationError::UnsupportedPair`（含 Auto 源，不静默）；前端新增
  `isLocalPairSupported` 提示条「本地模型目前仅支持 en → zh-CN…请切换到云端 API」。
- API Key：仅从 Windows Credential Manager（`VTrans:translation`）读取（`state.rs`），
  `AppConfig` 无 key 字段，日志只出现掩码（`mask_sensitive`），无明文。

## 6. 未解决项与风险

| # | 未解决项 | 处理/风险 |
|---|----------|-----------|
| 1 | **安装包打包**：`cargo tauri build` 的 MSI/NSIS 步骤需联网下载 WiX/NSIS，本环境 GitHub 不可达（Connection refused），仅产出并验证 Release exe | 待有网络环境执行 `cargo tauri build` 并安装冒烟；exe 级启动已验证，风险低 |
| 2 | **无 git remote**：本仓库未配置 origin，无法 `git pull origin main` / push / 创建 PR | 已在本地完成 `integration/mvp` 分支与提交；PR 创建需配置 remote 后执行 |
| 3 | **GUI 交互项**（拖动框选、快捷键真实触发、结果窗口交互、多显示器实机）为 `vtrans-app/README.md` 登记的手工验证项 | 本环境未自动执行；代码路径、事件契约与命令注册已核对，坐标/事件逻辑有单测 |
| 4 | **30 分钟长稳**：未做 30 分钟内存/任务堆积实测 | 实时链路通道 cap=1、帧差过滤、指纹去重保证队列有界；集成测试覆盖 burst 场景；标记为后续人工长测项 |
| 5 | **日文 OCR 实测**：rec_ja 模型已加载（Release 日志），但本机屏幕无日文内容，未做真实日文识别 | ocr 模块有 `ocr_verify` CLI 与测试素材；清晰日文识别验收待手工/测试图 |
| 6 | **DPI awareness 降级警告**：宿主已设置 DPI awareness 时 capture 非致命降级 | 已记录日志；多显示器/高 DPI 实机验证归入手工验证项 |
| 7 | **AppData 模型副本**：为 Release 冒烟在 `%APPDATA%\com.vtrans.app\models` 部署了约 420 MB 模型 | 应用运行必需；可通过删除目录或设置 `config.model_dir` 调整 |
| 8 | `docs/AGENT_*.md` 为 Agent 提示词工作文件（未跟踪） | 不属于交付物，保留未跟踪，未提交 |

## 7. MVP 验收标准 Checklist

对照 `windows_screen_translator_agent_spec.md` §14：

- [x] **Windows 10/11 可安装并启动**：Release exe 启动/初始化/快捷键注册验证通过；安装包生成受网络限制（§6-1）
- [~] **可在任意显示器框选区域（含多显示器与 DPI 缩放）**：选区窗口 + 坐标换算（物理像素）代码完整，坐标单测 7 项通过；多显示器实机待手工验证（§6-3）
- [~] **可识别清晰的日文和英文屏幕文字**：英文真实截图识别成功（单次/实时均验证）；日文模型加载成功，清晰文本识别待手工/测试图（§6-5）
- [x] **可完成中、日、英任意目标语言翻译（源语言受 Provider 能力约束时给出明确提示）**：API Provider 全语言组合有单测；本地模型仅 en→zh-CN 且 UI 明确提示
- [x] **实时模式只在画面或文本变化时触发翻译**：CLI 实测静止区域不重复触发；帧差/指纹去重有集成测试
- [~] **连续运行 30 分钟无明显内存增长、崩溃或任务堆积**：架构保证（cap=1、去重、取消）；30 分钟实机长测待做（§6-4）
- [x] **停止按钮能在短时间内终止捕获、OCR 和翻译**：`stop_terminates_all_workers` 等集成测试断言 < 5 s
- [x] **API Key 不出现在配置文件和日志中**：仅 Credential Manager 存取，日志掩码
- [x] **默认不保存截图及翻译内容**：无截图/译文持久化代码路径
- [x] **API 与本地翻译可通过配置切换，业务流水线无需修改**：CLI 与 Release 应用双路径验证，切换仅更换注入 Provider

## 8. 文档差异修正清单

| 文档 | 差异 | 处理 |
|------|------|------|
| `docs/DEVELOPMENT.md` | `vtrans-verify-models` 二进制不存在 | 补最小 bin（vtrans-models），文档保持有效 |
| `docs/DEVELOPMENT.md` §6.3 | 验证 CLI 示例缺 `-p` 前缀 | 修正 |
| `docs/DEVELOPMENT.md` §7.1 | 「10MB 轮转」与实现（按小时、保留 5 个）不符 | 修正 |
| `crates/vtrans-translation/README.md` | 验证 CLI 示例缺 `-p` 前缀 | 修正 |
| `crates/vtrans-models/README.md` | 「未提供验证 CLI」旧描述 | 同步为 `vtrans-verify-models` |

## 9. 提交清单

按 `docs/GIT_WORKFLOW.md` 提交规范整理（`<type>(<scope>): <subject>`）：

1. `feat(models): add vtrans-verify-models integrity CLI`
2. `feat(capture): keep minimal capture demo example`
3. `fix(frontend): run vitest once and surface local model language limit`
4. `docs(dev): align example commands and log rotation with implementation`
5. `docs(app): add MVP integration report`

全部提交保持 `cargo check --workspace --all-targets` 可编译。
