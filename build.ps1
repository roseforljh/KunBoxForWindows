# KunBox Build Script
# 0 = Release (default), 1 = Debug

param(
    [string]$Mode
)

Write-Host ""
Write-Host "================================" -ForegroundColor Cyan
Write-Host "    KunBox Build Script" -ForegroundColor Cyan
Write-Host "================================" -ForegroundColor Cyan
Write-Host ""

# Get build mode
if (-not $Mode) {
    Write-Host "Select build mode:" -ForegroundColor Yellow
    Write-Host "  [0] Release (default)"
    Write-Host "  [1] Debug"
    Write-Host ""
    $input = Read-Host "Enter choice (0/1)"

    if ($input -eq "1") {
        $Mode = "debug"
    } else {
        $Mode = "release"
    }
}

$isDebug = $Mode -eq "debug" -or $Mode -eq "1"
$hasSigningKey = $false
$tauriArgs = @("tauri", "build")
$tempConfigPath = $null

if ($isDebug) {
    Write-Host "Building DEBUG version..." -ForegroundColor Yellow
} else {
    Write-Host "Building RELEASE version..." -ForegroundColor Green
}

Write-Host ""

# Load signing environment (for updater artifacts)
if (-not $env:TAURI_SIGNING_PRIVATE_KEY) {
    $localSigningScript = Join-Path $PSScriptRoot "tauri-signing.local.ps1"
    if (Test-Path $localSigningScript) {
        . $localSigningScript
    }
}

$hasSigningKey = -not [string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY)

if ($hasSigningKey) {
    if (-not $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) {
        Write-Host "TAURI_SIGNING_PRIVATE_KEY_PASSWORD not set; continuing (works only if key has no password)." -ForegroundColor Yellow
    }
    Write-Host "Signing key detected: release build will include updater artifacts." -ForegroundColor Green
} else {
    Write-Host "No TAURI_SIGNING_PRIVATE_KEY detected." -ForegroundColor Yellow
    Write-Host "Build will continue WITHOUT updater signing artifacts." -ForegroundColor Yellow

    $baseConfigPath = Join-Path $PSScriptRoot "src-tauri\tauri.conf.json"
    $tempConfigPath = Join-Path $env:TEMP "kunbox.tauri.unsigned.$PID.json"
    $config = Get-Content $baseConfigPath -Raw | ConvertFrom-Json
    if (-not $config.bundle) {
        $config | Add-Member -NotePropertyName bundle -NotePropertyValue ([pscustomobject]@{})
    }
    $config.bundle.createUpdaterArtifacts = $false
    $config | ConvertTo-Json -Depth 100 | Set-Content $tempConfigPath -Encoding UTF8
    $tauriArgs += @("--config", $tempConfigPath)
}

# Step 1: Build frontend
Write-Host "[1/2] Building frontend..." -ForegroundColor Cyan
Set-Location "$PSScriptRoot\kunbox-electron"

$frontendResult = & npm run build 2>&1
if ($LASTEXITCODE -ne 0) {
    Write-Host "Frontend build failed!" -ForegroundColor Red
    Write-Host $frontendResult
    Set-Location $PSScriptRoot
    if ($tempConfigPath -and (Test-Path $tempConfigPath)) { Remove-Item $tempConfigPath -Force }
    exit 1
}
Write-Host "Frontend build completed." -ForegroundColor Green

# Step 2: Build backend (Tauri)
Write-Host ""
Write-Host "[2/2] Building backend (Tauri)..." -ForegroundColor Cyan
Set-Location "$PSScriptRoot\src-tauri"

if ($isDebug) {
    $tauriArgs += "--debug"
}

$tauriResult = & cargo @tauriArgs 2>&1
$tauriExitCode = $LASTEXITCODE

if ($tempConfigPath -and (Test-Path $tempConfigPath)) {
    Remove-Item $tempConfigPath -Force
}

if ($tauriExitCode -ne 0) {
    Write-Host "Backend build failed!" -ForegroundColor Red
    Write-Host $tauriResult
    Set-Location $PSScriptRoot
    exit 1
}

Set-Location $PSScriptRoot

Write-Host ""
Write-Host "================================" -ForegroundColor Green
Write-Host "    Build completed!" -ForegroundColor Green
Write-Host "================================" -ForegroundColor Green
Write-Host ""

# Show output location
if ($isDebug) {
    Write-Host "Output: $PSScriptRoot\src-tauri\target\debug" -ForegroundColor Cyan
} else {
    Write-Host "Output directory: $PSScriptRoot\src-tauri\target\release\bundle" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "Installer packages:" -ForegroundColor Yellow
    Write-Host "  MSI:  $PSScriptRoot\src-tauri\target\release\bundle\msi\" -ForegroundColor White
    Write-Host "  NSIS: $PSScriptRoot\src-tauri\target\release\bundle\nsis\" -ForegroundColor White
    if (-not $hasSigningKey) {
        Write-Host ""
        Write-Host "Note: updater artifacts were skipped because no signing key was provided." -ForegroundColor Yellow
    }
}
Write-Host ""
