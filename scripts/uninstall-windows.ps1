# Remove the Corvus Windows service and its installed binaries.
#
# Database files are kept unless -PurgeData is given. Run from an elevated
# PowerShell when the service exists. The script is idempotent.
[CmdletBinding()]
param(
    [string]$ServiceName = "Corvus",
    [string]$InstallDir = (Join-Path $env:USERPROFILE ".corvus\bin"),
    [string]$DataDir = (Join-Path $env:USERPROFILE ".corvus"),
    [string]$ExtensionDir = (Join-Path $env:USERPROFILE ".corvus\extension"),
    [string]$NssmPath = "",
    [switch]$PurgeData
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Stop-WithError {
    param([string]$Message)
    throw "uninstall-windows: $Message"
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

function Find-Nssm {
    param([string]$ExplicitPath, [string]$InstallDirectory)
    $candidates = New-Object System.Collections.Generic.List[string]
    if (-not [string]::IsNullOrWhiteSpace($ExplicitPath)) {
        $candidates.Add($ExplicitPath)
    }
    $candidates.Add((Join-Path $InstallDirectory "nssm.exe"))
    $command = Get-Command nssm.exe -ErrorAction SilentlyContinue
    if ($null -ne $command) {
        $candidates.Add($command.Source)
    }
    $candidates.Add("C:\ProgramData\chocolatey\bin\nssm.exe")
    $candidates.Add("C:\Program Files\nssm\nssm.exe")
    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
            return Get-AbsolutePath $candidate
        }
    }
    return $null
}

function Wait-ServiceGone {
    param([string]$Name, [int]$TimeoutSeconds)
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $service = Get-Service -Name $Name -ErrorAction SilentlyContinue
        if ($null -eq $service) {
            return $true
        }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    return $false
}

function Remove-EmptyDirectory {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
        return
    }
    $full = [System.IO.Path]::GetFullPath($Path).TrimEnd([char]'\')
    $driveRoot = [System.IO.Path]::GetPathRoot($full).TrimEnd([char]'\')
    if ($full -eq $driveRoot) {
        Write-Host "Skipped unsafe empty directory $Path"
        return
    }
    $repoRoot = Split-Path -Parent $PSScriptRoot
    $protected = @($env:USERPROFILE, $env:TEMP, $repoRoot, (Join-Path $repoRoot "extension"))
    foreach ($candidate in $protected) {
        if ([string]::IsNullOrWhiteSpace($candidate)) {
            continue
        }
        $resolved = [System.IO.Path]::GetFullPath($candidate).TrimEnd([char]'\')
        if ($full -eq $resolved) {
            Write-Host "Skipped unsafe empty directory $Path"
            return
        }
    }
    try {
        $entries = @(Get-ChildItem -LiteralPath $Path -Force -ErrorAction Stop)
    } catch {
        Write-Host "Could not inspect directory $Path; leaving it in place."
        return
    }
    if ($entries.Count -ne 0) {
        Write-Host "Kept non-empty directory $Path"
        return
    }
    try {
        Remove-Item -LiteralPath $Path -Force
        Write-Host "Removed empty directory $Path"
    } catch {
        Write-Host "Could not remove empty directory $Path; leaving it in place."
    }
}

if ($ServiceName -notmatch '^[A-Za-z0-9_.-]+$' -or $ServiceName.Length -gt 64) {
    Stop-WithError "ServiceName must contain only letters, digits, dot, underscore and hyphen"
}

$InstallDir = (Get-AbsolutePath $InstallDir).TrimEnd([char]'\')
$DataDir = (Get-AbsolutePath $DataDir).TrimEnd([char]'\')
$ExtensionDir = (Get-AbsolutePath $ExtensionDir).TrimEnd([char]'\')

$service = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($null -ne $service) {
    if (-not (Test-Administrator)) {
        Stop-WithError "run this script from an elevated PowerShell session to remove the service"
    }
    $nssm = Find-Nssm -ExplicitPath $NssmPath -InstallDirectory $InstallDir
    if ($null -eq $nssm) {
        Write-Host "nssm.exe not found; using sc.exe to remove service $ServiceName ..."
        & sc.exe stop $ServiceName | Out-Null
        & sc.exe delete $ServiceName | Out-Null
        if ($LASTEXITCODE -ne 0) {
            Stop-WithError "sc.exe delete $ServiceName failed with exit code $LASTEXITCODE"
        }
    } else {
        Write-Host "Stopping service $ServiceName ..."
        if ($service.Status.ToString() -ne "Stopped") {
            & $nssm stop $ServiceName
            if ($LASTEXITCODE -ne 0) {
                Stop-WithError "nssm stop $ServiceName failed with exit code $LASTEXITCODE"
            }
            $deadline = (Get-Date).AddSeconds(30)
            do {
                Start-Sleep -Milliseconds 500
                $service = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
            } while ($null -ne $service -and $service.Status.ToString() -ne "Stopped" -and (Get-Date) -lt $deadline)
            if ($null -ne $service -and $service.Status.ToString() -ne "Stopped") {
                Stop-WithError "service $ServiceName did not stop within 30 seconds"
            }
        }
        Write-Host "Removing service $ServiceName ..."
        & $nssm remove $ServiceName confirm
        if ($LASTEXITCODE -ne 0) {
            Stop-WithError "nssm remove $ServiceName failed with exit code $LASTEXITCODE"
        }
    }
    if (-not (Wait-ServiceGone -Name $ServiceName -TimeoutSeconds 30)) {
        Stop-WithError "service $ServiceName is still registered after removal"
    }
    Write-Host "Service $ServiceName removed."
} else {
    Write-Host "Service $ServiceName is not installed."
}

foreach ($name in @("corvusd.exe", "corvus.exe")) {
    $path = Join-Path $InstallDir $name
    if (Test-Path -LiteralPath $path -PathType Leaf) {
        Remove-Item -LiteralPath $path -Force
        Write-Host "Removed $path"
    }
}

$separator = [System.IO.Path]::DirectorySeparatorChar
$repoRoot = Split-Path -Parent $PSScriptRoot
$safeExtensionDir = $ExtensionDir -ne $DataDir -and $ExtensionDir -ne $InstallDir -and
    $ExtensionDir -ne $repoRoot -and $ExtensionDir -ne (Join-Path $repoRoot "extension") -and
    -not $DataDir.StartsWith($ExtensionDir + $separator, [System.StringComparison]::OrdinalIgnoreCase) -and
    -not $InstallDir.StartsWith($ExtensionDir + $separator, [System.StringComparison]::OrdinalIgnoreCase) -and
    -not $repoRoot.StartsWith($ExtensionDir + $separator, [System.StringComparison]::OrdinalIgnoreCase)
if (Test-Path -LiteralPath $ExtensionDir) {
    if ($safeExtensionDir) {
        Remove-Item -LiteralPath $ExtensionDir -Recurse -Force
        Write-Host "Removed $ExtensionDir"
    } else {
        Write-Host "Skipped unsafe ExtensionDir: $ExtensionDir"
    }
}

if ($PurgeData) {
    foreach ($name in @("corvus.db", "corvus.db-wal", "corvus.db-shm", "corvusd.log", "corvusd.err.log")) {
        $path = Join-Path $DataDir $name
        if (Test-Path -LiteralPath $path -PathType Leaf) {
            Remove-Item -LiteralPath $path -Force
            Write-Host "Removed $path"
        }
    }
    Write-Host "Database and log files were purged."
    Remove-EmptyDirectory -Path $InstallDir
    Remove-EmptyDirectory -Path $DataDir
} else {
    Write-Host "Database kept at $(Join-Path $DataDir "corvus.db") (use -PurgeData to delete it)."
}

Write-Host "Uninstall complete."
