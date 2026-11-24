# Builds the Windows packages:
#   dist/RSQuake-<version>-windows-x64.zip
#   dist/RSQuake-<version>-windows-x64-installer.msi   (needs the WiX CLI: dotnet tool install --global wix --version 5.0.2)
#   powershell -File scripts/package-windows.ps1
# Pass -SkipBuild if the release build already exists.
# Pass -SkipMsi to build only the zip.
param([switch]$SkipBuild, [switch]$SkipMsi)
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

# MSI のバージョンは数字だけなので、v0.2.0-pre.1 と v0.2.0 が同じ 0.2.0 になって
# アップグレードできなくなる。プレリリース (version に - を含む) は MSI を作らず zip だけにする。
if (-not $SkipMsi -and $version.Contains('-')) {
    Write-Host "pre-release $version: skipping MSI"
    $SkipMsi = $true
}

if (-not $SkipMsi) {
    if (-not (Get-Command wix -ErrorAction SilentlyContinue)) {
        throw "wix not found. Install it with: dotnet tool install --global wix --version 5.0.2 (or pass -SkipMsi)"
    }
    # MSI versions must be numeric (major.minor.build).
    $msiVersion = ($version -split '[-+]')[0]
    $msi = Join-Path $root "dist\$name-installer.msi"
    if (Test-Path $msi) { Remove-Item -LiteralPath $msi -Force }
    wix build wix\main.wxs `
        -arch x64 `
        -d "Version=$msiVersion" `
        -d "ExePath=$(Join-Path $stage 'RSQuake.exe')" `
        -d "IconPath=$(Join-Path $root 'assets\icon\icon.ico')" `
        -o $msi
    if ($LASTEXITCODE -ne 0) { throw "wix build failed" }
    Write-Host "created $msi"
}
