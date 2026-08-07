# VTrans Git 工作流

本文档定义模块化开发的分支策略、合并流程和提交规范。

## 1. 分支模型

### 1.1 分支命名

| 分支类型 | 命名格式 | 示例 |
|---------|---------|------|
| 主分支 | `main` | `main` |
| 模块分支 | `feat/NN-module` | `feat/01-core` |
| 修复分支 | `fix/NN-description` | `fix/03-credential-leak` |
| 发布分支 | `release/vX.Y.Z` | `release/v0.1.0` |

NN 是模块编号（01-11），与 docs/modules/ 中的编号一致。

### 1.2 分支策略

```text
main (基础骨架 + 已合并模块)
  |
  +-- feat/01-core (Phase 0)
  |     已合并 -> 回到 main
  |
  +-- feat/02-config (Phase 1, 从 main 拉取)
  +-- feat/03-security (Phase 1, 从 main 拉取)
  +-- feat/06-text (Phase 1, 从 main 拉取)
  +-- feat/08-models (Phase 1, 从 main 拉取)
  |     各自合并 -> 回到 main
  |
  +-- feat/04-capture (Phase 2, 从 main 拉取)
  +-- feat/05-ocr (Phase 2, 从 main 拉取)
  +-- feat/07-translation (Phase 2, 从 main 拉取)
  |     各自合并 -> 回到 main
  |
  +-- feat/09-pipeline (Phase 3, 从 main 拉取)
  |     合并 -> 回到 main
  |
  +-- feat/10-app (Phase 4, 从 main 拉取)
  +-- feat/11-frontend (Phase 4, 从 main 拉取)
        各自合并 -> 回到 main
```

## 2. 开发流程

### 2.1 开始一个模块

```powershell
git checkout main
git pull origin main
git checkout -b feat/05-ocr
```

### 2.2 日常开发

```powershell
cargo test -p vtrans-ocr
git add .
git commit -m "feat(ocr): implement text detection model loading"
```

### 2.3 同步 main 更新

```powershell
git fetch origin
git rebase origin/main
git rebase --continue
```

### 2.4 完成模块

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all -- --check
git push origin feat/05-ocr
gh pr create --base main --head feat/05-ocr --title "feat(ocr): PP-OCR ONNX detection and recognition"
```

## 3. 提交规范

### 3.1 Commit Message 格式

```text
<type>(<scope>): <subject>
```

| type | 说明 |
|------|------|
| feat | 新功能 |
| fix | 修复 bug |
| test | 新增或修改测试 |
| docs | 文档变更 |
| refactor | 重构 |
| perf | 性能优化 |
| chore | 构建/工具变更 |

scope 取值：core, config, security, capture, ocr, text, translation, models, pipeline, app, frontend

示例：

```
feat(ocr): implement PaddleOCR detection model inference
fix(capture): correct DPI conversion for 150% scale
test(text): add fingerprint dedup test cases
docs(arch): update module dependency graph
```

## 4. PR 审查清单

合并前必须满足：

- [ ] cargo test -p <crate> 通过
- [ ] cargo clippy 零警告
- [ ] cargo fmt 零差异
- [ ] 公开 API 有 rustdoc 注释
- [ ] 错误路径有日志
- [ ] 无敏感数据出现在日志中
- [ ] unsafe 有 SAFETY 注释
- [ ] README.md 已更新
- [ ] 验收标准全部满足

## 5. 冲突解决

- 优先 rebase 而非 merge，保持线性历史
- 冲突时优先保留功能更完整的版本
- 接口定义冲突（如 vtrans-core 类型变更）需在 PR 中讨论
- 同一 Phase 的模块如修改了同一文件，通过沟通协调

## 6. .gitignore 要点

```gitignore
*.onnx
*.bin
src-tauri/resources/models/ocr/
src-tauri/resources/models/translation/
target/
dist/
node_modules/
.env
*.local
.idea/
.vscode/launch.json
*.log
logs/
```

## 7. 模型文件管理

- 模型文件（.onnx, .bin, 字典文件）不提交 Git
- manifest.json 提交到 src-tauri/resources/models/
- 下载脚本 scripts/ppocrv6/setup_ppocrv6.ps1 提交到 Git
- 如需 Git LFS，在 .gitattributes 中配置 *.onnx filter=lfs

## 8. 发布流程

1. 从 main 创建 release/vX.Y.Z 分支
2. 运行全量测试和 clippy
3. 更新版本号（Cargo.toml workspace.package.version）
4. 执行 cargo tauri build
5. 测试安装包
6. 合并 release 分支到 main 并打 tag
7. 生成 Release Notes
