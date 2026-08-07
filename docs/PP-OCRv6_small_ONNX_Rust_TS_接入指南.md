# PP-OCRv6 Small ONNX 接入指南

**适用目标：** 在个人软件中使用 PP-OCRv6 Small 完成通用文字检测与识别，并接入 Rust、TypeScript/Node.js 或浏览器项目。  
**资料核对日期：** 2026-08-07  
**推荐模型组合：** `PP-OCRv6_small_det` + `PP-OCRv6_small_rec`

> 结论先行：Small 是 PP-OCRv6 官方面向移动端和资源受限应用的“精度/速度均衡档”。完整 OCR 不是单个模型，而是“检测 → 文本框排序/裁剪 → 识别 → CTC 解码”的流水线。生产运行时不需要 PaddlePaddle；Paddle/PaddleX 只在模型下载、转换和验模阶段使用。

---

## 1. 交付内容与适用范围

本指南覆盖：

1. PP-OCRv6 Small 检测与识别模型的选择、下载和 ONNX 转换。
2. 检测模型的输入预处理、DB 后处理和坐标还原。
3. 识别模型的裁剪、缩放、归一化、字典装载和 CTC 解码。
4. Rust 使用 `ort` crate 的接入方式。
5. TypeScript 在 Node.js 使用 `onnxruntime-node`、在浏览器使用 `onnxruntime-web` 的接入方式。
6. 参数调优、并发、模型打包、故障排查与验收清单。
7. 附带的 starter 代码包：模型下载/转换脚本、ONNX 检查脚本、Python 基准实现、Rust 识别示例和 TypeScript 识别示例。

本指南重点是 **通用场景 OCR**。若你的输入已经是单行文字裁剪图，可以跳过检测模型，只使用 `PP-OCRv6_small_rec`。

---

## 2. 模型选择

### 2.1 PP-OCRv6 的三个档位

| 档位 | 适用场景 | 特点 |
|---|---|---|
| Medium | 服务器、高精度离线处理 | 精度最高，模型与计算量较大 |
| Small | 桌面软件、移动端、个人应用 | 推荐默认选择，精度和速度均衡 |
| Tiny | IoT、低功耗边缘设备、极小包体 | 最快最小，但精度下降更明显 |

官方资料中，Small 检测模型约 9.6 MB，Small 识别模型约 20.4 MB。模型文件转换为 ONNX 后大小可能略有变化。

### 2.2 推荐文件

- 检测模型：`PP-OCRv6_small_det_infer.tar`
- 识别模型：`PP-OCRv6_small_rec_infer.tar`
- 字符字典：优先读取模型包内 `inference.yml` 的 `PostProcess.character_dict`；若包内没有嵌入字符表，再使用官方 `ppocrv6_dict.txt`。

官方下载地址：

```text
https://paddle-model-ecology.bj.bcebos.com/paddlex/official_inference_model/paddle3.0.0/PP-OCRv6_small_det_infer.tar
https://paddle-model-ecology.bj.bcebos.com/paddlex/official_inference_model/paddle3.0.0/PP-OCRv6_small_rec_infer.tar
https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/main/ppocr/utils/dict/ppocrv6_dict.txt
```

### 2.3 “Small” 的完整含义

在 OCR 流水线中，“Small”通常意味着同时选用：

```text
PP-OCRv6_small_det  # 找出文字区域
PP-OCRv6_small_rec  # 识别每一个文字区域
```

不要把检测模型和识别模型互换。两者输入、输出和后处理完全不同。

---

## 3. 最终软件架构

推荐把 OCR 代码拆成以下模块：

```text
ImageDecoder
    ↓ BGR/RGB 像素
DetectorPreprocessor
    ↓ float32 NCHW
ONNX Detector
    ↓ probability map [N,1,H,W]
DBPostprocessor
    ↓ quadrilateral boxes
BoxSorter + PerspectiveCrop
    ↓ text-line images
RecognizerPreprocessor
    ↓ float32 NCHW [N,3,48,W]
ONNX Recognizer
    ↓ logits/probabilities [N,T,C]
CTCDecoder
    ↓ text + confidence
ResultFilter / JSON output
```

建议对外暴露统一结果：

```json
{
  "text": "示例文本",
  "score": 0.9821,
  "points": [[12, 30], [220, 28], [222, 62], [14, 65]]
}
```

---

## 4. 环境分层

### 4.1 模型准备环境（只在开发机使用）

推荐：

- Python 3.10 或 3.11
- PaddlePaddle 3.0+
- PaddleX 与 Paddle2ONNX 插件
- `onnx`、`onnxruntime`、`opencv-python-headless`、`numpy`
- Linux/macOS/Windows 均可；Windows 转换遇到依赖问题时可使用 WSL2

示例：

```bash
python -m venv .venv
# Linux/macOS
source .venv/bin/activate
# Windows PowerShell
# .venv\Scripts\Activate.ps1

python -m pip install --upgrade pip
python -m pip install paddlepaddle
python -m pip install "paddlex[ocr]"
paddlex --install paddle2onnx

python -m pip install onnx onnxruntime numpy opencv-python-headless pyclipper pyyaml
```

若只做 CPU 转换和验模，不需要 CUDA。

### 4.2 最终 Rust 运行环境

最低依赖：

- Rust stable
- `ort`：ONNX Runtime Rust 社区绑定
- `image` 或 `opencv`：图像读取与变换
- `ndarray`：NCHW 张量
- DB 后处理需要轮廓、旋转矩形、透视变换和多边形外扩，推荐 `opencv` + Clipper 类库

当前资料核对时，`ort` 最新文档版本为 `2.0.0-rc.13`，其默认特性包含 ONNX Runtime 二进制下载与 `ndarray` 支持。发布时应使用 `Cargo.lock` 固定版本。

### 4.3 最终 TypeScript 运行环境

Node.js：

```bash
npm install onnxruntime-node sharp
npm install -D typescript tsx @types/node
```

浏览器：

```bash
npm install onnxruntime-web
```

资料核对时，`onnxruntime-node` / `onnxruntime-web` 的 npm 最新版本为 1.27.0。示例工程已固定该版本，升级后应重新做模型回归测试。

### 4.4 GPU 不是必需条件

Small 模型在 CPU 上即可运行。优先完成 CPU 基准，再决定是否增加 GPU：

- Node.js 官方预编译包：CPU 覆盖主流 Windows/Linux/macOS；CUDA 预编译包的平台范围更窄。
- 浏览器：WASM 最通用；WebGPU 性能潜力更高，但兼容性和行为需要单独验收。
- Rust：通过 `ort` 的 execution provider 特性接入 CUDA、DirectML、CoreML、OpenVINO 等；具体可用性取决于平台与 ORT 动态库。

---

## 5. 下载并转换 ONNX

### 5.1 下载模型

```bash
mkdir -p models/paddle models/onnx

curl -L \
  -o models/paddle/PP-OCRv6_small_det_infer.tar \
  "https://paddle-model-ecology.bj.bcebos.com/paddlex/official_inference_model/paddle3.0.0/PP-OCRv6_small_det_infer.tar"

curl -L \
  -o models/paddle/PP-OCRv6_small_rec_infer.tar \
  "https://paddle-model-ecology.bj.bcebos.com/paddlex/official_inference_model/paddle3.0.0/PP-OCRv6_small_rec_infer.tar"

tar -xf models/paddle/PP-OCRv6_small_det_infer.tar -C models/paddle
tar -xf models/paddle/PP-OCRv6_small_rec_infer.tar -C models/paddle
```

Windows 可用 7-Zip 解压。

### 5.2 转换命令

对检测和识别模型分别转换：

```bash
paddlex \
  --paddle2onnx \
  --paddle_model_dir models/paddle/PP-OCRv6_small_det_infer \
  --onnx_model_dir models/onnx/det \
  --opset_version 7

paddlex \
  --paddle2onnx \
  --paddle_model_dir models/paddle/PP-OCRv6_small_rec_infer \
  --onnx_model_dir models/onnx/rec \
  --opset_version 7
```

PaddleX 文档说明：`opset_version` 默认值是 7；若低 opset 无法转换，插件可能自动选择更高版本。不要在业务代码中假定 opset，验模脚本应读取实际模型。

转换后常见文件名为 `model.onnx`，但不同插件版本可能采用其他名称。请以目录中真实 `.onnx` 文件为准。

### 5.3 立即检查模型

```bash
python tools/inspect_onnx.py models/onnx/det/model.onnx
python tools/inspect_onnx.py models/onnx/rec/model.onnx
```

必须记录：

- 输入节点名称
- 输入 dtype
- 输入维度
- 输出节点名称
- 输出维度
- opset

**禁止把节点名硬编码成 `x`、`input` 或 `output`。** Rust/TS 应从 session metadata 读取真实名称。

### 5.4 验证原则

1. `onnx.checker.check_model()` 通过。
2. ONNX Runtime 能创建 session。
3. 检测输出通常是 `[1, 1, H, W]`。
4. 识别输出通常是 `[N, T, C]`。
5. 识别输出的 `C` 必须与字符表长度相符。
6. 使用同一张测试图，Python 基准输出应与 Rust/TS 结果近似一致。

---

## 6. 检测模型参数

### 6.1 官方结构和参数

`PP-OCRv6_small_det`：

| 项目 | 值 |
|---|---|
| 算法 | DB |
| Backbone | PPLCNetV4 Small |
| Neck | RepLKFPN |
| Neck 输出通道 | 96 |
| 大核尺寸 | 7 |
| Head | DBHead |
| DB `k` | 50 |
| 训练/静态图参考形状 | `[3, 640, 640]` |
| `thresh` | `0.2` |
| `box_thresh` | `0.45` |
| `max_candidates` | `3000` |
| `unclip_ratio` | `1.4` |

这些值来自 PP-OCRv6 Small 的模型配置，不应被旧版本通用示例中的默认值覆盖。

### 6.2 输入张量

常见形式：

```text
dtype: float32
layout: NCHW
shape: [1, 3, H, W]
```

若 ONNX metadata 显示固定 `[1,3,640,640]`，使用固定 640×640 直接缩放，并保存两个独立比例：

```text
ratio_h = 640 / original_height
ratio_w = 640 / original_width
```

官方固定形状路径是直接 resize，不是 letterbox。后处理时分别用 `ratio_h`、`ratio_w` 恢复坐标。

若 ONNX 输入是动态 H/W，建议完全复现模型包 `inference.yml` 中的 `DetResizeForTest`。通常要求目标 H/W 至少为 32，且为 32 的倍数。

### 6.3 颜色顺序与归一化

官方配置使用 `DecodeImage: BGR`，之后：

```text
pixel = pixel / 255.0
pixel = (pixel - mean) / std

mean = [0.485, 0.456, 0.406]
std  = [0.229, 0.224, 0.225]
```

然后 HWC → CHW。

注意：

- OpenCV 解码默认是 BGR，可直接按 B、G、R 三通道顺序写入。
- 浏览器 Canvas、Sharp 和多数 JS 图片库输出 RGB；要么先交换 R/B，要么明确让张量三个通道按 BGR 写入。
- 不要因为 mean/std 看起来像 ImageNet RGB 参数，就擅自改成 RGB；以官方处理链和你基准实现的结果为准。

### 6.4 检测输出

典型输出：

```text
[batch, 1, map_height, map_width]
```

取第一通道得到概率图：

```text
prob = output[n, 0, :, :]
bitmap = prob > thresh
```

### 6.5 DB 后处理步骤

每张图：

1. `bitmap = probability_map > 0.2`
2. 在 bitmap 上查找外轮廓。
3. 最多处理 `3000` 个候选轮廓。
4. 对每个轮廓计算最小外接旋转矩形。
5. 过滤最短边过小的框。
6. 在原始 probability map 上计算框内平均分。
7. 过滤 `score < 0.45`。
8. 使用 `distance = polygon_area * 1.4 / polygon_perimeter` 计算外扩距离。
9. 用 Clipper/pyclipper 做 polygon offset。
10. 对外扩多边形再次求最小旋转矩形。
11. 将坐标从输出图映射到输入图，再除以 `ratio_w`、`ratio_h` 映射回原图。
12. 裁剪到图像边界。
13. 统一四点顺序：左上、右上、右下、左下。

常见最短边过滤阈值为 3 像素左右；这是后处理工程参数，不是模型权重参数，可按分辨率调整。

### 6.6 关键伪代码

```text
for contour in findContours(bitmap):
    box = minAreaRect(contour)
    if shortSide(box) < min_size:
        continue

    score = mean(probability_map inside contour_or_box)
    if score < box_thresh:
        continue

    distance = area(box) * unclip_ratio / perimeter(box)
    expanded = polygonOffset(box, distance)
    final_box = minAreaRect(expanded)

    final_box = scaleBack(final_box, ratio_h, ratio_w)
    results.push({ points: orderClockwise(final_box), score })
```

---

## 7. 文本框排序与透视裁剪

### 7.1 阅读顺序

官方实现先按左上点的 `y`、再按 `x` 排序；若相邻框左上点 y 差小于约 10 像素，则按 x 调整为同一行从左到右。

```text
sort key = (top_left_y, top_left_x)
same-line tolerance ≈ 10 px
```

复杂版面（多栏、表格、竖排）需要单独的版面分析，不应只依赖此简单排序。

### 7.2 透视裁剪

对四边形：

1. 计算上/下边长度的最大值作为目标宽。
2. 计算左/右边长度的最大值作为目标高。
3. 建立到矩形 `(0,0)-(w,h)` 的透视矩阵。
4. `warpPerspective` 得到文字行图。
5. 若裁剪图高度/宽度比大于约 1.5，可顺时针旋转 90°，以兼容竖直文字行。

裁剪必须从原始高分辨率图像执行，不要从 640×640 的检测输入图裁剪，否则会损失识别精度。

---

## 8. 识别模型参数

### 8.1 官方结构和参数

`PP-OCRv6_small_rec`：

| 项目 | 值 |
|---|---|
| 算法 | SVTR_LCNet |
| Backbone | PPLCNetV4 Small |
| 推理后处理 | CTCLabelDecode |
| 参考输入 | `[3, 48, 320]` |
| 训练最大文本长度 | 25 |
| 使用空格字符 | `true` |
| 字符字典 | `ppocrv6_dict.txt` |
| 训练多尺度 | `320×32`、`320×48`、`320×64` |

`max_text_length=25` 是训练配置；直接 ONNX 推理时，真实可解码时间步由输出维度 `T` 决定。不要在业务层无条件截断到 25 个 Unicode 字符。

### 8.2 识别预处理

目标形状通常为 `[1,3,48,320]`。

对于原裁剪图 `(h,w)`：

```text
resized_w = min(ceil(48 * w / h), 320)
resize image to [48, resized_w]
right-pad zeros to [48, 320]
```

归一化：

```text
v = pixel / 255.0
v = (v - 0.5) / 0.5
```

即范围约为 `[-1, 1]`，然后 HWC → CHW。

同样应保持官方 BGR 通道顺序。

### 8.3 动态宽度

若 ONNX metadata 的宽度为动态值：

- 同一 batch 内可按最大宽高比计算本批次宽度；
- 目标宽应满足模型约束；
- 不同宽度会改变时间步 `T`；
- 最简单稳定的个人软件方案仍是固定 320 宽右侧补零。

### 8.4 识别输出

典型：

```text
shape = [N, T, C]
```

其中：

- `N`：batch
- `T`：时间步
- `C`：类别数

每个时间步取最大类别：

```text
index[t] = argmax(output[n, t, :])
confidence[t] = max(output[n, t, :])
```

有些导出模型输出 logits，有些是已归一化分数。CTC 的 argmax 不受 softmax 影响，但“置信度”数值会受影响。若输出值明显不在 `[0,1]`，应先按最后一维 softmax 后再计算展示分数。

---

## 9. 字典与 CTC 解码

### 9.1 字符表优先级

从高到低：

1. 模型包 `inference.yml` 中的 `PostProcess.character_dict`
2. 模型包中生成的 `ppocr_keys.txt`
3. 与模型版本匹配的官方 `ppocrv6_dict.txt`

不要随意使用 PP-OCRv3/v4/v5 字典。字符顺序改变一个位置，所有输出都会错位。

### 9.2 构建类别表

官方 CTC 解码逻辑相当于：

```text
characters = ["blank"] + dictionary_lines + [" "]
```

其中空格是否追加，应以 `use_space_char` 和模型包配置为准。

截至资料核对日，仓库中的 `ppocrv6_dict.txt` 可作为约 18,708 行的 sanity check；加空格与 blank 后，常见类别数约为 18,710。**这不是永久契约，必须以你的 ONNX 输出 C 和模型包字符表为准。**

### 9.3 CTC 去重规则

```text
previous = -1
for t in 0..T:
    idx = argmax(...)
    if idx == blank:
        previous = idx
        continue
    if idx == previous:
        continue
    append characters[idx]
    append confidence
    previous = idx
```

最终置信度通常取所有保留时间步置信度的均值。

### 9.4 必做一致性检查

```text
output_class_count == characters.length
```

不一致时立即报错，并输出：

- ONNX 输出 shape
- 字典行数
- 是否追加空格
- blank index
- 字典文件路径

不要用 `% characters.length` 等方式“容错”，那会隐藏错误。

---

## 10. 推荐运行参数

### 10.1 默认配置

```json
{
  "det": {
    "input_width": 640,
    "input_height": 640,
    "threshold": 0.2,
    "box_threshold": 0.45,
    "max_candidates": 3000,
    "unclip_ratio": 1.4,
    "min_box_size": 3
  },
  "rec": {
    "input_height": 48,
    "input_width": 320,
    "batch_size": 8,
    "append_space": true,
    "blank_index": 0
  },
  "pipeline": {
    "drop_score": 0.5,
    "rotate_tall_crop": true,
    "same_line_y_tolerance": 10
  }
}
```

### 10.2 调参方向

| 问题 | 优先调整 |
|---|---|
| 漏掉浅色/细小文字 | 降低 `det.threshold`，如 0.18；必要时提高检测输入分辨率 |
| 出现大量背景框 | 提高 `box_threshold`，如 0.5–0.6 |
| 文字框切得太紧 | 提高 `unclip_ratio`，如 1.5–1.8 |
| 相邻文字行粘连 | 降低 `unclip_ratio` 或提高检测分辨率 |
| 识别长文本被压缩 | 使用动态宽度，或提高固定宽度并重新确认模型支持 |
| 结果中低质量文本太多 | 提高 `drop_score` |
| CPU 慢 | 批量识别、session 复用、减少图片长边、调整 ORT 线程 |
| 内存高 | 降低识别 batch、限制最大候选框、分块处理超大图 |

每次只调整一个参数，并用固定测试集记录 precision/recall、平均耗时、P95 和峰值内存。

---

## 11. Rust 接入

### 11.1 Cargo.toml

```toml
[dependencies]
anyhow = "1"
clap = { version = "4", features = ["derive"] }
image = "0.25"
ndarray = "0.16"
ort = "2.0.0-rc.13"
```

若需要完整检测后处理，再增加 OpenCV 与多边形 offset 库。OpenCV Rust crate 需要本机安装 OpenCV 或在 CI 中准备对应二进制。

### 11.2 创建 Session

```rust
use ort::session::Session;

let mut session = Session::builder()?
    .with_intra_threads(2)?
    .commit_from_file("models/rec/model.onnx")?;

for input in session.inputs() {
    println!("input: {} {:?}", input.name(), input.dtype());
}
for output in session.outputs() {
    println!("output: {} {:?}", output.name(), output.dtype());
}
```

`ort` 2.x 的 `run` 使用 `&mut self`。并发建议：

- 单线程应用：复用一个 session。
- 多线程服务：每个 worker 一个 session，或在上层做 batch。
- 不要为每张图片重新加载模型。
- 检测 session 与识别 session 分开持有。

### 11.3 输入张量

```rust
use ort::value::TensorRef;

let outputs = session.run(ort::inputs![
    TensorRef::from_array_view(&input_array)?
])?;
```

`input_array` 应为连续的 `ndarray::Array4<f32>`，形状严格为 NCHW。

### 11.4 输出读取

```rust
let output = outputs[0].try_extract_array::<f32>()?;
println!("shape = {:?}", output.shape());
```

生产代码应在启动时检查 rank 和维度，并在不符合预期时退出。

### 11.5 Rust 完整 OCR 的建议模块

```text
src/
  model.rs          # session、metadata、provider
  det_preprocess.rs
  db_postprocess.rs # OpenCV contours + polygon offset
  crop.rs           # perspective transform
  rec_preprocess.rs
  ctc.rs
  pipeline.rs
  types.rs
```

附带 starter 中的 Rust 示例完整实现了“单行识别模型”的预处理、ORT 推理和 CTC 解码。检测几何部分建议先对照 Python 基准逐项移植，避免一次性调试整条链路。

### 11.6 Rust 打包注意事项

- 若使用 `ort` 默认 `download-binaries`，构建阶段会获取匹配的 ORT 二进制。
- 离线构建或企业 CI 应缓存依赖，或使用 `load-dynamic` 并随应用分发 ORT 动态库。
- Windows 动态库要位于 exe 同目录或 `PATH`。
- Linux 注意 `rpath`/`LD_LIBRARY_PATH` 和 glibc 兼容性。
- macOS Apple Silicon 使用 arm64 依赖，避免混用 x64。
- 对模型文件计算 SHA-256，在启动时校验，防止错版本。

---

## 12. TypeScript / Node.js 接入

### 12.1 创建 Session

```ts
import * as ort from "onnxruntime-node";

const session = await ort.InferenceSession.create(
  "models/rec/model.onnx",
  {
    executionProviders: ["cpu"],
    graphOptimizationLevel: "all"
  }
);

console.log(session.inputNames);
console.log(session.outputNames);
console.log(session.inputMetadata);
console.log(session.outputMetadata);
```

### 12.2 创建 Tensor 并运行

```ts
const tensor = new ort.Tensor(
  "float32",
  nchwFloat32Array,
  [1, 3, 48, 320]
);

const feeds: Record<string, ort.Tensor> = {
  [session.inputNames[0]]: tensor
};

const outputs = await session.run(feeds);
const result = outputs[session.outputNames[0]];
console.log(result.dims);
```

不要硬编码输入输出节点名。

### 12.3 图片处理

Node 示例使用 `sharp`：

1. 解码为 3 通道 RGB。
2. 按比例 resize 到高 48、宽不超过 320。
3. 把 RGB Buffer 写入 BGR NCHW。
4. 归一化到 `[-1,1]`。
5. 剩余右侧保持 0。

完整代码位于 starter 的 `typescript/src/recognize.ts`。

### 12.4 Node 并发

- session 创建是昂贵操作，只创建一次。
- 可把多个文本行组成 batch `[N,3,48,320]`。
- 识别 crop 数量较多时，batch 4–16 往往比逐张运行更有效。
- worker_threads 中不要无条件共享可变状态；可每个 worker 初始化自己的 session。
- 先测单 session + batch，再考虑 worker 数。

### 12.5 Electron/Tauri

- Electron 主进程或 Node sidecar 可直接使用 `onnxruntime-node`。
- Electron renderer 不建议直接加载 Node native addon，优先通过 IPC 调主进程。
- Tauri 主体通常优先走 Rust `ort`，前端只负责传图和展示结果。
- 模型放在只读 resources 目录，首次启动可复制到应用数据目录。

---

## 13. 浏览器接入

### 13.1 WASM

```ts
import * as ort from "onnxruntime-web";

ort.env.wasm.numThreads = Math.min(4, navigator.hardwareConcurrency || 1);
ort.env.wasm.simd = true;

const session = await ort.InferenceSession.create("/models/rec/model.onnx", {
  executionProviders: ["wasm"],
  graphOptimizationLevel: "all"
});
```

需要正确部署 `.wasm` 资源。若构建工具改变了资源路径，可设置：

```ts
ort.env.wasm.wasmPaths = "/ort/";
```

### 13.2 WebGPU

```ts
import * as ort from "onnxruntime-web/webgpu";

const session = await ort.InferenceSession.create("/models/rec/model.onnx", {
  executionProviders: ["webgpu", "wasm"]
});
```

官方文档仍把 WebGPU 导入路径标为实验性能力。上线前必须覆盖目标浏览器、显卡、驱动、移动端温控和前后台切换测试。

### 13.3 浏览器部署建议

- 先做“识别单行裁剪图”；完整 DB 检测后处理在浏览器端需要 OpenCV.js 或纯 JS/WASM 几何库。
- 模型使用长期缓存与内容哈希文件名。
- 配置正确的 MIME：`.onnx` 通常使用 `application/octet-stream`。
- 避免模型被打进 JS bundle；使用静态 URL 按需加载。
- 首次加载显示明确进度。
- 对跨域模型资源配置 CORS。
- 低内存移动浏览器应降低检测分辨率和识别 batch。

---

## 14. Python 基准实现的用途

starter 包中的 `python/reference_full_ocr.py` 用于：

1. 验证 det/rec ONNX 能运行。
2. 打印真实输入输出名称和 shape。
3. 生成统一 JSON，作为 Rust/TS 回归基准。
4. 帮助定位误差发生在预处理、推理、DB 后处理、裁剪还是 CTC。

推荐对同一张图保存以下中间数据：

```text
det_input.npy
det_output.npy
det_boxes.json
crop_000.png
rec_input_000.npy
rec_output_000.npy
result.json
```

Rust/TS 可逐文件比对：

- 输入张量最大绝对误差：建议接近 0
- ONNX 输出最大绝对误差：CPU 同版本通常应很小
- DB 框坐标：允许少量浮点/轮廓差异
- CTC 文本：应完全一致

---

## 15. 性能与稳定性

### 15.1 Session 生命周期

错误：

```text
每次请求：加载模型 → 创建 session → 推理 → 释放
```

正确：

```text
应用启动：创建 det session + rec session
每次请求：复用 session
应用退出：释放资源
```

### 15.2 线程

ORT 的线程数并非越多越快。个人桌面软件建议从以下配置测试：

```text
intra_op_threads = 1, 2, 4
inter_op_threads = 1
```

同时运行 det 和 rec 时，避免两套 session 都占满全部 CPU 核，导致争用。

### 15.3 批处理

- 检测通常 batch=1。
- 识别适合 batch。
- 将 crop 按宽高比排序，再组成 batch，可减少动态宽度场景的 padding。
- 固定宽 320 时，也可直接按数量分批。

### 15.4 超大图

对于长截图、扫描图或 4K/8K 图片：

- 限制检测输入最大边。
- 或分块检测并保留重叠区域。
- 合并跨块框时做 IoU/距离去重。
- 始终在原图裁剪识别区域。
- 设置最大候选框数量和单图超时。

---

## 16. 常见错误与排查

### 16.1 全部识别为乱码

检查：

1. 字典是否对应 PP-OCRv6。
2. blank 是否在 index 0。
3. 是否按配置追加空格。
4. 输出 `C` 是否等于字符表长度。
5. 是否错误地把字典按 UTF-16 code unit 拆分；应按“每行一个字符串”读取。

### 16.2 识别结果接近但总有错字

检查：

- BGR/RGB 通道顺序。
- 归一化是否为 `(x/255 - 0.5)/0.5`。
- resize 是否保持比例并右补零。
- 是否从低分辨率检测图裁剪。
- CTC 是否去除连续重复和 blank。

### 16.3 检测框整体偏移或缩放错误

检查：

- 是否保存 `ratio_h` 和 `ratio_w`。
- 固定 640×640 直接 resize 时，两个比例通常不同。
- 是否先映射 output map → detector input，再映射 detector input → original。
- 宽高顺序是否混用。
- 坐标是否在最后裁剪到原图范围。

### 16.4 检测不到文字

检查：

- 输入是否为 float32 NCHW。
- 颜色顺序。
- mean/std。
- 输出是否取了 `[0,0,:,:]`。
- `threshold` 可从 0.2 降到 0.18 做诊断。
- 不要一开始就提高 `box_threshold`。

### 16.5 框太紧导致缺字

- 调高 `unclip_ratio`。
- 检查 polygon offset 距离公式。
- 检查轮廓 perimeter 是否为 0。
- 透视裁剪可额外增加 1–2 像素安全边距。

### 16.6 Node 找不到 native module

- 确认 Node 版本和平台受 `onnxruntime-node` 预编译包支持。
- 删除 `node_modules` 和 lockfile 后重新安装只作为诊断，不是长期方案。
- Electron 需要匹配 ABI，必要时 rebuild。
- 避免把 native addon 交给前端 bundler 重新打包。

### 16.7 Rust 构建期下载失败

- 缓存 Cargo registry/git 和 ORT 下载目录。
- 使用公司代理。
- 或改用 `load-dynamic`，手动分发 ORT 动态库。
- 固定 crate 和 ORT 版本，避免 CI 当天获取到不同产物。

### 16.8 ONNX 转换成功但运行失败

- 用 `onnx.checker` 检查。
- 打印 opset。
- 更新到与模型兼容的 ORT。
- 检查模型是否含 Paddle 特有算子。
- 分别测试 CPU provider，先排除 GPU provider 配置问题。
- 把转换日志和模型 metadata 保存进发布产物。

---

## 17. 发布目录建议

```text
app/
  bin/
  models/
    ppocrv6-small/
      det.onnx
      rec.onnx
      ppocr_keys.txt
      inference-det.yml
      inference-rec.yml
      model-manifest.json
  config/
    ocr-defaults.json
  licenses/
    PaddleOCR-Apache-2.0.txt
    ONNX-Runtime-MIT.txt
  tests/
    fixtures/
```

`model-manifest.json` 示例：

```json
{
  "name": "ppocrv6-small",
  "converted_at": "2026-08-07",
  "det_sha256": "<fill>",
  "rec_sha256": "<fill>",
  "dictionary_sha256": "<fill>",
  "det_input": [1, 3, 640, 640],
  "rec_input": [1, 3, 48, 320],
  "dictionary_classes": "<fill from actual model>",
  "conversion_tool": "PaddleX paddle2onnx",
  "opset": "<fill from actual model>"
}
```

---

## 18. 验收清单

### 模型

- [ ] det/rec 来自同一 PP-OCRv6 Small 发布体系。
- [ ] ONNX checker 通过。
- [ ] 记录真实输入输出名、shape、dtype、opset。
- [ ] 字典与识别输出类别数一致。
- [ ] 模型和字典有 SHA-256。

### 检测

- [ ] BGR + ImageNet mean/std。
- [ ] NCHW float32。
- [ ] 使用 0.2 / 0.45 / 3000 / 1.4 作为初始 DB 参数。
- [ ] 正确还原 x/y 两个方向比例。
- [ ] 从原图透视裁剪。

### 识别

- [ ] 高 48、宽不超过 320。
- [ ] 保持比例，右侧补 0。
- [ ] `(x/255 - 0.5)/0.5`。
- [ ] blank、重复字符和空格处理正确。
- [ ] 输出 C 与字典严格一致。

### 工程

- [ ] session 在启动时创建并复用。
- [ ] Rust/TS 与 Python 基准对同一测试集结果一致。
- [ ] 有中文、英文、数字、标点、空格、旋转、低对比度样本。
- [ ] 记录平均/P95/P99 延迟和峰值内存。
- [ ] 异常图、空图、超大图不会崩溃。
- [ ] 发布包包含许可证和模型来源记录。

---

## 19. 推荐实施顺序

1. 下载 Paddle inference 模型。
2. 转换 det/rec ONNX。
3. 用 `inspect_onnx.py` 生成模型清单。
4. 用 Python 基准跑通单行识别。
5. 用 Python 基准跑通完整检测+识别。
6. 在 Rust 或 Node 中先移植识别模型。
7. 对比 `rec_input.npy` 和 `rec_output.npy`。
8. 再移植检测预处理。
9. 最后移植 DB 几何后处理和透视裁剪。
10. 建立固定回归集，锁定依赖与模型哈希。

这种顺序能把问题快速定位到具体阶段，避免在“整条 OCR 没结果”时同时排查十几个变量。

---

## 20. 官方与主要资料来源

1. PaddleOCR General OCR / PP-OCRv6 模型说明  
   https://github.com/PaddlePaddle/PaddleOCR/blob/main/docs/version3.x/pipeline_usage/OCR.en.md

2. PP-OCRv6 Small 检测配置  
   https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/main/configs/det/PP-OCRv6/PP-OCRv6_small_det.yml

3. PP-OCRv6 Small 识别配置  
   https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/main/configs/rec/PP-OCRv6/PP-OCRv6_small_rec.yml

4. PaddleOCR 检测预处理实现  
   https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/main/ppocr/data/imaug/operators.py

5. PaddleOCR DB 后处理实现  
   https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/main/ppocr/postprocess/db_postprocess.py

6. PaddleOCR 识别推理实现  
   https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/main/tools/infer/predict_rec.py

7. PaddleOCR CTC 解码实现  
   https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/main/ppocr/postprocess/rec_postprocess.py

8. PaddleOCR 系统流水线、框排序与裁剪调用  
   https://github.com/PaddlePaddle/PaddleOCR/blob/main/tools/infer/predict_system.py

9. PaddleX Paddle2ONNX 文档  
   https://paddlepaddle.github.io/PaddleX/latest/pipeline_deploy/paddle2onnx.html

10. ONNX Runtime Node.js  
    https://onnxruntime.ai/docs/get-started/with-javascript/node.html

11. ONNX Runtime Web  
    https://onnxruntime.ai/docs/get-started/with-javascript/web.html

12. ONNX Runtime JavaScript API  
    https://onnxruntime.ai/docs/api/js/interfaces/InferenceSession.html

13. Rust `ort` 文档  
    https://docs.rs/ort/latest/ort/

---

## 21. 版本风险说明

PP-OCRv6、PaddleX、Paddle2ONNX、ONNX Runtime 和 Rust `ort` 都会更新。本文把“模型语义参数”和“当前工具版本”分开处理：

- 模型语义参数：以下载模型包中的 `inference.yml` 为最高优先级。
- 输入输出节点：以 ONNX session metadata 为准。
- 字符表：以模型包为准。
- ORT API：以 lockfile 固定版本对应文档为准。
- 本文的版本与链接核对日期为 2026-08-07。

在升级任意一项后，必须重新运行固定回归集。
