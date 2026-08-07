# 英→中 / 日→中 轻量离线翻译接入指南

**目标：** 与 PP-OCRv6 Small 配套，在个人桌面软件中实现离线 `English → Simplified Chinese` 与 `Japanese → Simplified Chinese` 翻译。  
**硬约束：** 翻译模型总存储预算 ≤ 200 MB。  
**推荐方案：** Firefox/Bergamot `en→zh` + `shun89/opus-mt-ja-zh` 经 CTranslate2 INT8。  
**资料核对日期：** 2026-08-07。

> 这套方案不再包含“中→英”，也不使用 M2M100。核心思路是：英中使用 Mozilla 为 Firefox 本地翻译优化的量化 Marian 模型；日中使用 6 层 MarianMT 直译模型并转为 CTranslate2 INT8。这样可以避免 `ja→en→zh` 的二次翻译损失，同时把总翻译模型预算控制在 200 MB 内。

---

## 1. 最终推荐

### 1.1 模型组合

| 方向 | 模型 | 运行时 | 体积规划 |
|---|---|---|---:|
| English → Chinese | Mozilla Firefox Translations `en-zh` Release / base-memory | Bergamot Translator | 模型权重约 43.85 MB；完整语言包建议预算 50–65 MB |
| Japanese → Chinese | `shun89/opus-mt-ja-zh` | CTranslate2 INT8 | 预计约 85–110 MB，必须转换后实测 |
| **翻译模型总预算** | | | **建议目标 140–175 MB；硬门槛 200 MB** |

Mozilla 当前模型注册表（2026-08-07 生成）中，`en-zh` 的 Release `base-memory` 模型：

- `releaseStatus`: `Release`
- `architecture`: `base-memory`
- 模型权重解压大小：`43,849,787 bytes`
- 参数量：`43,536,965`
- FLORES-200+ COMET22：`0.8628`
- 模型格式：`model.enzh.intgemm.alphas.bin.gz`
- 使用独立 source/target SentencePiece 词表。

日中模型 `shun89/opus-mt-ja-zh`：

- Hugging Face 仓库总大小约 314 MB；
- FP32 `pytorch_model.bin` 约 310 MB；
- MarianMT encoder 6 层、decoder 6 层；
- `d_model = 512`；
- FFN = 2048；
- vocab size = 65,001；
- 默认 `num_beams = 4`；
- Apache-2.0；
- source/target SentencePiece 文件各约 1.31 MB。

CTranslate2 官方支持 Transformers 的 `MarianMT`，并支持在转换阶段保存 INT8 权重。官方示例中，典型 base Transformer 从 FP32 364 MB 降至 INT8 约 100 MB，因此本指南对日中包按 85–110 MB 做规划，但**最终必须用你转换后的目录大小作为发布依据**。

---

## 3. 与 PP-OCRv6 的完整软件架构

推荐的数据流：

```text
Screenshot / Image
        |
        v
PP-OCRv6 Small DET
        |
        v
PP-OCRv6 Small REC
        |
        v
OCR blocks / lines
        |
        v
Text merge + language routing
        |
        +-----------------------+
        |                       |
        v                       v
      English                 Japanese
        |                       |
        v                       v
Bergamot en-zh            CTranslate2 ja-zh
        |                       |
        +-----------+-----------+
                    |
                    v
              Chinese text
                    |
                    v
      UI overlay / clipboard / export
```

不要把每一个 OCR 检测框都立刻单独送进翻译模型。更好的流程是：

```text
OCR boxes
   ↓
reading-order sort
   ↓
same-line merge
   ↓
sentence / paragraph merge
   ↓
translation
```

上下文越完整，机器翻译通常越稳定。

---

## 5. English → Chinese：Firefox / Bergamot

### 5.1 不再从旧仓库固定下载

`mozilla/firefox-translations-models` 已于 2025-12-15 归档。

Mozilla 当前在：

```text
https://storage.googleapis.com/moz-fx-translations-data--303e-prod-translations-data/db/models.json
```

维护模型注册表。

因此产品构建流程应：

1. 拉取 `models.json`；
2. 查找 `models["en-zh"]`；
3. 优先选择 `releaseStatus == "Release"`；
4. 获取 `model`、`srcVocab`、`trgVocab`、`lexicalShortlist` 的 path；
5. 下载 `.gz`；
6. 校验哈希/大小；
7. 解压到发布目录；
8. 把实际版本信息写进 manifest。

starter 中的：

```text
tools/fetch_firefox_enzh.py
```

会按这个逻辑生成下载清单，并支持直接下载。

### 5.2 当前推荐的 en-zh Release 模型

当前注册表中的 Release 项是：

```text
architecture: base-memory
source: en
target: zh
uncompressed model bytes: 43,849,787
parameters: 43,536,965
```

使用的主要文件类型：

```text
model.enzh.intgemm.alphas.bin
srcvocab.enzh.spm
trgvocab.enzh.spm
lex.50.50.enzh.s2t.bin
```

注意：注册表中的文件通常是 `.gz`，Bergamot 最终加载的是解压后的二进制/词表。

### 5.3 Bergamot 运行参数

建议初始配置：

```yaml
beam-size: 1
normalize: 1.0
word-penalty: 0
max-length-break: 128
mini-batch-words: 1024
workspace: 128
max-length-factor: 2.0
skip-cost: true
cpu-threads: 0
quiet: true
quiet-translation: true
gemm-precision: int8shiftAlphaAll
alignment: soft
```

说明：

- `gemm-precision` 必须与 `.intgemm.alphas.bin` 模型匹配。
- `beam-size: 1` 是优先低延迟的默认值。
- 若你更在意质量，可以测试 beam 2，但不要默认提高到很大的 beam。
- `workspace` 和 `mini-batch-words` 应根据桌面内存和文本长度做基准测试。
- `cpu-threads: 0` 让引擎按默认逻辑选择线程；生产版本建议实测固定 1/2/4 线程。

### 5.4 Bergamot Native 构建

官方仓库：

```text
https://github.com/browsermt/bergamot-translator
```

当前资料核对时稳定 release 为 `v0.4.5`。

Native：

```bash
git clone --recursive https://github.com/browsermt/bergamot-translator.git
cd bergamot-translator
git checkout v0.4.5

cmake -S . -B build-native \
  -DCMAKE_BUILD_TYPE=Release

cmake --build build-native --config Release -j
```

Bergamot 提供 C++ API。Rust 项目建议通过一个非常薄的 C ABI/C++ wrapper 暴露：

```text
create_model()
translate_batch()
destroy_model()
```

而不是让 Rust 直接绑定大量 C++ 模板类型。

### 5.5 Bergamot WASM

如果你的 TypeScript 运行在：

- 浏览器；
- Electron renderer；
- WebView；
- Tauri frontend；  

可以使用 Bergamot 的 WASM 构建。

官方构建方向：

```bash
emcmake cmake -DCOMPILE_WASM=on ..
emmake make -j2
```

生成 JavaScript + WASM artifacts。

建议在 Web Worker 中运行翻译，不要阻塞 UI 主线程。

---

## 6. Japanese → Chinese：OPUS-MT Marian + CTranslate2 INT8

### 6.1 原模型

模型：

```text
shun89/opus-mt-ja-zh
```

下载：

```bash
hf download shun89/opus-mt-ja-zh \
  --local-dir models/source/opus-mt-ja-zh
```

或：

```python
from huggingface_hub import snapshot_download

snapshot_download(
    repo_id="shun89/opus-mt-ja-zh",
    local_dir="models/source/opus-mt-ja-zh",
)
```

不要把 FP32 `pytorch_model.bin` 直接随最终软件分发，否则仅日中就超过 300 MB。

### 6.2 转为 CTranslate2 INT8

准备环境：

```bash
python -m venv .venv
source .venv/bin/activate

python -m pip install --upgrade pip
python -m pip install \
  "ctranslate2==4.8.1" \
  "transformers>=4.50" \
  torch \
  sentencepiece \
  huggingface_hub
```

转换：

```bash
ct2-transformers-converter \
  --model shun89/opus-mt-ja-zh \
  --output_dir models/translation/ja-zh-ct2-int8 \
  --quantization int8 \
  --copy_files source.spm target.spm tokenizer_config.json \
               special_tokens_map.json vocab.json
```

完成后：

```bash
du -sh models/translation/ja-zh-ct2-int8
```

Windows PowerShell：

```powershell
(Get-ChildItem models/translation/ja-zh-ct2-int8 -Recurse |
  Measure-Object -Property Length -Sum).Sum / 1MB
```

### 6.3 为什么选 CTranslate2

CTranslate2：

- 官方支持 Transformers `MarianMT`；
- 官方支持 OPUS-MT / Marian；
- 支持 INT8；
- C++ 原生 runtime；
- x86-64 和 ARM64 CPU 均有优化；
- 解码器自带 beam search、cache 和 batch；
- 不需要自己在 ONNX Runtime 中手写 autoregressive decoder loop。

对于桌面软件，这一点很重要。

### 6.4 SentencePiece

CTranslate2 不负责把 Unicode 原文自动转成 SentencePiece token。

流程：

```text
Japanese UTF-8
     ↓
source.spm encode
     ↓
List<String> tokens
     ↓
CTranslate2 translate_batch
     ↓
target token strings
     ↓
target.spm decode
     ↓
Chinese UTF-8
```

必须保留与模型一致的：

```text
source.spm
target.spm
```

不要替换成其他 OPUS 模型的 SPM 文件。

---

## 7. 日中推荐解码参数

原模型配置中 `num_beams = 4`，因此“质量优先”的初始值可以从 beam 4 开始。

但 OCR 软件更强调响应速度，建议做两档：

### Fast

```json
{
  "beam_size": 1,
  "max_input_tokens": 256,
  "max_decoding_length": 256
}
```

### Balanced

```json
{
  "beam_size": 4,
  "max_input_tokens": 256,
  "max_decoding_length": 320
}
```

OCR 文本通常比文档翻译短，没必要默认允许 512 个生成 token。

建议 UI 提供：

```text
Translation quality:
[ Fast ] [ Balanced ]
```

而不要向普通用户暴露 beam、length penalty 等专业参数。

---

## 8. 语言路由

由于你的系统只翻译到中文，路由只需要区分：

```text
English
Japanese
```

最可靠的方式：

1. 如果软件知道当前 OCR 语言，直接复用 OCR language mode。
2. 如果用户选择“日文 OCR”，直接走 ja→zh。
3. 如果用户选择“英文 OCR”，直接走 en→zh。
4. Auto 模式再做 Unicode heuristic。

简单 heuristic：

```text
存在 Hiragana (3040–309F)
or Katakana (30A0–30FF)
or Halfwidth Katakana (FF65–FF9F)
    => Japanese

否则如果主要是 Latin letters
    => English
```

仅包含 Kanji 的短文本可能同时像中文/日文，因此不要把 Unicode heuristic 当成百分百准确的语言识别器。

对于游戏/漫画 OCR，建议让用户显式选择：

```text
OCR language: Japanese
Translation target: Chinese
```

准确性最好，也不需要额外语言识别模型。

---

## 9. OCR 文本如何送入翻译模型

### 9.1 不推荐逐框翻译

错误：

```text
box 1 -> translate
box 2 -> translate
box 3 -> translate
```

可能把：

```text
I don't
know
what happened.
```

拆成三次翻译，质量会明显下降。

### 9.2 推荐合并

```text
det boxes
   ↓
reading order
   ↓
line merge
   ↓
punctuation-aware sentence merge
   ↓
translation batch
```

### 9.3 适合 OCR 的 chunk 规则

建议：

```text
单 chunk <= 200~256 source tokens
优先在 。！？.!? 换行
其次在逗号/分号
最后才做强制 token cut
```

日文：

```text
。！？
```

英文：

```text
. ! ?
```

对于游戏对话，保留角色换行会比把整个屏幕拼成一个大段更自然。

---

## 10. Rust 接入总体设计

最推荐的产品形态是：

```text
Tauri / Rust backend
│
├── PP-OCRv6 Small
│     └── ONNX Runtime / ort
│
├── English -> Chinese
│     └── Bergamot native C++ wrapper
│
└── Japanese -> Chinese
      └── CTranslate2 C++ wrapper
```

TypeScript 前端只调用：

```text
ocr(image)
translate(text, sourceLanguage)
ocrAndTranslate(image, sourceLanguage)
```

这样前端不需要理解：

- SentencePiece；
- C++ runtime；
- beam search；
- model lifecycle；
- native thread pools。

---

## 11. Rust FFI 层建议

不要直接 bindgen 整个 Bergamot / CTranslate2 C++ API。

建议写统一的 C ABI：

```c
typedef void* TranslationEngine;

TranslationEngine translation_create(
    const char* enzh_model_dir,
    const char* jazh_model_dir
);

int translation_translate(
    TranslationEngine engine,
    const char* source_lang,
    const char* utf8_input,
    char** utf8_output
);

void translation_free_string(char* ptr);
void translation_destroy(TranslationEngine engine);
```

内部：

```text
TranslationEngineImpl
├── BergamotEnZh
├── CTranslate2JaZh
├── SentencePieceJa
└── SentencePieceZh
```

Rust：

```rust
#[link(name = "translation_bridge")]
unsafe extern "C" {
    fn translation_create(
        enzh_model_dir: *const c_char,
        jazh_model_dir: *const c_char,
    ) -> *mut c_void;

    fn translation_translate(
        engine: *mut c_void,
        source_lang: *const c_char,
        input: *const c_char,
        output: *mut *mut c_char,
    ) -> c_int;

    fn translation_free_string(ptr: *mut c_char);
    fn translation_destroy(engine: *mut c_void);
}
```

将 C++ 复杂度锁在 bridge 内，是跨 Windows/macOS/Linux 最容易维护的方式。

starter 中提供：

```text
native/translation_bridge.h
rust/src/ffi.rs
```

作为接口骨架。

---

## 12. TypeScript / Tauri

推荐：

```ts
type SourceLanguage = "en" | "ja";

interface TranslateRequest {
  sourceLanguage: SourceLanguage;
  text: string;
}

interface TranslateResult {
  text: string;
  sourceLanguage: SourceLanguage;
}
```

Tauri frontend：

```ts
import { invoke } from "@tauri-apps/api/core";

export async function translate(
  sourceLanguage: "en" | "ja",
  text: string
): Promise<string> {
  return invoke<string>("translate_text", {
    sourceLanguage,
    text,
  });
}
```

Rust backend 再调用 native translation bridge。

这样：

```text
TS frontend
    ↓ IPC
Rust
    ↓
Bergamot / CTranslate2
```

是本指南最推荐的桌面架构。

---

## 13. TypeScript / Electron

Electron 推荐放在：

```text
main process / native addon / sidecar
```

而不是 renderer。

结构：

```text
Renderer
  ↓ IPC
Electron Main
  ↓
Native translation bridge
  ↓
Bergamot + CTranslate2
```

原因：

- C++ 动态库不暴露给 renderer；
- 不影响 Chromium sandbox；
- 模型只加载一次；
- 多窗口共享一个 translation service；
- 更容易控制线程和内存。

---

## 14. 纯浏览器场景

如果你的 TS 项目是纯浏览器网页：

### en→zh

Bergamot WASM 非常适合。

### ja→zh

CTranslate2 当前主要提供 C++ / Python runtime，不是一个直接面向浏览器 WASM 的标准方案。

因此纯浏览器版本有两个选择：

1. 自己把日中 Marian 导出为 ONNX/Transformers.js，并重新验证模型大小；
2. 把 ja→zh 放到本地桌面 native service。

所以本指南的 **≤200 MB 最优方案主要面向桌面/Tauri/Electron**。

如果你明确要求：

```text
Browser-only
No native code
```

应重新选择日中模型部署格式。

---

## 15. Session / Model 生命周期

### 错误

```text
每次翻译：
load model
translate
destroy
```

### 正确

```text
App startup
  ↓
load enzh
load jazh
  ↓
keep engines alive
  ↓
translate many requests
  ↓
App shutdown
```

模型加载应放在：

```text
TranslationService::new()
```

而不是：

```text
translate()
```

中。

---

## 16. 并发建议

OCR 与翻译都会占 CPU。

不要：

```text
ORT det threads = all cores
ORT rec threads = all cores
Bergamot threads = all cores
CTranslate2 threads = all cores
```

否则非常容易 CPU oversubscription。

一个 8 核 CPU 的初始测试组合可以是：

```text
PP-OCR det intra threads      2
PP-OCR rec intra threads      2
Bergamot                     2
CTranslate2 intra threads    2
CTranslate2 inter threads    1
```

再通过 benchmark 调整。

实际最佳值取决于：

- Intel/AMD/Apple Silicon；
- OCR 图片尺寸；
- 一次翻译多少句；
- 是否同时做截图、UI 和音频处理。

---

## 17. Batch

### OCR

检测通常 batch 1。

识别可以 batch 多个文字行。

### 翻译

翻译也建议 batch：

```text
["first sentence", "second sentence", "third sentence"]
```

而不是串行逐句调用。

CTranslate2 的 `translate_batch` 原生支持 batch。

Bergamot 同样适合一次传入多个文本项。

对于屏幕 OCR：

```text
一帧 OCR 完成
  ↓
合并成 N 个翻译 chunk
  ↓
一次 batch translation
```

通常比 N 次模型调用更合理。

---

## 18. 缓存

屏幕 OCR 软件非常适合翻译缓存。

推荐：

```text
cache key =
  model_version
  + source_language
  + normalized_source_text
```

例如：

```text
SHA256("ja-zh-v1\0ja\0こんにちは")
```

LRU：

```text
1,000 ~ 10,000 entries
```

非常常见。

游戏 UI、菜单、技能名和重复对白经常重复出现，缓存可以明显降低延迟。

---

## 19. OCR 文本规范化

翻译前可以做轻量清理：

1. 去除首尾多余空格。
2. 修复 OCR 产生的连续空白。
3. 保留日文标点。
4. 保留英文 apostrophe：
   - `don't`
   - `I'm`
5. 不要随意 lower-case 英文。
6. 不要把日文全角符号粗暴替换成英文符号。
7. 不要在翻译前把所有换行删除。

可以将：

```text
This   is
a test.
```

轻量合并为：

```text
This is a test.
```

但漫画对白：

```text
やめて！
来ないで！
```

最好仍保留为两个句子或两个 chunk。

---

## 20. 日中 tokenizer 注意事项

`shun89/opus-mt-ja-zh` 有独立：

```text
source.spm
target.spm
```

不要假设 source 和 target tokenizer 相同。

在 native bridge 中建议：

```text
SentencePieceProcessor ja_sp
SentencePieceProcessor zh_sp
```

翻译：

```text
ja_sp.Encode(input)
    ↓
CTranslate2
    ↓
zh_sp.Decode(output tokens)
```

必须使用 UTF-8。

Windows 下不要把文本通过系统 ACP/ANSI API 中转。

---

## 21. 错误处理

建议统一错误码：

```text
0   OK
1   invalid argument
2   unsupported language
3   model not loaded
4   tokenizer failure
5   inference failure
6   output encoding failure
7   model version mismatch
```

Rust 外层映射到：

```rust
enum TranslationError {
    InvalidArgument,
    UnsupportedLanguage,
    ModelNotLoaded,
    Tokenizer,
    Inference,
    Encoding,
    VersionMismatch,
}
```

TS 层只接收稳定的业务错误，不要把底层 C++ exception 字符串直接暴露到 UI。

---

## 22. 模型 manifest

建议每个发布语言包都带：

```json
{
  "id": "ja-zh",
  "source": "ja",
  "target": "zh-Hans",
  "engine": "ctranslate2",
  "quantization": "int8",
  "model_source": "shun89/opus-mt-ja-zh",
  "model_revision": "<commit>",
  "converted_with": "ctranslate2 4.8.1",
  "bytes": 0,
  "sha256": "<fill>",
  "source_spm_sha256": "<fill>",
  "target_spm_sha256": "<fill>"
}
```

英中：

```json
{
  "id": "en-zh",
  "source": "en",
  "target": "zh-Hans",
  "engine": "bergamot",
  "registry_generated": "2026-08-07T00:43:32Z",
  "architecture": "base-memory",
  "release_status": "Release",
  "model_uncompressed_bytes": 43849787,
  "model_sha256": "<fill>",
  "package_bytes": 0
}
```

应用启动时校验 manifest，避免：

```text
模型 A + tokenizer B
```

这种最难排查的错误。

---

## 23. 模型下载策略

不建议安装包自动“永远拉最新版”。

推荐：

```text
Build/Release pipeline
    ↓
resolve registry/model revision
    ↓
download
    ↓
hash
    ↓
test
    ↓
freeze manifest
    ↓
ship
```

软件运行时只读取冻结好的 manifest。

如果以后要支持模型在线更新：

```text
model catalog
version
sha256
minimum app version
download URL
```

都应该由你自己的更新清单控制。

---

## 24. 商业发布与许可证

### CTranslate2

官方项目许可证：MIT。

### Bergamot Translator

官方项目许可证：MPL-2.0。

### `shun89/opus-mt-ja-zh`

Hugging Face 模型卡标记：Apache-2.0。

### Mozilla translation model

不要只因为 Bergamot 引擎是 MPL-2.0，就自动推断每一个训练模型文件的商业再分发条件完全等同于引擎许可证。

正式商业发布前建议：

1. 保存模型 registry 记录；
2. 保存模型来源和训练/数据说明；
3. 检查模型 artifact 的许可说明；
4. 检查依赖许可证；
5. 在 `licenses/` 中放置必要 notices；
6. 对最终要随安装包分发的具体模型做一次法律/合规复核。

本指南是工程接入说明，不构成法律意见。

---

## 25. 性能参数建议

### English → Chinese / Bergamot

初始：

```text
beam-size            1
cpu threads          1/2/4 benchmark
batch                1~16 chunks
```

### Japanese → Chinese / CTranslate2

Fast：

```text
beam_size            1
inter_threads        1
intra_threads        2
max_input_tokens     256
```

Balanced：

```text
beam_size            4
inter_threads        1
intra_threads        2
max_input_tokens     256
```

不要先追求最高 beam。

对于 OCR 翻译软件，用户感知更强的是：

```text
截图 -> 首屏翻译显示 latency
```

而不是离线 BLEU 提升极小的一点点。

---

## 26. Benchmark 必须记录什么

固定 100–500 条真实 OCR 文本，至少包含：

### English

- UI；
- 技术文本；
- 对话；
- 数字；
- 缩写；
- apostrophe；
- OCR 少量错字。

### Japanese

- 普通说明；
- 游戏对白；
- 敬语；
- 片假名外来语；
- 人名；
- 汉字 + 假名混排；
- 省略主语；
- OCR 少量错字。

记录：

```text
cold model load
warm first translation
average latency
P50
P95
P99
peak RSS
model bytes
translation failures
manual quality score
```

---

## 27. 质量回归

每次以下变化都要重新回归：

- CTranslate2 版本；
- Bergamot 版本；
- 模型 revision；
- INT8 转换；
- beam；
- tokenizer；
- chunking；
- OCR 文本合并策略。

建议保存：

```json
{
  "source": "今日はもう帰るね。",
  "expected_contains": ["今天", "回"],
  "source_language": "ja"
}
```

不要只做 exact string equality，因为机器翻译可能有多个正确说法。

---

## 28. 200 MB CI 门禁

starter 的：

```text
tools/audit_model_sizes.py
```

可以用于 CI：

```bash
python tools/audit_model_sizes.py \
  models/translation \
  --max-mb 200
```

超过预算直接返回非 0。

推荐进一步分项：

```text
en-zh <= 65 MB
ja-zh <= 110 MB
translation <= 175 MB target
translation <= 200 MB hard
```

---

## 29. 推荐项目目录

```text
app/
├── models/
│   ├── ocr/
│   │   ├── det.onnx
│   │   └── rec.onnx
│   │
│   └── translation/
│       ├── en-zh/
│       │   ├── model.enzh.intgemm.alphas.bin
│       │   ├── srcvocab.enzh.spm
│       │   ├── trgvocab.enzh.spm
│       │   ├── lex.50.50.enzh.s2t.bin
│       │   └── manifest.json
│       │
│       └── ja-zh/
│           ├── model.bin
│           ├── config.json
│           ├── source_vocabulary.json
│           ├── target_vocabulary.json
│           ├── source.spm
│           ├── target.spm
│           └── manifest.json
│
├── native/
│   └── translation_bridge/
│
├── src-tauri/
│
└── frontend/
```

---

## 30. 推荐 API

Rust：

```rust
pub enum SourceLanguage {
    English,
    Japanese,
}

pub struct TranslationResult {
    pub text: String,
}

pub trait TranslationService {
    fn translate(
        &self,
        source: SourceLanguage,
        text: &str,
    ) -> Result<TranslationResult, TranslationError>;
}
```

TS：

```ts
export type SourceLanguage = "en" | "ja";

export interface TranslationResult {
  text: string;
}

export interface TranslationService {
  translate(
    sourceLanguage: SourceLanguage,
    text: string
  ): Promise<TranslationResult>;
}
```

上层 OCR pipeline 不应该知道底层到底是 Bergamot 还是 CTranslate2。

---

## 31. 推荐实施顺序

1. 保留已经跑通的 PP-OCRv6 Small。
2. 下载当前 Firefox `en-zh` Release 模型。
3. 单独跑通 Bergamot 英中。
4. 下载 `shun89/opus-mt-ja-zh`。
5. 转 CTranslate2 INT8。
6. 实测 ja-zh 目录大小。
7. 跑通 Python/C++ CTranslate2 baseline。
8. 写统一 native bridge。
9. Rust 只绑定统一 bridge。
10. Tauri/TS 通过 IPC 调 Rust。
11. 再接 OCR 文本合并。
12. 做 100–500 条真实截图回归。
13. 设置 200 MB CI 模型门禁。
14. 冻结模型 revision、runtime version 和 SHA-256。

---

## 32. 推荐配置文件

```json
{
  "translation": {
    "target": "zh-Hans",
    "model_budget_mb": 200,
    "target_budget_mb": 175,
    "english": {
      "engine": "bergamot",
      "source": "en",
      "beam_size": 1
    },
    "japanese": {
      "engine": "ctranslate2",
      "source": "ja",
      "quantization": "int8",
      "beam_size_fast": 1,
      "beam_size_balanced": 4,
      "max_input_tokens": 256
    }
  }
}
```

---

## 33. 关键工程结论

如果你的目标是：

```text
offline
CPU
Rust / TypeScript
English -> Chinese
Japanese -> Chinese
translation models <= 200 MB
```

推荐：

```text
en -> zh:
Mozilla Firefox Translations
Bergamot intgemm

ja -> zh:
shun89/opus-mt-ja-zh
CTranslate2 INT8
```

产品架构优先：

```text
Tauri
TS UI
  ↓
Rust backend
  ↓
C/C++ translation bridge
  ├── Bergamot
  └── CTranslate2
```

不要为追求“全部使用 ONNX Runtime”而重新实现 Marian 自回归 decoder。对于这个 200 MB 的硬约束，专用翻译 runtime 能明显降低模型占用和工程风险。

---

## 34. 资料来源

### Mozilla / Bergamot

- Mozilla Translations  
  https://github.com/mozilla/translations

- Current Mozilla model registry  
  https://storage.googleapis.com/moz-fx-translations-data--303e-prod-translations-data/db/models.json

- Firefox released models dashboard  
  https://mozilla.github.io/translations/firefox-models/

- Bergamot Translator  
  https://github.com/browsermt/bergamot-translator

- Bergamot releases  
  https://github.com/browsermt/bergamot-translator/releases

- Archived Firefox Translations Models repo  
  https://github.com/mozilla/firefox-translations-models

### Japanese → Chinese

- shun89/opus-mt-ja-zh  
  https://huggingface.co/shun89/opus-mt-ja-zh

- config.json  
  https://huggingface.co/shun89/opus-mt-ja-zh/blob/main/config.json

### CTranslate2

- CTranslate2  
  https://github.com/OpenNMT/CTranslate2

- Documentation  
  https://opennmt.net/CTranslate2/

- Model conversion  
  https://opennmt.net/CTranslate2/conversion.html

- Quantization  
  https://opennmt.net/CTranslate2/quantization.html

- Transformers / MarianMT support  
  https://opennmt.net/CTranslate2/guides/transformers.html

- Translation C++ API  
  https://opennmt.net/CTranslate2/translation.html

- PyPI  
  https://pypi.org/project/ctranslate2/

---

## 35. 版本说明

本文核对日期：**2026-08-07**。

核对时：

```text
CTranslate2 PyPI latest: 4.8.1
Bergamot stable release: v0.4.5
Mozilla model registry generated:
2026-08-07T00:43:32Z
```

生产软件必须固定：

```text
model revision
model hash
Bergamot version
CTranslate2 version
SentencePiece version / implementation
build toolchain
```

不要让生产构建自动跟随 `latest`。
