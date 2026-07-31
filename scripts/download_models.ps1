# VTrans model download script
# Downloads and verifies the models required by docs/DEVELOPMENT.md (section 4).
# Model files are NOT committed to Git; run this script after cloning.
# Requires: network access, curl.exe (built into Windows 10 1803+ / Windows 11).
# Optional: Python 3.12+ with `pip install teradata-opus-translate` to (re)generate
# the local ONNX translation model.
$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$modelDir = Join-Path $repoRoot "src-tauri\resources\models"
$ocrDir = Join-Path $modelDir "ocr"
$translationDir = Join-Path $modelDir "translation"

$base = "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.9.2"

# name -> (relative url under $base, expected SHA-256)
$ocrFiles = [ordered]@{
    "det.onnx"    = @("onnx/PP-OCRv4/det/ch_PP-OCRv4_det_mobile.onnx", "d2a7720d45a54257208b1e13e36a8479894cb74155a5efe29462512d42f49da9")
    "rec_ja.onnx" = @("onnx/PP-OCRv4/rec/japan_PP-OCRv4_rec_mobile.onnx", "e1075a67dba758ecfc7ebc78a10ae61c95ac8fb66a9c86fab5541e33f085cb7a")
    "rec_en.onnx" = @("onnx/PP-OCRv4/rec/en_PP-OCRv4_rec_mobile.onnx", "e8770c967605983d1570cdf5352041dfb68fa0c21664f49f47b155abd3e0e318")
    "dict_ja.txt" = @("paddle/PP-OCRv4/rec/japan_PP-OCRv4_rec_mobile/japan_dict.txt", $null)
    "dict_en.txt" = @("paddle/PP-OCRv4/rec/en_PP-OCRv4_rec_mobile/en_dict.txt", $null)
}

Write-Host "VTrans Model Download Script"
Write-Host "Target: $modelDir"

foreach ($d in @($ocrDir, $translationDir)) {
    if (-not (Test-Path $d)) { New-Item -ItemType Directory -Path $d -Force | Out-Null }
}

function Download-File {
    param([string]$Url, [string]$OutFile, [string]$ExpectedSha)

    Write-Host "Downloading $(Split-Path $OutFile -Leaf) ..."
    curl.exe -sS -L --fail --retry 3 --connect-timeout 30 -o $OutFile $Url
    if ($LASTEXITCODE -ne 0) { throw "Download failed for $OutFile (curl exit $LASTEXITCODE)" }

    if ($ExpectedSha) {
        $actual = (Get-FileHash -Algorithm SHA256 -Path $OutFile).Hash.ToLower()
        if ($actual -ne $ExpectedSha) {
            throw "SHA-256 mismatch for $OutFile`n  expected: $ExpectedSha`n  actual:   $actual"
        }
        Write-Host "  verified sha256 $actual"
    }
}

Write-Host ""
Write-Host "== OCR models =="
foreach ($name in $ocrFiles.Keys) {
    $rel, $sha = $ocrFiles[$name]
    Download-File -Url "$base/$rel" -OutFile (Join-Path $ocrDir $name) -ExpectedSha $sha
}

Write-Host ""
Write-Host "== Local translation model (en -> zh-CN) =="
$modelOnnx = Join-Path $translationDir "model.onnx"
$tokenizerJson = Join-Path $translationDir "tokenizer.json"
if ((Test-Path $modelOnnx) -and (Test-Path $tokenizerJson)) {
    Write-Host "translation/model.onnx and translation/tokenizer.json already present; skipping."
}
else {
    $py = Get-Command python -ErrorAction SilentlyContinue
    if (-not $py) {
        Write-Host "Python not found. Place translation/model.onnx and translation/tokenizer.json manually."
        Write-Host "  Export command: pip install teradata-opus-translate"
    }
    else {
        python -c "import teradata_opus_translate" 2>$null
        if ($LASTEXITCODE -ne 0) {
            Write-Host "teradata-opus-translate not installed. Install it first:"
            Write-Host "  pip install teradata-opus-translate"
        }
        else {
            Write-Host "Exporting Helsinki-NLP/opus-mt-en-zh as single-file int8 ONNX (this downloads weights and runs parity verification) ..."
            @"
import pathlib
from teradata_opus_translate import convert_model, convert_tokenizer
outdir = pathlib.Path(r'$translationDir')
convert_model("Helsinki-NLP/opus-mt-en-zh", output_path=outdir / "model.onnx", precision="int8")
convert_tokenizer("Helsinki-NLP/opus-mt-en-zh", output_path=outdir / "tokenizer.json")
"@ | python -
            if ($LASTEXITCODE -ne 0) { throw "Translation model export failed (exit $LASTEXITCODE)" }
        }
    }
}

Write-Host ""
Write-Host "Done. Verify integrity with:"
Write-Host "  cargo run --bin vtrans-verify-models"

