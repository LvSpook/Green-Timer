# Build Green Timer into a distributable Windows executable.
#
#   powershell -ExecutionPolicy Bypass -File build.ps1
#
# Steps: run the tests, build a release binary with Cargo, stage the
# executable and its resources, then (if Inno Setup is installed) compile
# the installer.

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
Push-Location $root
try {
    Write-Host "==> Running tests" -ForegroundColor Cyan
    cargo test
    if ($LASTEXITCODE -ne 0) { throw "Tests failed" }

    Write-Host "==> Building release binary" -ForegroundColor Cyan
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "Cargo build failed" }

    $built = Join-Path $root "target\release\GreenTimer.exe"
    if (-not (Test-Path $built)) { throw "Expected executable not found: $built" }

    $stage = Join-Path $root "dist\GreenTimer"
    New-Item -ItemType Directory -Force -Path $stage | Out-Null
    Copy-Item $built (Join-Path $stage "GreenTimer.exe") -Force
    Copy-Item (Join-Path $root "green_timer.ico") $stage -Force
    Write-Host "Build complete: $stage\GreenTimer.exe" -ForegroundColor Green

    $iscc = Get-Command iscc -ErrorAction SilentlyContinue
    if ($iscc) {
        Write-Host "==> Compiling installer" -ForegroundColor Cyan
        & $iscc.Source "installer.iss"
        if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed" }
        Write-Host "Installer written to Output\" -ForegroundColor Green
    } else {
        Write-Host "Inno Setup (iscc) not found; skipping installer." -ForegroundColor Yellow
    }
}
finally {
    Pop-Location
}
