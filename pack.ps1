# DbLens one-click package script
# 1) cargo build --release
# 2) Inno Setup compile installer
#
# Usage:
#   .\pack.ps1
#   .\pack.ps1 -SkipBuild

param(
    [switch]$SkipBuild,
    [string]$Configuration = "release"
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $Root

function Find-ISCC {
    $candidates = @(
        (Get-Command iscc -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source),
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
        "${env:ProgramFiles(x86)}\Inno Setup 5\ISCC.exe"
    ) | Where-Object { $_ -and (Test-Path $_) }
    if (-not $candidates) {
        throw "未找到 Inno Setup 编译器 ISCC.exe。请安装 Inno Setup 6：https://jrsoftware.org/isinfo.php"
    }
    return $candidates[0]
}

Write-Host "==> DbLens 打包" -ForegroundColor Cyan
Write-Host "    目录: $Root"

# Ensure icons exist
$ico = Join-Path $Root "assets\dblens.ico"
$png = Join-Path $Root "assets\dblens.png"
if (-not (Test-Path $ico) -or -not (Test-Path $png)) {
    Write-Host "==> 生成图标..." -ForegroundColor Yellow
    python (Join-Path $Root "scripts\gen_icon.py")
    if ($LASTEXITCODE -ne 0) { throw "图标生成失败" }
}

if (-not $SkipBuild) {
    Write-Host "==> cargo build --$Configuration" -ForegroundColor Cyan
    cargo build --$Configuration
    if ($LASTEXITCODE -ne 0) { throw "cargo build 失败" }
}

$exe = Join-Path $Root "target\$Configuration\dblens.exe"
if (-not (Test-Path $exe)) {
    throw "找不到可执行文件: $exe"
}

$dist = Join-Path $Root "dist"
New-Item -ItemType Directory -Force -Path $dist | Out-Null

# Copy portable exe alongside installer
$portable = Join-Path $dist "dblens.exe"
Copy-Item $exe $portable -Force
Write-Host "==> 已复制便携版: $portable" -ForegroundColor Green

$iscc = Find-ISCC
$iss = Join-Path $Root "packaging\dblens.iss"
Write-Host "==> Inno Setup: $iscc" -ForegroundColor Cyan
Write-Host "    脚本: $iss"

& $iscc $iss
if ($LASTEXITCODE -ne 0) { throw "Inno Setup 编译失败" }

Write-Host ""
Write-Host "==> 打包完成" -ForegroundColor Green
Get-ChildItem $dist | Sort-Object LastWriteTime -Descending | Format-Table Name, Length, LastWriteTime -AutoSize
Write-Host "安装包目录: $dist" -ForegroundColor Green
