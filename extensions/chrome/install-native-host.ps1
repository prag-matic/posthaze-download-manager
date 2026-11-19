param(
    [Parameter(Mandatory = $true)]
    [string]$ExtensionId,
    [ValidateSet("debug", "release")]
    [string]$Profile = "debug"
)

$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$hostExe = Join-Path $repoRoot "target\$Profile\posthaze-cli.exe"
if (-not (Test-Path $hostExe)) {
    throw "Host binary not found at $hostExe. Build it first with: cargo build -p posthaze-cli"
}

$wrapperDir = Join-Path $env:LOCALAPPDATA "posthaze"
New-Item -ItemType Directory -Force -Path $wrapperDir | Out-Null

# Chrome Native Messaging does not pass CLI subcommands in manifest `path`,
# so generate a tiny .cmd shim that invokes `posthaze-cli.exe native-host`.
$shimPath = Join-Path $wrapperDir "posthaze-native-host.cmd"
@"
@echo off
"$hostExe" native-host %*
"@ | Set-Content -Path $shimPath -Encoding ASCII

$manifestPath = Join-Path $wrapperDir "com.posthaze.host.json"
$manifest = @{
    name = "com.posthaze.host"
    description = "Posthaze Download Manager Native Messaging Host"
    path = $shimPath
    type = "stdio"
    allowed_origins = @("chrome-extension://$ExtensionId/")
} | ConvertTo-Json -Depth 3

Set-Content -Path $manifestPath -Value $manifest -Encoding UTF8

foreach ($regBase in @(
    "HKCU:\Software\Google\Chrome\NativeMessagingHosts",
    "HKCU:\Software\Microsoft\Edge\NativeMessagingHosts"
)) {
    $keyPath = Join-Path $regBase "com.posthaze.host"
    New-Item -Path $keyPath -Force | Out-Null
    Set-ItemProperty -Path $keyPath -Name "(Default)" -Value $manifestPath
}

Write-Host "Registered com.posthaze.host -> $manifestPath (Extension ID: $ExtensionId)"
