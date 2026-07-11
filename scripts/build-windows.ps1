$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$RootDir = Split-Path -Parent $PSScriptRoot
$DesktopDir = Join-Path $RootDir "apps/desktop"

Set-Location $DesktopDir

Write-Host "Installing frontend dependencies..."
npm ci

Write-Host "Building the Windows x64 NSIS installer..."
cargo tauri build --target x86_64-pc-windows-msvc --bundles nsis

Write-Host "Build complete: $RootDir\target\x86_64-pc-windows-msvc\release\bundle\nsis"
