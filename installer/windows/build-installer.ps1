# Builds LightScio-Setup-<version>.exe in target\installer.
# Version: LIGHTSCIO_VERSION env var, else the latest git tag (v1.2.3 -> 1.2.3), else Cargo.toml.
# Requires Inno Setup 6:  winget install JRSoftware.InnoSetup
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..\..')

$ver = $env:LIGHTSCIO_VERSION
if (-not $ver) {
    $tag = (git describe --tags --abbrev=0 2>$null)
    if ($LASTEXITCODE -eq 0 -and $tag) { $ver = $tag.Trim().TrimStart('v', 'V') }
}
if (-not $ver) {
    $ver = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
}
$env:LIGHTSCIO_VERSION = $ver
Write-Host "LightScio $ver"

cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$iscc = @(
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw "Inno Setup 6 not found. Install it with: winget install JRSoftware.InnoSetup" }

& $iscc "/DMyAppVersion=$ver" "installer\windows\LightScio.iss"
if ($LASTEXITCODE -ne 0) { throw "ISCC failed" }
Get-ChildItem target\installer\*.exe | Select-Object Name, Length
