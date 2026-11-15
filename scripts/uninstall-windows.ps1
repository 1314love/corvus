# Remove the Corvus Windows service and its installed files.
#
# Everything is addressed through -HomeDir. The service is removed first; the
# home directory is kept unless -PurgeData is given, which removes only that
# root. Run from an elevated PowerShell. The script is idempotent.
# -SelfTest checks the destructive-purge guard without changing anything.
[CmdletBinding()]
param(
    [string]$ServiceName = "Corvus",
    [string]$HomeDir = (Join-Path $env:ProgramData "Corvus"),
    [string]$InstallDir = "",
    [string]$DataDir = "",
    [string]$ExtensionDir = "",
    [string]$NssmPath = "",
    [switch]$PurgeData,
    [switch]$SelfTest
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

function Test-PathUnder {
    param([string]$Candidate, [string]$Root)
    $candidatePath = $Candidate.TrimEnd([char]'\')
    $rootPath = $Root.TrimEnd([char]'\')
    if ($candidatePath.Equals($rootPath, [System.StringComparison]::OrdinalIgnoreCase)) {
        return $true
    }
    return $candidatePath.StartsWith($rootPath + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase)
}

function Assert-NoReparsePoint {
    param([string]$Path)
    $current = $Path
    while ($true) {
        if (Test-Path -LiteralPath $current) {
            $item = Get-Item -LiteralPath $current -Force
            if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -eq [System.IO.FileAttributes]::ReparsePoint) {
                Stop-WithError "HomeDir must not contain a reparse point: $current"
            }
        }
        $parent = Split-Path -Parent $current
        if ([string]::IsNullOrWhiteSpace($parent) -or $parent.Equals($current, [System.StringComparison]::OrdinalIgnoreCase)) {
            break
        }
        $current = $parent
        if ([string]::IsNullOrWhiteSpace($current)) {
            break
        }
    }
}

function Assert-SafeHome {
    param([string]$Path)
    $full = (Get-AbsolutePath $Path).TrimEnd([char]'\')
    $driveRoot = [System.IO.Path]::GetPathRoot($full).TrimEnd([char]'\')
    if ($full -eq $driveRoot) {
        Stop-WithError "HomeDir must not be a drive root: $full"
    }
    $repoRoot = Split-Path -Parent $PSScriptRoot
    $tempRoots = @()
    foreach ($temp in @([System.IO.Path]::GetTempPath(), $env:TEMP)) {
        if (-not [string]::IsNullOrWhiteSpace($temp)) {
            $tempRoots += (Get-AbsolutePath $temp).TrimEnd([char]'\')
        }
    }
    $isTemp = $false
    foreach ($tempRoot in $tempRoots) {
        if (Test-PathUnder -Candidate $full -Root $tempRoot) {
            $isTemp = $true
        }
    }
    $exactProtected = @(
        $env:ProgramData,
        $env:USERPROFILE,
        $env:WINDIR,
        $env:ProgramFiles,
        [Environment]::GetEnvironmentVariable("ProgramFiles(x86)"),
        [System.IO.Path]::GetTempPath(),
        $env:TEMP,
        $repoRoot
    )
    foreach ($candidate in $exactProtected) {
        if ([string]::IsNullOrWhiteSpace($candidate)) {
            continue
        }
        $resolved = (Get-AbsolutePath $candidate).TrimEnd([char]'\')
        if ($full.Equals($resolved, [System.StringComparison]::OrdinalIgnoreCase)) {
            Stop-WithError "HomeDir must not be $resolved"
        }
    }
    if ((Test-PathUnder -Candidate $repoRoot -Root $full) -or (Test-PathUnder -Candidate $full -Root $repoRoot)) {
        Stop-WithError "HomeDir must not contain or live inside the repository: $repoRoot"
    }
    if (-not [string]::IsNullOrWhiteSpace($env:ProgramData)) {
        $programData = (Get-AbsolutePath $env:ProgramData).TrimEnd([char]'\')
        if (Test-PathUnder -Candidate $full -Root $programData) {
            $relative = $full.Substring($programData.Length).TrimStart([char]'\')
            $hasSeparator = $relative.Contains([System.IO.Path]::DirectorySeparatorChar.ToString())
            if ($hasSeparator -or -not $relative.StartsWith("Corvus", [System.StringComparison]::OrdinalIgnoreCase)) {
                Stop-WithError "HomeDir under ProgramData must be ProgramData\Corvus or a direct Corvus* child: $full"
            }
        }
    }
    $systemRoots = @($env:WINDIR, $env:ProgramFiles, [Environment]::GetEnvironmentVariable("ProgramFiles(x86)"))
    foreach ($root in $systemRoots) {
        if ([string]::IsNullOrWhiteSpace($root)) {
            continue
        }
        $resolved = (Get-AbsolutePath $root).TrimEnd([char]'\')
        if (Test-PathUnder -Candidate $full -Root $resolved) {
            Stop-WithError "HomeDir must not be under $resolved"
        }
    }
    if (-not $isTemp -and -not [string]::IsNullOrWhiteSpace($env:USERPROFILE)) {
        $profile = (Get-AbsolutePath $env:USERPROFILE).TrimEnd([char]'\')
        if (Test-PathUnder -Candidate $full -Root $profile) {
            Stop-WithError "HomeDir must not be under the user profile: $full"
        }
    }
    Assert-NoReparsePoint -Path $full
    return $full
}

function Resolve-LegacySubdir {
    param(
        [string]$Name,
        [string]$Value,
        [string]$Canonical,
        [bool]$WasBound
    )
    if (-not $WasBound) {
        return $Canonical
    }
    $resolved = (Get-AbsolutePath $Value).TrimEnd([char]'\')
    if (-not (Test-PathUnder -Candidate $resolved -Root $HomeDir) -or
        $resolved.Equals($HomeDir, [System.StringComparison]::OrdinalIgnoreCase)) {
        Stop-WithError "$Name is a deprecated alias and must be a subdirectory of HomeDir ($HomeDir); got $resolved. Use -HomeDir instead."
    }
    if (-not $resolved.Equals($Canonical, [System.StringComparison]::OrdinalIgnoreCase)) {
        Write-Warning "$Name is deprecated; the fixed layout uses $Canonical"
    }
    return $Canonical
}

function Find-Nssm {
    param([string]$ExplicitPath, [string]$InstallDirectory)
    $candidates = New-Object System.Collections.Generic.List[string]
    $candidates.Add((Join-Path $InstallDirectory "nssm.exe"))
    if (-not [string]::IsNullOrWhiteSpace($ExplicitPath)) {
        $candidates.Add($ExplicitPath)
    }
    $candidates.Add((Join-Path $PSScriptRoot "nssm.exe"))
    $candidates.Add((Join-Path (Split-Path -Parent $PSScriptRoot) "packaging\windows\nssm.exe"))
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

if ($SelfTest) {
    $bad = Join-Path $env:USERPROFILE "Documents"
    $badRejected = $false
    try {
        $null = Assert-SafeHome $bad
    } catch {
        $badRejected = $true
    }
    if (-not $badRejected) {
        Stop-WithError "self-test: $bad was accepted as a HomeDir"
    }
    $good = Join-Path $env:ProgramData "Corvus"
    try {
        $null = Assert-SafeHome $good
    } catch {
        Stop-WithError "self-test: $good was rejected: $($_.Exception.Message)"
    }
    $tempHome = Join-Path ([System.IO.Path]::GetTempPath()) "Corvus Guard Self Test"
    try {
        $null = Assert-SafeHome $tempHome
    } catch {
        Stop-WithError "self-test: $tempHome was rejected: $($_.Exception.Message)"
    }
    Write-Host "self-test passed: the user-profile path is refused; ProgramData\Corvus and a temp HomeDir are allowed"
    exit 0
}

if (-not (Test-Administrator)) {
    Stop-WithError "run this script from an elevated PowerShell session"
}
if ($ServiceName -notmatch '^[A-Za-z0-9_.-]+$' -or $ServiceName.Length -gt 64) {
    Stop-WithError "ServiceName must contain only letters, digits, dot, underscore and hyphen"
}

$HomeDir = Assert-SafeHome $HomeDir
$binDir = Join-Path $HomeDir "bin"
$dataDir = Join-Path $HomeDir "data"
$extensionDir = Join-Path $HomeDir "extension"

$InstallDir = Resolve-LegacySubdir -Name "InstallDir" -Value $InstallDir -Canonical $binDir -WasBound $PSBoundParameters.ContainsKey("InstallDir")
$DataDir = Resolve-LegacySubdir -Name "DataDir" -Value $DataDir -Canonical $dataDir -WasBound $PSBoundParameters.ContainsKey("DataDir")
$ExtensionDir = Resolve-LegacySubdir -Name "ExtensionDir" -Value $ExtensionDir -Canonical $extensionDir -WasBound $PSBoundParameters.ContainsKey("ExtensionDir")

$service = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($null -ne $service) {
    $nssm = Find-Nssm -ExplicitPath $NssmPath -InstallDirectory $binDir
    if ($null -eq $nssm) {
        Write-Host "nssm.exe not found; using sc.exe to remove service $ServiceName ..."
        & sc.exe stop $ServiceName | Out-Null
        & sc.exe delete $ServiceName | Out-Null
        $scExit = $LASTEXITCODE
        if (-not (Wait-ServiceGone -Name $ServiceName -TimeoutSeconds 30)) {
            Stop-WithError "sc.exe delete $ServiceName failed with exit code $scExit"
        }
    } else {
        if ($service.Status.ToString() -ne "Stopped") {
            Write-Host "Stopping service $ServiceName ..."
            & $nssm stop $ServiceName
            $stopExit = $LASTEXITCODE
            $deadline = (Get-Date).AddSeconds(30)
            do {
                Start-Sleep -Milliseconds 500
                $service = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
            } while ($null -ne $service -and $service.Status.ToString() -ne "Stopped" -and (Get-Date) -lt $deadline)
            if ($null -ne $service -and $service.Status.ToString() -ne "Stopped") {
                Stop-WithError "nssm stop $ServiceName failed with exit code $stopExit and the service did not stop"
            }
        }
        Write-Host "Removing service $ServiceName ..."
        & $nssm remove $ServiceName confirm
        if (-not (Wait-ServiceGone -Name $ServiceName -TimeoutSeconds 30)) {
            Stop-WithError "nssm remove $ServiceName failed and the service is still registered"
        }
    }
    Write-Host "Service $ServiceName removed."
} else {
    Write-Host "Service $ServiceName is not installed."
}

if ($PurgeData) {
    if (Test-Path -LiteralPath $HomeDir) {
        Remove-Item -LiteralPath $HomeDir -Recurse -Force
        Write-Host "Removed $HomeDir"
    } else {
        Write-Host "$HomeDir does not exist."
    }
    Write-Host "All Corvus data under $HomeDir was purged."
} else {
    foreach ($name in @("corvusd.exe", "corvus.exe")) {
        $path = Join-Path $binDir $name
        if (Test-Path -LiteralPath $path -PathType Leaf) {
            Remove-Item -LiteralPath $path -Force
            Write-Host "Removed $path"
        }
    }
    if (Test-Path -LiteralPath $extensionDir) {
        Remove-Item -LiteralPath $extensionDir -Recurse -Force
        Write-Host "Removed $extensionDir"
    }
    Write-Host "Keeping database at $(Join-Path $dataDir 'corvus.db') (use -PurgeData to remove $HomeDir)."
}

Write-Host "Uninstall complete."
