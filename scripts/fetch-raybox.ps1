[CmdletBinding()]
param(
    [string]$OutputDirectory
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $PSScriptRoot "..\src-tauri\resources\libs\raybox"
}

$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$lockPath = Join-Path $root "src-tauri\resources\libs\raybox-windows.lock.json"
$lock = Get-Content $lockPath -Raw | ConvertFrom-Json
$legacyIdentity = $lock.commit -cmatch '^[0-9a-f]{40}$' -and
    $lock.tag -ceq "windows-$($lock.commit)" -and $lock.version -ceq "1.0.0-$($lock.commit.Substring(0, 12))"
$semanticIdentity = $lock.version -cmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' -and
    $lock.tag -ceq "windows-v$($lock.version)"
if ($lock.repository -ne "roseforljh/RayBox-rx" -or
    $lock.commit -notmatch '^[0-9a-f]{40}$' -or
    -not ($legacyIdentity -or $semanticIdentity) -or
    $lock.product -ne "RayBox" -or
    $lock.manifestSha256 -notmatch '^[0-9a-f]{64}$' -or
    $lock.sourceDigest -notmatch '^[0-9a-f]{64}$') {
    throw "RayBox Windows lock is invalid"
}

function Get-Sha256([string]$Path) {
    $sha = [Security.Cryptography.SHA256]::Create()
    $stream = [IO.File]::OpenRead($Path)
    try { return ([BitConverter]::ToString($sha.ComputeHash($stream))).Replace('-', '').ToLowerInvariant() }
    finally { $stream.Dispose(); $sha.Dispose() }
}

function Assert-Release([string]$Directory) {
    $manifestPath = Join-Path $Directory "raybox-windows-manifest.json"
    $executablePath = Join-Path $Directory "raybox-windows-amd64.exe"
    if (-not (Test-Path -LiteralPath $manifestPath) -or -not (Test-Path -LiteralPath $executablePath)) {
        return $false
    }
    $manifestBytes = [IO.File]::ReadAllBytes($manifestPath)
    if ((Get-Sha256 $manifestPath) -ne $lock.manifestSha256) { throw "RayBox manifest SHA-256 mismatch" }
    $manifest = [Text.Encoding]::UTF8.GetString($manifestBytes) | ConvertFrom-Json
    if ($manifest.commit -ne $lock.commit -or $manifest.tag -ne $lock.tag -or
        $manifest.sourceDigest -ne $lock.sourceDigest -or $manifest.product -ne $lock.product -or
        $manifest.version -ne $lock.version -or $manifest.dirty) {
        throw "RayBox manifest does not match the trusted lock"
    }
    foreach ($name in $manifest.assets.PSObject.Properties.Name) {
        $asset = $manifest.assets.$name
        if ($asset.file -cne $name -or $name -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]*$') {
            throw "Invalid RayBox asset path"
        }
        $path = Join-Path $Directory $asset.file
        if (-not (Test-Path -LiteralPath $path)) { throw "Missing RayBox asset: $($asset.file)" }
        $item = Get-Item -LiteralPath $path
        if ($item.Length -ne [int64]$asset.size -or (Get-Sha256 $path) -ne $asset.sha256) {
            throw "RayBox asset checksum mismatch: $($asset.file)"
        }
    }
    if ((Get-Sha256 $executablePath) -ne $manifest.assets.'raybox-windows-amd64.exe'.sha256) {
        throw "RayBox executable checksum mismatch"
    }
    return $true
}

if (Test-Path -LiteralPath $OutputDirectory) {
    if (Assert-Release $OutputDirectory) {
        Write-Host "RayBox $($lock.version) already verified at $OutputDirectory"
        return
    }
    throw "Existing RayBox directory is present but does not match the trusted release"
}

$temp = Join-Path ([IO.Path]::GetTempPath()) "kunbox-raybox-$PID"
New-Item -ItemType Directory -Path $temp | Out-Null
try {
    $ref = & gh api "repos/$($lock.repository)/git/ref/tags/$($lock.tag)" | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw "Failed to resolve RayBox release tag" }
    while ($ref.object.type -eq 'tag') {
        $ref = & gh api "repos/$($lock.repository)/git/tags/$($ref.object.sha)" | ConvertFrom-Json
        if ($LASTEXITCODE -ne 0) { throw "Failed to resolve annotated RayBox tag" }
    }
    if ($ref.object.type -ne 'commit' -or $ref.object.sha -ne $lock.commit) {
        throw "RayBox tag does not point to the locked commit"
    }
    & gh release download $lock.tag --repo $lock.repository --dir $temp --pattern "manifest.json"
    if ($LASTEXITCODE -ne 0) { throw "Failed to download RayBox manifest" }
    $manifestPath = Join-Path $temp "manifest.json"
    if ((Get-Sha256 $manifestPath) -ne $lock.manifestSha256) { throw "RayBox manifest SHA-256 mismatch" }
    $manifest = Get-Content $manifestPath -Raw | ConvertFrom-Json
    foreach ($name in $manifest.assets.PSObject.Properties.Name) {
        if ($name -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]*$' -or $manifest.assets.$name.file -cne $name) {
            throw "Invalid RayBox asset path"
        }
        & gh release download $lock.tag --repo $lock.repository --dir $temp --pattern $name
        if ($LASTEXITCODE -ne 0) { throw "Failed to download RayBox asset: $name" }
    }
    Rename-Item (Join-Path $temp "manifest.json") (Join-Path $temp "raybox-windows-manifest.json")
    if (-not (Assert-Release $temp)) { throw "Downloaded RayBox release failed validation" }
    New-Item -ItemType Directory -Path (Split-Path $OutputDirectory -Parent) -Force | Out-Null
    Move-Item -LiteralPath $temp -Destination $OutputDirectory
    Write-Host "Downloaded and verified RayBox $($lock.version)"
} finally {
    if (Test-Path -LiteralPath $temp) { Remove-Item -LiteralPath $temp -Recurse -Force }
}
