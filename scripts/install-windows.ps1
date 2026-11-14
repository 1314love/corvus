# Install or upgrade Corvus as a Windows service.
#
# Run from an elevated PowerShell. The script is idempotent: it stops a
# running service, copies the release binaries and extension, updates the
# service configuration, and starts the service again.
[CmdletBinding()]
param(
    [string]$ServiceName = "Corvus",
    [string]$InstallDir = (Join-Path $env:USERPROFILE ".corvus\bin"),
    [string]$DataDir = (Join-Path $env:USERPROFILE ".corvus"),
    [string]$ExtensionDir = (Join-Path $env:USERPROFILE ".corvus\extension"),
    [string]$NssmPath = "",
    [string]$DistDir = "",
    [int]$WsPort = 0,
    [int]$HttpPort = 0
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Stop-WithError {
    param([string]$Message)
    throw "install-windows: $Message"
}

function Get-AbsolutePath {
    param([string]$Path)
    if ([string]::IsNullOrWhiteSpace($Path)) {
        Stop-WithError "a path parameter is empty"
    }
    $resolved = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Path)
    return [System.IO.Path]::GetFullPath($resolved)
}

function Test-Administrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Invoke-Nssm {
    param([string[]]$NssmArguments)
    & $script:NssmCommand @NssmArguments
    if ($LASTEXITCODE -ne 0) {
        Stop-WithError ("nssm {0} failed with exit code {1}" -f ($NssmArguments -join " "), $LASTEXITCODE)
    }
}

function Wait-ServiceStatus {
    param(
        [string]$Name,
        [string]$Status,
        [int]$TimeoutSeconds
    )
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $service = Get-Service -Name $Name -ErrorAction SilentlyContinue
        if ($null -eq $service) {
            return $false
        }
        if ($service.Status.ToString() -eq $Status) {
            return $true
        }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    return $false
}

function Show-LogTail {
    param([string]$Path)
    if (Test-Path -LiteralPath $Path -PathType Leaf) {
        Get-Content -LiteralPath $Path -Tail 20 | ForEach-Object { Write-Host "  $_" }
    }
}

if (-not (Test-Administrator)) {
    Stop-WithError "run this script from an elevated PowerShell session"
}
if ($ServiceName -notmatch '^[A-Za-z0-9_.-]+$' -or $ServiceName.Length -gt 64) {
    Stop-WithError "ServiceName must contain only letters, digits, dot, underscore and hyphen"
}
if ($WsPort -lt 0 -or $WsPort -gt 65535) {
    Stop-WithError "WsPort must be between 0 and 65535"
}
if ($HttpPort -lt 0 -or $HttpPort -gt 65535) {
    Stop-WithError "HttpPort must be between 0 and 65535"
}

$InstallDir = (Get-AbsolutePath $InstallDir).TrimEnd([char]'\')
$DataDir = (Get-AbsolutePath $DataDir).TrimEnd([char]'\')
$ExtensionDir = (Get-AbsolutePath $ExtensionDir).TrimEnd([char]'\')
$repoRoot = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($DistDir)) {
    $DistDir = Join-Path $repoRoot "dist"
}
$DistDir = (Get-AbsolutePath $DistDir).TrimEnd([char]'\')

if ($InstallDir -eq $DataDir) {
    Stop-WithError "InstallDir and DataDir must be different directories"
}
if ($ExtensionDir -eq $DataDir -or $ExtensionDir -eq $InstallDir) {
    Stop-WithError "ExtensionDir must be different from DataDir and InstallDir"
}
$separator = [System.IO.Path]::DirectorySeparatorChar
if ($DataDir.StartsWith($ExtensionDir + $separator, [System.StringComparison]::OrdinalIgnoreCase) -or
    $InstallDir.StartsWith($ExtensionDir + $separator, [System.StringComparison]::OrdinalIgnoreCase)) {
    Stop-WithError "ExtensionDir must not contain DataDir or InstallDir"
}
if ($ExtensionDir -eq $repoRoot -or $ExtensionDir -eq (Join-Path $repoRoot "extension") -or
    $repoRoot.StartsWith($ExtensionDir + $separator, [System.StringComparison]::OrdinalIgnoreCase)) {
    Stop-WithError "ExtensionDir must not be the repository or contain it"
}

$corvusdSource = Join-Path $DistDir "corvusd.exe"
$corvusSource = Join-Path $DistDir "corvus.exe"
foreach ($required in @($corvusdSource, $corvusSource)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
        Stop-WithError "missing $required; run scripts/package.sh first"
    }
}

$extensionSource = Join-Path $DistDir "extension"
if (-not (Test-Path -LiteralPath (Join-Path $extensionSource "manifest.json") -PathType Leaf)) {
    $extensionSource = Join-Path $repoRoot "extension"
}
if (-not (Test-Path -LiteralPath (Join-Path $extensionSource "manifest.json") -PathType Leaf)) {
    Stop-WithError "missing extension manifest in $DistDir\extension or $repoRoot\extension"
}

$candidates = New-Object System.Collections.Generic.List[string]
if (-not [string]::IsNullOrWhiteSpace($NssmPath)) {
    $candidates.Add($NssmPath)
}
$candidates.Add((Join-Path $InstallDir "nssm.exe"))
$nssmCommand = Get-Command nssm.exe -ErrorAction SilentlyContinue
if ($null -ne $nssmCommand) {
    $candidates.Add($nssmCommand.Source)
}
$candidates.Add("C:\ProgramData\chocolatey\bin\nssm.exe")
$candidates.Add("C:\Program Files\nssm\nssm.exe")

$script:NssmCommand = $null
foreach ($candidate in $candidates) {
    if (Test-Path -LiteralPath $candidate -PathType Leaf) {
        $script:NssmCommand = Get-AbsolutePath $candidate
        break
    }
}
if ($null -eq $script:NssmCommand) {
    Stop-WithError "nssm.exe not found; install nssm, put it on PATH, or pass -NssmPath"
}

foreach ($directory in @($InstallDir, $DataDir, $ExtensionDir)) {
    New-Item -ItemType Directory -Path $directory -Force | Out-Null
}

$service = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
$serviceExisted = $null -ne $service
if ($serviceExisted -and $service.Status.ToString() -ne "Stopped") {
    Write-Host "Stopping service $ServiceName ..."
    Invoke-Nssm @("stop", $ServiceName)
    if (-not (Wait-ServiceStatus -Name $ServiceName -Status "Stopped" -TimeoutSeconds 30)) {
        Stop-WithError "service $ServiceName did not stop within 30 seconds"
    }
}

Write-Host "Copying binaries to $InstallDir ..."
Copy-Item -LiteralPath $corvusdSource -Destination (Join-Path $InstallDir "corvusd.exe") -Force
Copy-Item -LiteralPath $corvusSource -Destination (Join-Path $InstallDir "corvus.exe") -Force

Write-Host "Copying extension to $ExtensionDir ..."
if (Test-Path -LiteralPath $ExtensionDir) {
    Remove-Item -LiteralPath $ExtensionDir -Recurse -Force
}
New-Item -ItemType Directory -Path $ExtensionDir -Force | Out-Null
Copy-Item -Path (Join-Path $extensionSource "*") -Destination $ExtensionDir -Recurse -Force
if (-not (Test-Path -LiteralPath (Join-Path $ExtensionDir "manifest.json") -PathType Leaf)) {
    Stop-WithError "the extension copy did not produce $ExtensionDir\manifest.json"
}

$dbPath = Join-Path $DataDir "corvus.db"
$appParameters = "serve --db `"$dbPath`""
if ($WsPort -gt 0) {
    $appParameters += " --ws-port $WsPort"
}
if ($HttpPort -gt 0) {
    $appParameters += " --http-port $HttpPort"
}

if (-not $serviceExisted) {
    Write-Host "Installing service $ServiceName ..."
    Invoke-Nssm @("install", $ServiceName, (Join-Path $InstallDir "corvusd.exe"))
}

Invoke-Nssm @("set", $ServiceName, "Application", (Join-Path $InstallDir "corvusd.exe"))
Invoke-Nssm @("set", $ServiceName, "AppDirectory", $DataDir)
Invoke-Nssm @("set", $ServiceName, "AppParameters", $appParameters)
Invoke-Nssm @("set", $ServiceName, "Start", "SERVICE_AUTO_START")
Invoke-Nssm @("set", $ServiceName, "DisplayName", $ServiceName)
Invoke-Nssm @("set", $ServiceName, "Description", "Corvus browser hub daemon")
Invoke-Nssm @("set", $ServiceName, "AppStdout", (Join-Path $DataDir "corvusd.log"))
Invoke-Nssm @("set", $ServiceName, "AppStderr", (Join-Path $DataDir "corvusd.err.log"))

Write-Host "Starting service $ServiceName ..."
& $script:NssmCommand start $ServiceName
if ($LASTEXITCODE -ne 0) {
    Write-Host "nssm start returned exit code $LASTEXITCODE; waiting for the service status ..."
}
if (-not (Wait-ServiceStatus -Name $ServiceName -Status "Running" -TimeoutSeconds 30)) {
    Write-Host "service $ServiceName did not reach Running; recent log lines:"
    Show-LogTail (Join-Path $DataDir "corvusd.err.log")
    Show-LogTail (Join-Path $DataDir "corvusd.log")
    Stop-WithError "service $ServiceName did not reach Running within 30 seconds"
}
Start-Sleep -Seconds 1
$service = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($null -eq $service -or $service.Status.ToString() -ne "Running") {
    Stop-WithError "service $ServiceName started but is no longer running"
}

$mcpPort = 7331
if ($HttpPort -gt 0) {
    $mcpPort = $HttpPort
}
$wsPortActual = 7330
if ($WsPort -gt 0) {
    $wsPortActual = $WsPort
}

Write-Host ""
Write-Host "Corvus is installed and the service is running."
Write-Host "  service:   $ServiceName"
Write-Host "  binaries:  $InstallDir"
Write-Host "  data:      $DataDir"
Write-Host "  database:  $dbPath"
Write-Host "  extension: $ExtensionDir"
Write-Host "  websocket: ws://127.0.0.1:$wsPortActual"
Write-Host "  MCP:       http://127.0.0.1:$mcpPort/mcp"
Write-Host ""
Write-Host "Next steps:"
Write-Host "  1. Open chrome://extensions (or edge://extensions), enable Developer mode,"
Write-Host "     choose Load unpacked and select: $ExtensionDir"
Write-Host "  2. Verify the daemon: & `"$InstallDir\corvus.exe`" doctor"
Write-Host "  3. HTTP client configuration: & `"$InstallDir\corvus.exe`" config --transport http"
Write-Host "     Paste the mcpServers entry into the client; keep the service running."
Write-Host "     For a stdio-only client, stop the service first, then run:"
Write-Host "       & `"$InstallDir\corvus.exe`" serve --stdio"
