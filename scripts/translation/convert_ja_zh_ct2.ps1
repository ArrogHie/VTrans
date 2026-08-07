# VTrans ja→zh CTranslate2 INT8 转换脚本（Windows PowerShell）
#
# 流程：下载 shun89/opus-mt-ja-zh（锁定 HF revision）→
#       ct2-transformers-converter INT8 转换 → 目录体积实测（≤110 MB）。
#
# 产物（输出目录，默认 src-tauri/resources/models/translation/ja-zh）：
#   model.bin  config.json  source_vocabulary.json  target_vocabulary.json
#   source.spm  target.spm
# 以及转换元数据 scripts/translation/work/ja-zh-meta.json（模型 revision、
# ctranslate2 版本、quantization），供回填脚本写入根 manifest。
#
# 开发机要求：
#   - Python 3.10+（本机 3.12 已验证）
#   - 网络（下载 HF 模型约 314 MB；转换所需依赖安装一次即可）
#   - CTranslate2 锁定 4.8.1（本脚本在专用 venv 中安装）
#
# 用法：
#   .\scripts\translation\convert_ja_zh_ct2.ps1                      # 全流程
#   .\scripts\translation\convert_ja_zh_ct2.ps1 -Revision <commit>   # 锁定 revision
#   .\scripts\translation\convert_ja_zh_ct2.ps1 -SkipDownload        # 使用本地已下载的模型目录
#   .\scripts\translation\convert_ja_zh_ct2.ps1 -SkipInstall         # 复用已存在的 venv
#
# 退出码：0 成功；非 0 失败（缺依赖 / 下载失败 / 转换失败 / 体积超限）。
[CmdletBinding()]
param(
    [string]$Model = "shun89/opus-mt-ja-zh",
    [string]$Revision = "",
    [string]$OutputDir = "",
    [switch]$SkipDownload,
    [switch]$SkipInstall,
    [string]$LocalModelDir = ""
)

$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$workDir = Join-Path $PSScriptRoot "work"
$venvDir = Join-Path $workDir ".venv"
$sourceDir = Join-Path $workDir "opus-mt-ja-zh"
if (-not $OutputDir) {
    $OutputDir = Join-Path $repoRoot "src-tauri\resources\models\translation\ja-zh"
}
$metaPath = Join-Path $workDir "ja-zh-meta.json"

# 锁定版本：ctranslate2 4.8.1（与 docs/DEVELOPMENT.md 与接入指南一致）。
$CT2_VERSION = "4.8.1"
$PYTHON_REQ = "3.10"

function Write-Step([string]$Title) {
    Write-Host ""
    Write-Host "==== $Title ====" -ForegroundColor Cyan
}

function Assert-Command([string]$Name) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "required command not found: $Name"
    }
}

function Assert-PythonVersion {
    $py = (Get-Command python).Source
    $ver = & $py -c "import sys; print(f'{sys.version_info.major}.{sys.version_info.minor}')"
    $major = [int]($ver.Split(".")[0])
    $minor = [int]($ver.Split(".")[1])
    if ($major -lt 3 -or ($major -eq 3 -and $minor -lt 10)) {
        throw "Python $PYTHON_REQ+ required, found $ver"
    }
    Write-Host "python: $py ($ver)"
}

Write-Step "ja→zh CTranslate2 INT8 转换"
Write-Host "model: $Model"
Write-Host "output: $OutputDir"
New-Item -ItemType Directory -Path $workDir -Force | Out-Null
New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null

Assert-Command "python"
Assert-PythonVersion

# ── 1. 准备 venv（首次）与安装锁定依赖 ──────────────────────────────
Write-Step "1/4 准备 Python 环境（ctranslate2==$CT2_VERSION）"
if (-not (Test-Path (Join-Path $venvDir "Scripts\python.exe"))) {
    Write-Host "创建 venv: $venvDir"
    & python -m venv $venvDir
    if ($LASTEXITCODE -ne 0) { throw "venv 创建失败" }
}
$venvPython = Join-Path $venvDir "Scripts\python.exe"
if (-not $SkipInstall) {
    & $venvPython -m pip install --upgrade pip
    if ($LASTEXITCODE -ne 0) { throw "pip 升级失败" }
    & $venvPython -m pip install "ctranslate2==$CT2_VERSION" "transformers>=4.50" "torch" "sentencepiece" "huggingface_hub"
    if ($LASTEXITCODE -ne 0) { throw "依赖安装失败（ctranslate2==$CT2_VERSION 等）" }
}
& $venvPython -c "import ctranslate2; print('ctranslate2', ctranslate2.__version__)"
if ($LASTEXITCODE -ne 0) { throw "ctranslate2 不可用（如首次安装失败请删除 $venvDir 重试）" }

# ── 2. 下载/定位源模型 ──────────────────────────────────────────────
Write-Step "2/4 下载源模型（$Model）"
$resolvedRevision = ""
if ($SkipDownload) {
    if (-not $LocalModelDir) {
        $LocalModelDir = $sourceDir
    }
    if (-not (Test-Path (Join-Path $LocalModelDir "config.json"))) {
        throw "本地模型目录不完整: $LocalModelDir（缺少 config.json）"
    }
    Write-Host "使用本地模型目录: $LocalModelDir"
} else {
    if (Test-Path $sourceDir) {
        Write-Host "已有本地副本: $sourceDir"
    } else {
        New-Item -ItemType Directory -Path $sourceDir -Force | Out-Null
        & $venvPython -c "from huggingface_hub import snapshot_download; snapshot_download(repo_id='$Model', local_dir=r'$sourceDir')"
        if ($LASTEXITCODE -ne 0) { throw "模型下载失败（需要网络）" }
    }
    $LocalModelDir = $sourceDir
    # 解析当前 commit 作为冻结 revision（未显式指定时）。
    $resolvedRevision = & $venvPython -c "from huggingface_hub import HfApi; print(HfApi().model_info('$Model').sha)"
    if ($LASTEXITCODE -ne 0) { throw "无法解析模型 revision" }
}
if ($Revision) {
    $resolvedRevision = $Revision
}
Write-Host "model_revision: $resolvedRevision"

# ── 3. CTranslate2 INT8 转换 ─────────────────────────────────────────
Write-Step "3/4 转换（quantization=int8）"
$converter = Join-Path $venvDir "Scripts\ct2-transformers-converter.exe"
if (-not (Test-Path $converter)) {
    # 部分 pip 版本安装为无扩展名入口，退回调用模块。
    $converter = Join-Path $venvDir "Scripts\ct2-transformers-converter"
}
$copyFiles = @("source.spm", "target.spm")
$copyArgs = $copyFiles | ForEach-Object { "--copy_files", $_ }

if (Test-Path $converter) {
    & $converter --model $LocalModelDir --output_dir $OutputDir --quantization int8 --force @copyArgs
} else {
    & $venvPython -m ctranslate2.converters.transformers --model $LocalModelDir --output_dir $OutputDir --quantization int8 --force @copyArgs
}
if ($LASTEXITCODE -ne 0) { throw "ct2-transformers-converter 转换失败" }

# Marian 模型 source/target 词表共享：转换器只产出
# shared_vocabulary.json。按 manifest v2 的 6 文件布局复制为
# source_vocabulary.json 与 target_vocabulary.json（内容相同；与 OCR
# rec 三槽位共享单文件同理，schema 不感知磁盘文件是否重复）。
if (Test-Path (Join-Path $OutputDir "shared_vocabulary.json")) {
    Copy-Item (Join-Path $OutputDir "shared_vocabulary.json") (Join-Path $OutputDir "source_vocabulary.json") -Force
    Copy-Item (Join-Path $OutputDir "shared_vocabulary.json") (Join-Path $OutputDir "target_vocabulary.json") -Force
}
# 转换器默认只复制 target.spm；source.spm 从源模型目录补拷。
if (-not (Test-Path (Join-Path $OutputDir "source.spm"))) {
    Copy-Item (Join-Path $LocalModelDir "source.spm") (Join-Path $OutputDir "source.spm")
}

# 只保留 schema 约定的 6 个文件；多余 HF 元数据（tokenizer_config.json、
# special_tokens_map.json、vocab.json）不随发布分发。
$expected = @("model.bin", "config.json", "source_vocabulary.json", "target_vocabulary.json", "source.spm", "target.spm")
foreach ($name in $expected) {
    if (-not (Test-Path (Join-Path $OutputDir $name))) {
        throw "转换产物缺失: $name"
    }
}
Get-ChildItem $OutputDir -File | Where-Object { $_.Name -notin $expected } | Remove-Item -Force

# 写入转换元数据。
$meta = [ordered]@{
    model            = $Model
    model_revision   = $resolvedRevision
    converted_with   = "ctranslate2 $CT2_VERSION"
    quantization     = "int8"
    output_dir       = $OutputDir
}
$meta | ConvertTo-Json | Set-Content -Path $metaPath -Encoding utf8
Write-Host "转换元数据: $metaPath"

# ── 4. 体积实测（≤110 MB） ─────────────────────────────────────────
Write-Step "4/4 体积审计（ja-zh ≤ 110 MB）"
& python (Join-Path $PSScriptRoot "audit_model_sizes.py") $OutputDir --max-mb 110
if ($LASTEXITCODE -ne 0) { throw "ja-zh 体积超限（>110 MB）" }

Write-Step "完成"
Write-Host "ja-zh INT8 模型已就绪: $OutputDir"
Write-Host "下一步：运行 .\scripts\translation\setup_translation_models.ps1 完成全流程（审计 + manifest 回填）。"
