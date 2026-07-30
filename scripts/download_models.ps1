# VTrans model download script
$ErrorActionPreference = "Stop"
$modelDir = "src-tauri/resources/models"
Write-Host "VTrans Model Download Script"
Write-Host "Target: $modelDir"
$dirs = @("$modelDir/ocr", "$modelDir/translation")
foreach ($d in $dirs) {
    if (-not (Test-Path $d)) { New-Item -ItemType Directory -Path $d -Force | Out-Null }
}
Write-Host "Place model files manually. Required files:"
Write-Host "  manifest.json, ocr/det.onnx, ocr/rec_ja.onnx, ocr/rec_en.onnx"
Write-Host "  ocr/dict_ja.txt, ocr/dict_en.txt"
Write-Host "  translation/model.onnx, translation/tokenizer.json"
