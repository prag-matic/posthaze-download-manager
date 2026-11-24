param(
    [string]$ExtensionId = "eiophahfcjidkfgdddgejjglglaopahi",
    [ValidateSet("debug", "release")]
    [string]$Profile = "debug"
)

$ErrorActionPreference = "Stop"

$defaultId = "eiophahfcjidkfgdddgejjglglaopahi"
$trimmedId = $ExtensionId.Trim().TrimStart("chrome-extension://").TrimEnd("/")
if ($trimmedId -notmatch '^[a-p]{32}$') {
    throw "ExtensionId must be 32 lowercase letters (a-p). Got: '$ExtensionId'"
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$hostExe = Join-Path $repoRoot "target\$Profile\posthaze-cli.exe"
if (-not (Test-Path $hostExe)) {
    throw "Host binary not found at $hostExe. Build it first with: cargo build -p posthaze-cli"
}

$wrapperDir = Join-Path $env:LOCALAPPDATA "posthaze"
New-Item -ItemType Directory -Force -Path $wrapperDir | Out-Null

# Remove legacy .cmd shim if upgrading from <= 0.2.1; Chromium invokes posthaze-cli.exe directly.
$legacyShimPath = Join-Path $wrapperDir "posthaze-native-host.cmd"
if (Test-Path $legacyShimPath) {
    Remove-Item -Path $legacyShimPath -Force
}

$allowedOrigins = @("chrome-extension://$trimmedId/")
if ($trimmedId -ne $defaultId) {
    $allowedOrigins += "chrome-extension://$defaultId/"
}

$manifestPath = Join-Path $wrapperDir "com.posthaze.host.json"
$manifest = @{
    name = "com.posthaze.host"
    description = "Posthaze Download Manager Native Messaging Host"
    path = $hostExe
    type = "stdio"
    allowed_origins = $allowedOrigins
} | ConvertTo-Json -Depth 3

Set-Content -Path $manifestPath -Value $manifest -Encoding UTF8

foreach ($regBase in @(
    "HKCU:\Software\Google\Chrome\NativeMessagingHosts",
    "HKCU:\Software\Microsoft\Edge\NativeMessagingHosts",
    "HKCU:\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts"
)) {
    $keyPath = Join-Path $regBase "com.posthaze.host"
    New-Item -Path $keyPath -Force | Out-Null
    Set-ItemProperty -Path $keyPath -Name "(Default)" -Value $manifestPath
}

Write-Host "Registered com.posthaze.host -> $manifestPath (Extension IDs: $($allowedOrigins -join ', '))"
