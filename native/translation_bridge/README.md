# translation_bridge（C++ 原生翻译桥）

为 `vtrans-translation` 提供的 C++17 统一 C ABI 桥，封装三个引擎：

| 方向 | 引擎 | 版本（锁定） | 许可证 |
|------|------|--------------|--------|
| en → zh | [Bergamot Translator](https://github.com/browsermt/bergamot-translator) | `v0.4.5` | MPL-2.0 |
| ja → zh | [CTranslate2](https://github.com/OpenNMT/CTranslate2) | `4.8.1` | MIT |
| ja/zh 子词 | [SentencePiece](https://github.com/google/sentencepiece) | 固定 revision（见下方） | Apache-2.0 |

许可证登记见仓库根 `licenses/NOTICE.md`（决策 B6）。本桥只做封装：不修改任何引擎源码、不随本仓库分发引擎二进制。

## 1. 职责与边界

做：

- 加载并持有两个引擎（Bergamot en→zh、CTranslate2 INT8 ja→zh）与各自 SentencePiece 词表，生命周期与进程一致（指南 §15）
- 按 `"en"` / `"ja"` 路由翻译，UTF-8 出入（指南 §20，禁止 ACP/ANSI 中转）
- 质量档位映射：Bergamot beam fast 1 / balanced 2；CTranslate2 beam fast 1 / balanced 4（指南 §7/§25）
- 线程上限：Bergamot `cpu-threads=2`；CTranslate2 intra 2 / inter 1；引擎互斥锁串行化全部调用（B5）

不做：

- 不管理 manifest / SHA-256（`vtrans-models` 负责）
- 不下载或转换模型（`scripts/translation/` 负责）
- 不暴露 C++ 类型给 Rust（保持 C ABI 最小面）

## 2. 目录与文件

```text
native/translation_bridge/
├── CMakeLists.txt            # CMake 3.20+，输出 translation_bridge.dll
├── build.ps1                 # Windows 一键构建（需要预构建的引擎产物）
├── translation_bridge.h      # C ABI（四个核心函数 + version/set_quality 扩展）
├── translation_bridge.cpp    # C++17 实现
└── README.md
```

## 3. 构建环境（Windows）

- Visual Studio 2022/2026 Build Tools，含 "Desktop development with C++"（MSVC + Windows 10/11 SDK）
- CMake ≥ 3.20
- 三个引擎的预构建产物（见第 4 节；引擎源码构建本身不在本脚本内）

## 4. 依赖构建（锁定版本）

### 4.1 Bergamot v0.4.5

```powershell
git clone --recursive https://github.com/browsermt/bergamot-translator.git
git -C bergamot-translator checkout v0.4.5
git -C bergamot-translator submodule update --init --recursive
cmake -S bergamot-translator -B build-bergamot -DCMAKE_BUILD_TYPE=Release
cmake --build build-bergamot --config Release
```

将产物整理为 `<DepsRoot>/bergamot/`（含 `include/` 与 `lib/`）。Bergamot 依赖 marian 等子模块，`--recursive` 不可省略。

### 4.2 CTranslate2 4.8.1

```powershell
git clone https://github.com/OpenNMT/CTranslate2.git
git -C CTranslate2 checkout v4.8.1
git -C CTranslate2 submodule update --init --recursive
cmake -S CTranslate2 -B build-ct2 -DCMAKE_BUILD_TYPE=Release -DWITH_MKL=OFF -DOPENMP_RUNTIME=NONE
cmake --build build-ct2 --config Release
```

将产物整理为 `<DepsRoot>/ctranslate2/`（含 `include/` 与 `lib/`）。注意本桥使用 `Model::load(..., ComputeType::INT8)` 与 `translate_batch`，需要 CTranslate2 以静态/动态库形式暴露 C++ API（默认构建即满足）。

### 4.3 SentencePiece（固定 revision）

```powershell
git clone https://github.com/google/sentencepiece.git
git -C sentencepiece checkout <固定 revision>   # 见下方「锁定说明」
cmake -S sentencepiece -B build-spm -DCMAKE_BUILD_TYPE=Release -DSPM_ENABLE_SHARED=ON
cmake --build build-spm --config Release
```

将产物整理为 `<DepsRoot>/sentencepiece/`（含 `include/` 与 `lib/`）。

> 锁定说明：SentencePiece 无稳定 release 标签，接入指南与 08 脚本通过 Python 侧 `sentencepiece` 包版本锁定；C++ 侧同样必须固定 commit。集成时把实际使用的 commit 回填到本文件并重跑固定回归集（指南 §27）。

## 5. 构建本桥

```powershell
.\native\translation_bridge\build.ps1 -DepsRoot D:\deps\translation
```

产物复制到 `src-tauri/resources/native/translation_bridge.dll`（Rust 侧默认查找路径；`tauri.conf.json` 的 `bundle.resources` 声明由 10 任务维护）。

可选参数：`-OutputDir`、`-BuildDir`。CMake 变量等价形式：

```powershell
cmake -S native\translation_bridge -B native\translation_bridge\build `
  -DCMAKE_BUILD_TYPE=Release `
  -DBERGAMOT_INCLUDE_DIRS=D:\deps\translation\bergamot\include `
  -DBERGAMOT_LIBRARIES="D:\deps\translation\bergamot\lib\bergamot.lib;..." `
  -DCTRANSLATE2_INCLUDE_DIRS=D:\deps\translation\ctranslate2\include `
  -DCTRANSLATE2_LIBRARIES="..." `
  -DSENTENCEPIECE_INCLUDE_DIRS=D:\deps\translation\sentencepiece\include `
  -DSENTENCEPIECE_LIBRARIES="..." `
  -DTRANSLATION_BRIDGE_OUTPUT_DIR=src-tauri\resources\native
cmake --build native\translation_bridge\build --config Release
```

## 6. C ABI 约定

| 函数 | 语义 |
|------|------|
| `translation_bridge_version()` | 返回 ABI 版本（当前 1）；Rust 侧加载时校验，不匹配报 `ModelLoad` |
| `translation_create(enzh_dir, jazh_dir)` | 加载双引擎，失败返回 NULL；进程内常驻一个引擎（指南 §15） |
| `translation_set_quality(engine, "fast"\|"balanced")` | 切换质量档位；Bergamot 重建（beam 为构造期参数），CTranslate2 仅改运行时 beam |
| `translation_translate(engine, lang, input, &output)` | `lang` 为 `"en"` / `"ja"`；成功 0 且 `output` 需 `translation_free_string` 释放 |
| `translation_free_string(ptr)` / `translation_destroy(engine)` | 资源释放；`destroy(NULL)` 安全 |

错误码（指南 §21）：0 OK；1 invalid argument；2 unsupported language；3 model not loaded；4 tokenizer；5 inference；6 encoding；7 version mismatch。

## 7. 已知限制

- 原生推理为不可中断阻塞调用，取消语义退化为「调用前后检查」（决策 B3），由 Rust 侧实现
- `translation_set_quality` 在 Balanced 切换时会重建 Bergamot 模型（约 44 MB 权重重载）；只应在 Provider 组装期调用一次，不在热路径调用
- 引擎互斥锁串行化全部请求；batch 翻译（一次多句）尚未实现（指南 §17，登记为后续优化）
- 本桥源码按锁版本 API 编写；如引擎升级，需按指南 §27 重跑固定回归集
