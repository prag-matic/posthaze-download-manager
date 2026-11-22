# Repo invariants that cargo does not check. Run by CI, the pre-commit hook, and the release workflow.
$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)
$failures = @()

# 1. Version: Cargo.toml [workspace.package] is the single source of truth.
$cargo = Get-Content Cargo.toml -Raw
if ($cargo -notmatch '(?ms)\[workspace\.package\].*?^version\s*=\s*"([^"]+)"') { throw "workspace version not found" }
$version = $Matches[1]
$manifest = (Get-Content extensions/chrome/manifest.json -Raw | ConvertFrom-Json).version
if ($manifest -ne $version) { $failures += "extensions/chrome/manifest.json version $manifest != Cargo.toml $version" }
$iss = Get-Content packaging/windows/posthaze.iss -Raw
if ($iss -notmatch '#define MyAppVersion "([^"]+)"' -or $Matches[1] -ne $version) {
    $failures += "packaging/windows/posthaze.iss default MyAppVersion != Cargo.toml $version"
}

# 2. SQLx hashes migration bytes, so CRLF in a checkout breaks existing databases.
Get-ChildItem crates/posthaze-engine/migrations -Filter *.sql | ForEach-Object {
    if ([System.IO.File]::ReadAllText($_.FullName).Contains("`r`n")) { $failures += "$($_.Name) contains CRLF; migrations must be LF" }
}

# 3. Windows integration goes through Win32 APIs; shelling out broke quoting and default-app handling.
Get-ChildItem crates -Recurse -Filter *.rs | Where-Object { $_.FullName -notmatch '\\target\\' } | ForEach-Object {
    $text = Get-Content $_.FullName -Raw
    # explorer.exe is legitimate only for revealing folders (`/select,`), which lives in platform_hooks.rs.
    if ($text -match 'Command::new\("explorer(\.exe)?"\)' -and $_.Name -ne "platform_hooks.rs") { $failures += "$($_.Name): use ShellExecuteW instead of explorer.exe" }
    if ($text -match 'Command::new\("reg(\.exe)?"\)') {
        $failures += "$($_.Name): use the Win32 Registry API instead of reg.exe"
    }
}

if ($failures) { $failures | ForEach-Object { Write-Error $_ -ErrorAction Continue }; exit 1 }
Write-Host "repo checks passed (version $version)"
