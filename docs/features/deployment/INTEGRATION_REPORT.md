# 整合报告：发行部署（单文件夹安装 + 内置 OCR + 翻译模型一键下载）

> 需求：`docs/features/deployment/REQUIREMENTS.md`。所有合并均在本地 main 完成（未 push origin）。

## 合并记录

| 模块 | 分支 | 合并顺序 | 结果 |
|------|------|----------|------|
| 03-security | feat/03-dpapi-file-store | 1 | ✅ 526f93c |
| 08-models | feat/08-manifest-optional-entries | 2 | ✅ 19b7aca |
| 07-translation（整合修复） | fix/07-model-entry-fields | 3 | ✅ ae38b9e |
| 10-app | feat/10-portable-data-layout | 4 | ✅ b87a22d |
| 11-frontend | feat/11-model-download-ui | 5 | ✅ 3e2acf7 |
| 文档同步 | docs/deployment-doc-sync | 6 | ✅ 4da1a49 |

## 集成验证

### workspace 门禁
- `cargo fmt --all -- --check`：PASS
- `cargo clippy --workspace --all-targets`：0 error；1 条既有警告（`vtrans-translation/src/providers/deepl.rs:293` items-after-test-module，非本功能引入，遗留问题 4）
- `cargo test --workspace`：全绿（含 03 的 111、08 的 77、app 的 143+19+5）。注意：`vtrans-translation --test api_provider` 的本地服务器用例在残留代理环境变量（尤其 `all_proxy`）下会 502，清空全部代理变量后 10/10 通过——环境问题非代码问题
- `pnpm test`：46 文件 / 368 测试全绿；`pnpm exec tsc --noEmit`：零错误

### 打包冒烟（验收标准 1 构建侧）
- `cargo tauri build` 成功：`target/release/bundle/nsis/VTrans_0.1.0_x64-setup.exe`（36MB）+ `target/release/bundle/msi/VTrans_0.1.0_x64_en-US.msi`（41MB）
- release 资源目录 `target/release/resources/models/` 实况：manifest.json（含 `optional/download_url/download_size_bytes`）、ocr/det.onnx（9.9MB）+ ocr/rec.onnx（21.2MB）+ 三个字典、translation/tokenizer.json（4.3MB）；**无 translation/model.onnx**（403MB 未进包）
- Git LFS：`git lfs ls-files` 确认 det/rec 为 LFS 指针（oid 与 manifest 一致）；`git check-ignore` 确认 translation/model.onnx 仍被忽略；`git rev-list --objects HEAD | grep translation/model.onnx` 零命中
- bundle.resources 用 `ocr/**/*`（tauri-build 2.6.3 的 glob 行为，`ocr/**` 会因空匹配报构建错误），已在提交信息与 README 说明

### 验收标准代码层对照
1. ✅ 构建侧达成（见上）；「构建全程断网」未在断网环境复验（NSIS 工具链本机已缓存）——手工验证项 1
2. ⏳ 全新安装到 `D:\VTrans` 的运行时行为（OCR 离线开箱即用、`%APPDATA%`/`%LOCALAPPDATA%` 无数据）为 GUI/安装冒烟——手工验证项 2
3. ⏳ 设置页下载全流程（进度/取消/sha256/切 local 离线翻译/重下/删除）：前后端契约已两端实现并各自测试通过，端到端冒烟——手工验证项 3。**注意：manifest 的 `download_url` 为占位直链（GitHub Releases v0.1.0），发布流程必须回填最终 URL 与 sha256 后下载功能才真实可用**
4. ⏳ 删除/篡改 `data/models` 重启自恢复：`ensure_data_models` 幂等/自恢复有单测（首次复制/二次跳过/删除后重拷/损坏后重拷/optional 不复制），运行时冒烟——手工验证项 4
5. ✅ 代码层：`DpapiFileStore`（DPAPI 加密、原子写、无明文断言测试）+ 首启 `migrate_windows_to_dpapi`（逐条迁移+删除）+ 三级回退链；「凭据管理器无 VTrans 条目」为运行时冒烟——手工验证项 5
6. ✅ `cargo test --workspace` / `cargo clippy --workspace --all-targets` / `pnpm test` 全绿；新增单测覆盖 optional 语义、`ensure_data_models` 幂等/自恢复、下载校验失败回滚（app 侧 `model_download`/`model_setup` 单测 + models 侧 5 个 optional 单测）

## 契约核对
- 5 个新命令全部注册 `invoke_handler` 且无参数：`download_translation_model` / `cancel_translation_model_download` / `delete_translation_model` / `get_model_status` / `retry_model_setup`；前端 `tauri.ts` 命令名一字不差（mock invoke 契约测试双向断言）
- 事件 `model_download_progress`：payload `{ bytes: u64, total: u64, fraction: f32 }` snake_case，前后端类型一致；500ms/1MiB 节流、完成必发 1.0
- 冻结契约（vtrans-core）：零改动 ✅
- 既有链路回归：单次/实时/多框翻译、provider 切换、托盘、热键均未触碰（10-app 仅新增文件与增量修改；workspace 全量测试通过）

## 遗留问题

1. **手工验证项（需用户执行，GUI/安装冒烟）**：见各 crate README 手工验证项 17-20（vtrans-app）与 src/README：全新安装、下载全流程、自恢复、凭据迁移、断网构建
2. **发布流程回填**：`download_url` 占位直链与 sha256 由发布流程在 v0.1.0 制品就绪后回填（PLAN「用户已确认决策」1）；回填前设置页下载会 404，属预期
3. **环境注意**：本机代理（127.0.0.1:7897 + `all_proxy` 等环境变量）会让本地服务器集成测试 502；CI/开发者跑 workspace 测试时需清空代理变量
4. **既有非本功能警告**：`deepl.rs` items-after-test-module（clippy 1 条，建议后续顺手清理）
5. **历史文档缺口（DOCSYNC 记录，超出本功能范围）**：ARCHITECTURE §6.4 命令清单缺 `set_api_key`/`set_provider_credentials`/`get_app_config`/两个外观命令；10-app.md 内部文件树未列 overlay/tray/window_visibility/window_exclusion/debug_frame；DEVELOPMENT.md §8 `VTRANS_API_KEY` 行未验证实现状态
6. **断网构建未复验**：NSIS/WiX 工具链已缓存，未在断网环境验证；CI 若冷启动需先联网取工具

## 结论
- [x] 功能已整合（6 个分支按依赖顺序合并 main），workspace 与前端门禁全绿，验收标准代码层全部满足
- [ ] 存在遗留问题，需跟踪：手工验证项 1-5 由用户执行；发布流程回填 download_url/sha256（负责人：发布流程）
