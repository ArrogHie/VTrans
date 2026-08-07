# VTrans 翻译模型准备总入口（方案 B，Windows 优先）
#
# 完整流程：下载 en-zh（Bergamot）→ 转换 ja-zh（CTranslate2 INT8）→
#           体积审计（200 MB 门禁）→ manifest.json 回填（SHA-256）。
#
# 产物：
#   - src-tauri/resources/models/translation/en-zh/  （4 个 Bergamot 文件）
#   - src-tauri/resources/models/translation/ja-zh/  （6 个 CTranslate2 文件）
#   - src-tauri/resources/models/manifest.json        （v2，translation 段回填）
#   - scripts/translation/work/                       （venv / 中间产物，git 忽略）
#
# 开发机要求（见 docs/DEVELOPMENT.md §4）：
#   - Python 3.10+，网络（下载模型与转换依赖）
#   - 首次运行会创建 venv 并安装 ctranslate2==4.8.1 + torch + transformers
#   - 转换需要网络访问 Hugging Face
#
# 用法：
#   .\scripts\translation\setup_translation_models.ps1                # 全流程
#   .\scripts\translation\setup_translation_models.ps1 -SkipEnZh      # 仅 ja-zh（已下载 en-zh）
#   .\scripts\translation\setup_translation_models.ps1 -SkipJaZh      # 仅 en-zh（已转换 ja-zh）
#   .\scripts\translation\setup_translation_models.ps1 -SkipAudit     # 跳过体积审计（不建议）
#   .\scripts\translation\setup_translation_models.ps1 -SkipInstall   # 复用已存在的 venv
#
# 退出码：0 全流程成功；非 0 任一步骤失败（下载 / 转换 / 审计 / 回填）。
[CmdletBinding()]
param(
    [switch]$SkipEnZh,
    [switch]$SkipJaZh,
    [switch]$SkipAudit,
    [switch]$SkipInstall,
    [switch]$CleanWork
)

$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$workDir = Join-Path $PSScriptRoot "work"
$modelsDir = Join-Path $repoRoot "src-tauri\resources\models"
$translationDir = Join-Path $modelsDir "translation"
$manifestPath = Join-Path $modelsDir "manifest.json"

function Write-Step([string]$Title) {
    Write-Host ""
    Write-Host "==== $Title ====" -ForegroundColor Cyan
}

function Assert-Command([string]$Name) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "required command not found: $Name"
    }
}

Write-Step "翻译模型准备（en-zh Bergamot + ja-zh CTranslate2 INT8）"
Write-Host "repo: $repoRoot"

New-Item -ItemType Directory -Path $workDir -Force | Out-Null
New-Item -ItemType Directory -Path $translationDir -Force | Out-Null
if ($CleanWork) {
    Get-ChildItem $workDir -Force | Remove-Item -Recurse -Force
    New-Item -ItemType Directory -Path $workDir -Force | Out-Null
}

Assert-Command "python"

# ── 1. en-zh：Mozilla registry 下载 + SHA-256 校验 ──────────────────
if (-not $SkipEnZh) {
    Write-Step "1/4 下载 en-zh（Bergamot，Mozilla Release base-memory）"
    & python (Join-Path $PSScriptRoot "fetch_firefox_enzh.py") `
        --download `
        --output (Join-Path $translationDir "en-zh")
    if ($LASTEXITCODE -ne 0) { throw "en-zh 下载/校验失败" }
} else {
    Write-Step "1/4 en-zh 下载已跳过（-SkipEnZh）"
    $enZhDownload = Join-Path $translationDir "en-zh\manifest.json"
    if (-not (Test-Path $enZhDownload)) {
        Write-Host "WARN: 未找到 $enZhDownload，回填时将不写入 registry 元数据" -ForegroundColor Yellow
    }
}

# ── 2. ja-zh：HF 下载 + CTranslate2 INT8 转换 + 体积实测 ────────────
if (-not $SkipJaZh) {
    Write-Step "2/4 转换 ja-zh（shun89/opus-mt-ja-zh → CTranslate2 INT8）"
    $convertArgs = @()
    if ($SkipInstall) { $convertArgs += "-SkipInstall" }
    & (Join-Path $PSScriptRoot "convert_ja_zh_ct2.ps1") @convertArgs
    if ($LASTEXITCODE -ne 0) { throw "ja-zh 转换失败" }
} else {
    Write-Step "2/4 ja-zh 转换已跳过（-SkipJaZh）"
}

# ── 3. 体积审计（200 MB 门禁） ─────────────────────────────────────
if (-not $SkipAudit) {
    Write-Step "3/4 体积审计（en-zh ≤ 65 / ja-zh ≤ 110 / 总 ≤ 200 MB）"
    & python (Join-Path $PSScriptRoot "audit_model_sizes.py") `
        $translationDir `
        --manifest $manifestPath
    if ($LASTEXITCODE -ne 0) { throw "体积门禁未通过（见上方明细）" }
} else {
    Write-Step "3/4 体积审计已跳过（-SkipAudit，不建议）"
}

# ── 4. manifest 回填（SHA-256 / size_bytes / 元数据） ───────────────
Write-Step "4/4 回填 manifest.json"
& python (Join-Path $PSScriptRoot "backfill_translation_manifest.py") `
    --manifest $manifestPath `
    --en-zh-dir (Join-Path $translationDir "en-zh") `
    --ja-zh-dir (Join-Path $translationDir "ja-zh") `
    --update-template
if ($LASTEXITCODE -ne 0) { throw "manifest 回填失败" }

Write-Step "完成"
Write-Host "下一步：cargo run --bin vtrans-verify-models -- --models src-tauri/resources/models"
Write-Host "验证 CLI 输出 'all model files are valid' 即通过。"
