<#
.SYNOPSIS
    Builds translation_bridge.dll (Bergamot + CTranslate2 + SentencePiece)
    and copies it to src-tauri/resources/native/.

.DESCRIPTION
    The three engine libraries must be built beforehand with the pinned
    versions listed in README.md. This script locates their include and
    library directories under -DepsRoot, configures CMake, builds the DLL
    and copies it to the app resource directory used by the Rust crate.

.PARAMETER DepsRoot
    Root directory containing pre-built engine artifacts:
        <DepsRoot>/bergamot        (bergamot-translator v0.4.5)
        <DepsRoot>/ctranslate2     (CTranslate2 4.8.1)
        <DepsRoot>/sentencepiece   (SentencePiece, pinned revision)
    Each entry must expose include/ and lib/ (or lib64/) subdirectories.

.PARAMETER OutputDir
    Destination directory for translation_bridge.dll. Defaults to
    <repo>/src-tauri/resources/native.

.PARAMETER BuildDir
    CMake build directory. Defaults to <repo>/native/translation_bridge/build.

.EXAMPLE
    .\native\translation_bridge\build.ps1 -DepsRoot D:\deps\translation
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$DepsRoot,

    [string]$OutputDir = "",
    [string]$BuildDir = ""
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $OutputDir) {
    $OutputDir = Join-Path $repoRoot "src-tauri\resources\native"
}
if (-not $BuildDir) {
    $BuildDir = Join-Path $PSScriptRoot "build"
}

function Resolve-Dep {
    param([string]$Name)
    $dir = Join-Path $DepsRoot $Name
    if (-not (Test-Path $dir)) {
        throw "Missing dependency directory: $dir (see native/translation_bridge/README.md)"
    }
    return $dir
}

function Resolve-LibDir {
    param([string]$DepDir)
    foreach ($candidate in @("lib64", "lib")) {
        $path = Join-Path $DepDir $candidate
        if (Test-Path $path) {
            return $path
        }
    }
    throw "Missing lib directory under $DepDir"
}

function Get-Libraries {
    param([string]$LibDir)
    $patterns = @("*.lib", "*.a", "*.so", "*.dll.a")
    $libs = @()
    foreach ($pattern in $patterns) {
        $libs += Get-ChildItem -Path $LibDir -Filter $pattern -File |
            Select-Object -ExpandProperty FullName
    }
    if ($libs.Count -eq 0) {
        throw "No static/shared libraries found in $LibDir"
    }
    return $libs
}

$bergamot = Resolve-Dep "bergamot"
$ctranslate2 = Resolve-Dep "ctranslate2"
$sentencepiece = Resolve-Dep "sentencepiece"

$bergamotInc = Join-Path $bergamot "include"
$ct2Inc = Join-Path $ctranslate2 "include"
$spmInc = Join-Path $sentencepiece "include"
foreach ($inc in @($bergamotInc, $ct2Inc, $spmInc)) {
    if (-not (Test-Path $inc)) {
        throw "Missing include directory: $inc"
    }
}

$bergamotLib = Get-Libraries (Resolve-LibDir $bergamot)
$ct2Lib = Get-Libraries (Resolve-LibDir $ctranslate2)
$spmLib = Get-Libraries (Resolve-LibDir $sentencepiece)

$cmake = Get-Command cmake -ErrorAction Stop

New-Item -ItemType Directory -Force -Path $BuildDir | Out-Null
& $cmake.Source -S $PSScriptRoot -B $BuildDir `
    -DCMAKE_BUILD_TYPE=Release `
    "-DBERGAMOT_INCLUDE_DIRS=$bergamotInc" `
    "-DBERGAMOT_LIBRARIES=$($bergamotLib -join ';')" `
    "-DCTRANSLATE2_INCLUDE_DIRS=$ct2Inc" `
    "-DCTRANSLATE2_LIBRARIES=$($ct2Lib -join ';')" `
    "-DSENTENCEPIECE_INCLUDE_DIRS=$spmInc" `
    "-DSENTENCEPIECE_LIBRARIES=$($spmLib -join ';')" `
    "-DTRANSLATION_BRIDGE_OUTPUT_DIR=$OutputDir"
if ($LASTEXITCODE -ne 0) {
    throw "CMake configure failed with exit code $LASTEXITCODE"
}

& $cmake.Source --build $BuildDir --config Release
if ($LASTEXITCODE -ne 0) {
    throw "CMake build failed with exit code $LASTEXITCODE"
}

$dll = Join-Path $BuildDir "Release\translation_bridge.dll"
if (-not (Test-Path $dll)) {
    $dll = Join-Path $BuildDir "translation_bridge.dll"
}
if (-not (Test-Path $dll)) {
    throw "Build finished but translation_bridge.dll was not found under $BuildDir"
}

Write-Host "translation_bridge.dll -> $OutputDir"
Write-Host "Built with:"
Write-Host "  Bergamot      $bergamot"
Write-Host "  CTranslate2   $ctranslate2"
Write-Host "  SentencePiece $sentencepiece"
