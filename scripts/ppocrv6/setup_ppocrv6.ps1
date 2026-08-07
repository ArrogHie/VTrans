# VTrans PP-OCRv6 Small 模型准备脚本（方案 B）
#
# 完整流程：下载 → (转换 ONNX) → 检查 → Python 基准 → manifest 回填
# 产物：
#   - src-tauri/resources/models/ocr/det.onnx
#   - src-tauri/resources/models/ocr/rec.onnx（rec_ja / rec_en / rec_multi 三槽位共享同一文件）
#   - src-tauri/resources/models/ocr/ppocrv6_dict.txt
#   - 检查/基准报告（默认写入 scripts/ppocrv6/work/）
#
# 开发机要求（参见 docs/PP-OCRv6_small_ONNX_Rust_TS_接入指南.md §4.1）：
#   - Python 3.10 或 3.11（Windows 下 3.12 亦可，3.14 未经验证）
#   - PaddlePaddle 3.0+（脚本锁定 3.3.1）、paddlex[ocr]、paddle2onnx 2.0.2rc3 插件
#   - onnx、onnxruntime、opencv-python-headless、numpy、pyclipper、pyyaml
#   - Windows 下若 paddle2onnx 转换 DLL 异常，可使用 WSL2 执行本脚本
#
# 用法：
#   .\scripts\ppocrv6\setup_ppocrv6.ps1                      # 全流程（下载 + 转换 + 检查 + 基准 + 回填）
#   .\scripts\ppocrv6\setup_ppocrv6.ps1 -SkipConversion      # 使用已提供的 ONNX（不跑 PaddleX 转换）
#   .\scripts\ppocrv6\setup_ppocrv6.ps1 -SkipBaseline        # 跳过 Python 基准（仅下载/转换/检查/回填）
#   .\scripts\ppocrv6\setup_ppocrv6.ps1 -KeepWork            # 保留中间产物（默认保留，-CleanWork 清理）
[CmdletBinding()]
param(
    [switch]$SkipConversion,
    [switch]$SkipBaseline,
    [switch]$CleanWork
)

$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$workDir = Join-Path $PSScriptRoot "work"
$downloadDir = Join-Path $workDir "download"
$paddleDir = Join-Path $workDir "paddle"
$onnxDir = Join-Path $workDir "onnx"
$baselineDir = Join-Path $workDir "baseline"
$ocrDir = Join-Path $repoRoot "src-tauri\resources\models\ocr"
$manifestPath = Join-Path $repoRoot "src-tauri\resources\models\manifest.json"

$detTarUrl = "https://paddle-model-ecology.bj.bcebos.com/paddlex/official_inference_model/paddle3.0.0/PP-OCRv6_small_det_infer.tar"
$recTarUrl = "https://paddle-model-ecology.bj.bcebos.com/paddlex/official_inference_model/paddle3.0.0/PP-OCRv6_small_rec_infer.tar"
$dictUrl = "https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/main/ppocr/utils/dict/ppocrv6_dict.txt"

function Write-Step([string]$Title) {
    Write-Host ""
    Write-Host "==== $Title ====" -ForegroundColor Cyan
}

function Assert-Command([string]$Name) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "required command not found: $Name"
    }
}

Write-Step "PP-OCRv6 Small 模型准备"
Write-Host "repo: $repoRoot"

foreach ($d in @($workDir, $downloadDir, $paddleDir, $onnxDir, $baselineDir, $ocrDir)) {
    New-Item -ItemType Directory -Path $d -Force | Out-Null
}
if ($CleanWork) {
    Get-ChildItem $workDir -Force | Remove-Item -Recurse -Force
    foreach ($d in @($workDir, $downloadDir, $paddleDir, $onnxDir, $baselineDir, $ocrDir)) {
        New-Item -ItemType Directory -Path $d -Force | Out-Null
    }
}

# ── 1. 下载 ─────────────────────────────────────────────────────────────
Write-Step "1/5 下载官方模型包与字典"
$detTar = Join-Path $downloadDir "PP-OCRv6_small_det_infer.tar"
$recTar = Join-Path $downloadDir "PP-OCRv6_small_rec_infer.tar"
$dictRaw = Join-Path $downloadDir "ppocrv6_dict.txt"

if (-not (Test-Path $detTar)) {
    Write-Host "下载 det 模型包 ..."
    curl.exe -sS -L --fail --retry 3 --connect-timeout 30 -o $detTar $detTarUrl
    if ($LASTEXITCODE -ne 0) { throw "det 下载失败" }
}
if (-not (Test-Path $recTar)) {
    Write-Host "下载 rec 模型包 ..."
    curl.exe -sS -L --fail --retry 3 --connect-timeout 30 -o $recTar $recTarUrl
    if ($LASTEXITCODE -ne 0) { throw "rec 下载失败" }
}
if (-not (Test-Path $dictRaw)) {
    Write-Host "下载 ppocrv6_dict.txt ..."
    curl.exe -sS -L --fail --retry 3 --connect-timeout 30 -o $dictRaw $dictUrl
    if ($LASTEXITCODE -ne 0) { throw "字典下载失败" }
}
Write-Host "下载完成："
Get-ChildItem $downloadDir | Select-Object Name, Length | Format-Table -AutoSize

# ── 2. 解压 + 转换（可选跳过） ─────────────────────────────────────────
if (-not $SkipConversion) {
    Write-Step "2/5 解压并转换 ONNX（PaddleX paddle2onnx）"
    Assert-Command "python"

    $detPaddleDir = Join-Path $paddleDir "PP-OCRv6_small_det_infer"
    $recPaddleDir = Join-Path $paddleDir "PP-OCRv6_small_rec_infer"
    if (-not (Test-Path (Join-Path $detPaddleDir "inference.pdiparams"))) {
        tar -xf $detTar -C $paddleDir
        if ($LASTEXITCODE -ne 0) { throw "det 解压失败" }
    }
    if (-not (Test-Path (Join-Path $recPaddleDir "inference.pdiparams"))) {
        tar -xf $recTar -C $paddleDir
        if ($LASTEXITCODE -ne 0) { throw "rec 解压失败" }
    }

    # PaddleX CLI 通过 shutil.which("paddle2onnx") 检测插件，需把 venv Scripts 与
    # paddle libs 加入 PATH；Windows 下若 DLL 加载失败，请改用 WSL2 执行本脚本。
    $pythonExe = (Get-Command python).Source
    $pythonDir = Split-Path $pythonExe -Parent
    $env:PATH = "$pythonDir;$env:PATH"
    $sitePkgs = & $pythonExe -c "import site; print(site.getsitepackages()[0])"
    $paddleLibs = Join-Path $sitePkgs "paddle\libs"
    if (Test-Path $paddleLibs) { $env:PATH = "$paddleLibs;$env:PATH" }

    & $pythonExe -m pip show paddlex paddle2onnx paddlepaddle 2>$null | Select-String "Name|Version"

    $detOut = Join-Path $onnxDir "det"
    $recOut = Join-Path $onnxDir "rec"
    & $pythonExe -m paddlex --paddle2onnx --paddle_model_dir $detPaddleDir --onnx_model_dir $detOut --opset_version 11
    if ($LASTEXITCODE -ne 0) { throw "det 转换失败（Windows 下可尝试 WSL2）" }
    & $pythonExe -m paddlex --paddle2onnx --paddle_model_dir $recPaddleDir --onnx_model_dir $recOut --opset_version 11
    if ($LASTEXITCODE -ne 0) { throw "rec 转换失败（Windows 下可尝试 WSL2）" }

    # 转换产物可能命名为 model.onnx 或其他；统一查找。
    $detOnnx = Get-ChildItem -Recurse $detOut -Filter *.onnx | Sort-Object Length -Descending | Select-Object -First 1
    $recOnnx = Get-ChildItem -Recurse $recOut -Filter *.onnx | Sort-Object Length -Descending | Select-Object -First 1
    if (-not $detOnnx -or -not $recOnnx) { throw "转换后未找到 ONNX 产物" }
    Write-Host "det onnx: $($detOnnx.FullName) ($($detOnnx.Length) bytes)"
    Write-Host "rec onnx: $($recOnnx.FullName) ($($recOnnx.Length) bytes)"
} else {
    Write-Step "2/5 转换已跳过（-SkipConversion）"
    Write-Host "使用已提供的 ONNX："
    $detOnnx = Get-ChildItem $onnxDir -Recurse -Filter *.onnx -ErrorAction SilentlyContinue | Sort-Object Length -Descending | Select-Object -First 1
    $recOnnx = Get-ChildItem $onnxDir -Recurse -Filter *.onnx -ErrorAction SilentlyContinue | Sort-Object Length -Descending | Select-Object -First 1
    if (-not $detOnnx -or -not $recOnnx) {
        throw "未找到已提供的 ONNX。请放入 $onnxDir 或先运行完整流程。"
    }
    Write-Host "det onnx: $($detOnnx.FullName) ($($detOnnx.Length) bytes)"
    Write-Host "rec onnx: $($recOnnx.FullName) ($($recOnnx.Length) bytes)"
}

# ── 3. ONNX 检查 + 类数一致性 ─────────────────────────────────────────
Write-Step "3/5 ONNX 检查与类数一致性"
Assert-Command "python"
$inspectOut = Join-Path $workDir "inspect_report.json"
& python $PSScriptRoot\inspect_onnx.py `
    --det $detOnnx.FullName `
    --rec $recOnnx.FullName `
    --dict $dictRaw `
    --out $inspectOut
if ($LASTEXITCODE -ne 0) { throw "ONNX 检查失败" }

# ── 4. Python 基准 ─────────────────────────────────────────────────────
if (-not $SkipBaseline) {
    Write-Step "4/5 Python 基准（固定测试图 → det/rec 中间产物 + JSON）"
    $testImage = Join-Path $repoRoot "tests\ocr\test1.png"
    if (-not (Test-Path $testImage)) {
        Write-Host "WARN: 未找到 $testImage，跳过基准（可稍后手动运行）" -ForegroundColor Yellow
    } else {
        & python $PSScriptRoot\baseline_ocr.py `
            --det $detOnnx.FullName `
            --rec $recOnnx.FullName `
            --dict $dictRaw `
            --image $testImage `
            --out $baselineDir
        if ($LASTEXITCODE -ne 0) { throw "Python 基准失败" }
        Write-Host "基准产物："
        Get-ChildItem $baselineDir | Select-Object Name, Length | Format-Table -AutoSize
    }
} else {
    Write-Step "4/5 基准已跳过（-SkipBaseline）"
}

# ── 5. manifest 回填 ───────────────────────────────────────────────────
Write-Step "5/5 回填 manifest.json（SHA-256 / size_bytes）"
& python $PSScriptRoot\backfill_manifest.py `
    --manifest $manifestPath `
    --det $detOnnx.FullName `
    --rec $recOnnx.FullName `
    --dict $dictRaw `
    --dict-name ppocrv6_dict.txt `
    --deploy-dir $ocrDir
if ($LASTEXITCODE -ne 0) { throw "manifest 回填失败" }

Write-Step "完成"
Write-Host "下一步：cargo run --bin vtrans-verify-models -- --models src-tauri/resources/models"
Write-Host "验证 CLI 输出 'all model files are valid' 即通过。"
