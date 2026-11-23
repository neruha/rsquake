# Builds the Windows package (dist/RSQuake-<version>-windows-x64.zip).
#   powershell -File scripts/package-windows.ps1
# Pass -SkipBuild if the release build already exists.
param([switch]$SkipBuild)
$ErrorActionPreference = "Stop"

$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location $root

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
if (-not $SkipBuild) {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}

$name = "RSQuake-$version-windows-x64"
$stage = Join-Path $root "dist\$name"
if (Test-Path $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
New-Item -ItemType Directory -Force $stage | Out-Null

Copy-Item target\release\rsquake.exe (Join-Path $stage "RSQuake.exe")
$zip = Join-Path $root "dist\$name.zip"
if (Test-Path $zip) { Remove-Item -LiteralPath $zip -Force }
Compress-Archive -Path $stage -DestinationPath $zip
Write-Host "created $zip"
