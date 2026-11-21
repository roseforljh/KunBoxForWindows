# KunBox Build Script (Electron)
param(
    [string]$Mode
)

Write-Host ""
Write-Host "================================" -ForegroundColor Cyan
Write-Host "    KunBox Build Script (Electron)" -ForegroundColor Cyan
Write-Host "================================" -ForegroundColor Cyan
Write-Host ""

if (-not $Mode) {
    Write-Host "Select build target:" -ForegroundColor Yellow
    Write-Host "  [0] NSIS Installer (default)"
    Write-Host "  [1] Portable"
    Write-Host ""
    $choice = Read-Host "Enter choice (0/1)"

    if ($choice -eq "1") {
        $Mode = "portable"
    } else {
        $Mode = "installer"
    }
}

$buildScript = if ($Mode -eq "portable" -or $Mode -eq "1") { "build:portable" } else { "build:win" }

$electronDir = Join-Path $PSScriptRoot "kunbox-electron"

Write-Host "Building KunBox with Electron ($buildScript)..." -ForegroundColor Green

cmd /c "cd /d `"$electronDir`" && npm run $buildScript"

if ($LASTEXITCODE -ne 0) {
    Write-Host "Build failed with exit code $LASTEXITCODE" -ForegroundColor Red
    exit $LASTEXITCODE
}

Write-Host ""
Write-Host "Build completed successfully!" -ForegroundColor Green
