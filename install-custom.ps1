#!/usr/bin/env pwsh

# Install the custom Proton Pass CLI build from this repository's latest GitHub release.
# Usage: irm https://raw.githubusercontent.com/Winson-NoOT/pass-cli/main/install-custom.ps1 | iex
# Or with a custom install dir: $env:PROTON_PASS_CLI_INSTALL_DIR="C:\custom\path"; .\install-custom.ps1

$ErrorActionPreference = "Stop"

$REPO = "Winson-NoOT/pass-cli"
$BINARY_NAME = "pass-cli.exe"
$DOWNLOAD_URL = "https://github.com/$REPO/releases/latest/download/$BINARY_NAME"

function Write-Info {
    param([string]$Message)
    Write-Host "[INFO] $Message" -ForegroundColor Green
}

function Write-Error-Custom {
    param([string]$Message)
    Write-Host "[ERROR] $Message" -ForegroundColor Red
}

$installDir = $env:PROTON_PASS_CLI_INSTALL_DIR
if ([string]::IsNullOrEmpty($installDir)) {
    $installDir = Join-Path $env:LOCALAPPDATA "Programs\ProtonPass"
}

$target = Join-Path $installDir $BINARY_NAME

$running = Get-Process pass-cli -ErrorAction SilentlyContinue
if ($running) {
    Write-Error-Custom "pass-cli is running (PID: $($running.Id -join ', ')). Close it and re-run."
    exit 1
}

New-Item -ItemType Directory -Force -Path $installDir | Out-Null

Write-Info "Downloading $DOWNLOAD_URL"
try {
    Invoke-WebRequest -Uri $DOWNLOAD_URL -OutFile $target -UseBasicParsing
}
catch {
    Write-Error-Custom "Download failed: $($_.Exception.Message)"
    exit 1
}

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
$paths = @($userPath -split ';' | Where-Object { -not [string]::IsNullOrEmpty($_) })
if ($paths -notcontains $installDir) {
    [Environment]::SetEnvironmentVariable("Path", (($paths + $installDir) -join ';'), "User")
    Write-Info "Added $installDir to the user PATH. Open a new terminal to use it."
}

$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $target).Hash
Write-Info "Installed to $target"
Write-Info "SHA256: $hash"

& $target --version
