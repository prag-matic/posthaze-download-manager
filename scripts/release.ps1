<#
.SYNOPSIS
Automates local release preparation for Posthaze Download Manager.

.DESCRIPTION
Bumps the workspace version across Cargo.toml, extensions/chrome/manifest.json,
and packaging/windows/posthaze.iss, runs all CI release gates locally
(check-repo.ps1, cargo doc -D warnings, cargo clippy -D warnings, cargo test),
and optionally builds the local Windows setup installer or commits, tags, and pushes.

.EXAMPLE
./scripts/release.ps1 -Version 0.2.1 -BuildLocal

.EXAMPLE
./scripts/release.ps1 -Version 0.2.1 -Push
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Version,

    [switch]$BuildLocal,
    [switch]$Push,
    [switch]$SkipGates
)

$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)

if ($Version -notmatch '^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$') {
    throw "Invalid SemVer version '$Version' (expected e.g. 0.2.1 or 0.3.0-rc.1)."
}

# Chrome manifest.json requires numeric X.Y.Z(.W) version (strip any -prerelease suffix)
$chromeVersion = ($Version -split '-')[0]

Write-Host "=== Updating release version to $Version ===" -ForegroundColor Cyan

# 1. Cargo.toml ([workspace.package] version)
$cargoPath = "Cargo.toml"
$cargoText = [System.IO.File]::ReadAllText((Resolve-Path $cargoPath))
$cargoUpdated = [regex]::Replace(
    $cargoText,
    '(?ms)(\[workspace\.package\].*?^version\s*=\s*")[^"]+(")',
    "`${1}$Version`${2}",
    1
)
if ($cargoUpdated -eq $cargoText -and $cargoText -notmatch "version\s*=\s*`"$([regex]::Escape($Version))`"") {
    throw "Failed to update version in $cargoPath"
}
[System.IO.File]::WriteAllText((Resolve-Path $cargoPath), $cargoUpdated, (New-Object System.Text.UTF8Encoding $false))

# 2. extensions/chrome/manifest.json
$manifestPath = "extensions\chrome\manifest.json"
$manifestText = [System.IO.File]::ReadAllText((Resolve-Path $manifestPath))
$manifestUpdated = [regex]::Replace(
    $manifestText,
    '("version"\s*:\s*")[^"]+(")',
    "`${1}$chromeVersion`${2}",
    1
)
[System.IO.File]::WriteAllText((Resolve-Path $manifestPath), $manifestUpdated, (New-Object System.Text.UTF8Encoding $false))

# 3. packaging/windows/posthaze.iss
$issPath = "packaging\windows\posthaze.iss"
$issText = [System.IO.File]::ReadAllText((Resolve-Path $issPath))
$issUpdated = [regex]::Replace(
    $issText,
    '(#define MyAppVersion ")[^"]+(")',
    "`${1}$Version`${2}",
    1
)
[System.IO.File]::WriteAllText((Resolve-Path $issPath), $issUpdated, (New-Object System.Text.UTF8Encoding $false))

Write-Host "Updated Cargo.toml, extensions/chrome/manifest.json, and packaging/windows/posthaze.iss."

# Refresh Cargo.lock
cargo update --workspace --offline 2>$null
if ($LASTEXITCODE -ne 0) {
    cargo check --workspace --quiet
}

if (-not $SkipGates) {
    Write-Host "=== Running local CI release gates ===" -ForegroundColor Cyan
    powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check-repo.ps1
    if ($LASTEXITCODE -ne 0) { throw "scripts/check-repo.ps1 failed" }

    $env:RUSTDOCFLAGS = "-D warnings"
    cargo doc --workspace --no-deps
    if ($LASTEXITCODE -ne 0) { throw "cargo doc failed" }

    cargo clippy --workspace --all-targets -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw "cargo clippy failed" }

    cargo test --workspace
    if ($LASTEXITCODE -ne 0) { throw "cargo test failed" }
}

if ($BuildLocal) {
    Write-Host "=== Building release binaries & local Windows setup ($Version) ===" -ForegroundColor Cyan
    cargo build --release --workspace
    if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed" }

    $iscc = Get-Command iscc -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -ErrorAction SilentlyContinue
    if (-not $iscc) {
        foreach ($candidate in @(
            "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
            "C:\Program Files (x86)\Inno Setup 6\ISCC.exe",
            "C:\Program Files\Inno Setup 6\ISCC.exe"
        )) {
            if (Test-Path $candidate) { $iscc = $candidate; break }
        }
    }
    if (-not $iscc) {
        throw "ISCC.exe (Inno Setup 6.7+) not found. Install Inno Setup 6.7+ to compile the local installer."
    }

    & $iscc /Qp "/DMyAppVersion=$Version" packaging\windows\posthaze.iss
    if ($LASTEXITCODE -ne 0) { throw "ISCC compilation failed" }
    Write-Host "Built dist\posthaze-$Version-windows-x64-setup.exe" -ForegroundColor Green
}

if ($Push) {
    $branch = (git branch --show-current).Trim()
    $tag = "v$Version"
    $confirm = Read-Host "Commit version bump on '$branch', create tag '$tag', and push to origin? (y/N)"
    if ($confirm -ne "y" -and $confirm -ne "Y") {
        Write-Host "Push cancelled."
        exit 0
    }
    git add Cargo.toml Cargo.lock extensions/chrome/manifest.json packaging/windows/posthaze.iss CHANGELOG.md 2>$null
    git commit -m "release: $tag"
    if ($LASTEXITCODE -ne 0) { throw "git commit failed" }
    git tag -a $tag -m "Release $tag"
    if ($LASTEXITCODE -ne 0) { throw "git tag failed" }
    git push origin $branch $tag
    if ($LASTEXITCODE -ne 0) { throw "git push failed" }
    Write-Host "Released $tag to origin!" -ForegroundColor Green
}
