# Install or upgrade Corvus as a Windows service under one home directory.
#
# Run from an elevated PowerShell. The script is idempotent: it stops a
# running service, copies the release binaries and extension, migrates an old
# per-user database once, updates the service configuration, and starts the
# service again. Everything the service owns lives under -HomeDir. The only
# external entry is the nssm service registration.
[CmdletBinding()]
param(
    [string]$ServiceName = "Corvus",
    [string]$HomeDir = (Join-Path $env:ProgramData "Corvus"),
    [string]$InstallDir = "",
    [string]$DataDir = "",
    [string]$ExtensionDir = "",
    [string]$NssmPath = "",
    [string]$DistDir = "",
    [int]$WsPort = 7330,
    [int]$HttpPort = 7331,
    [switch]$KeepLegacyData
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

function Get-ServiceImagePath {
    param([string]$Name)
    $key = "HKLM:\SYSTEM\CurrentControlSet\Services\$Name"
    $item = Get-ItemProperty -LiteralPath $key -ErrorAction SilentlyContinue
    if ($null -eq $item -or [string]::IsNullOrWhiteSpace([string]$item.ImagePath)) {
        return ""
    }
    return [string]$item.ImagePath
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
if ($WsPort -lt 1 -or $WsPort -gt 65535) {
    Stop-WithError "WsPort must be between 1 and 65535"
}
if ($HttpPort -lt 1 -or $HttpPort -gt 65535) {
    Stop-WithError "HttpPort must be between 1 and 65535"
}

$HomeDir = Assert-SafeHome $HomeDir
$binDir = Join-Path $HomeDir "bin"
$dataDir = Join-Path $HomeDir "data"
$logsDir = Join-Path $HomeDir "logs"
$configDir = Join-Path $HomeDir "config"
$extensionDir = Join-Path $HomeDir "extension"

$InstallDir = Resolve-LegacySubdir -Name "InstallDir" -Value $InstallDir -Canonical $binDir -WasBound $PSBoundParameters.ContainsKey("InstallDir")
$DataDir = Resolve-LegacySubdir -Name "DataDir" -Value $DataDir -Canonical $dataDir -WasBound $PSBoundParameters.ContainsKey("DataDir")
$ExtensionDir = Resolve-LegacySubdir -Name "ExtensionDir" -Value $ExtensionDir -Canonical $extensionDir -WasBound $PSBoundParameters.ContainsKey("ExtensionDir")

$repoRoot = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($DistDir)) {
    if ((Test-Path -LiteralPath (Join-Path $PSScriptRoot "corvusd.exe") -PathType Leaf) -and
        (Test-Path -LiteralPath (Join-Path $PSScriptRoot "corvus.exe") -PathType Leaf)) {
        $DistDir = $PSScriptRoot
    } else {
        $DistDir = Join-Path $repoRoot "dist"
    }
}
$DistDir = (Get-AbsolutePath $DistDir).TrimEnd([char]'\')

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

foreach ($directory in @($HomeDir, $binDir, $dataDir, $logsDir, $configDir, $extensionDir)) {
    New-Item -ItemType Directory -Path $directory -Force | Out-Null
}

$service = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
$serviceExisted = $null -ne $service
$expectedNssm = Join-Path $binDir "nssm.exe"
$usesHomeNssm = $false
if ($serviceExisted) {
    $imagePath = Get-ServiceImagePath -Name $ServiceName
    $usesHomeNssm = -not [string]::IsNullOrWhiteSpace($imagePath) -and
        $imagePath.IndexOf($expectedNssm, [System.StringComparison]::OrdinalIgnoreCase) -ge 0
}
if ($serviceExisted -and $service.Status.ToString() -ne "Stopped") {
    Write-Host "Stopping service $ServiceName ..."
    Stop-Service -Name $ServiceName -Force -ErrorAction Stop
    if (-not (Wait-ServiceStatus -Name $ServiceName -Status "Stopped" -TimeoutSeconds 30)) {
        Stop-WithError "service $ServiceName did not stop within 30 seconds"
    }
}

$nssmSources = New-Object System.Collections.Generic.List[string]
$nssmSources.Add((Join-Path $DistDir "nssm.exe"))
$nssmSources.Add((Join-Path $repoRoot "packaging\windows\nssm.exe"))
$nssmSources.Add((Join-Path $binDir "nssm.exe"))
if (-not [string]::IsNullOrWhiteSpace($NssmPath)) {
    $nssmSources.Add($NssmPath)
}

$nssmSource = $null
foreach ($candidate in $nssmSources) {
    if (Test-Path -LiteralPath $candidate -PathType Leaf) {
        $nssmSource = Get-AbsolutePath $candidate
        break
    }
}
if ($null -eq $nssmSource) {
    Stop-WithError "nssm.exe is missing; expected packaging/windows/nssm.exe or dist/nssm.exe"
}

$vendorNssm = Join-Path $binDir "nssm.exe"
if (-not $nssmSource.Equals($vendorNssm, [System.StringComparison]::OrdinalIgnoreCase)) {
    Write-Host "Vendoring nssm.exe to $vendorNssm ..."
    Copy-Item -LiteralPath $nssmSource -Destination $vendorNssm -Force
}
if (-not (Test-Path -LiteralPath $vendorNssm -PathType Leaf)) {
    Stop-WithError "could not install $vendorNssm"
}
$script:NssmCommand = $vendorNssm



$legacyRoot = Join-Path $env:USERPROFILE ".corvus"
$migrationDir = Join-Path $HomeDir "migration\userprofile-corvus"
if (-not $KeepLegacyData -and (Test-Path -LiteralPath $legacyRoot)) {
    if ((Test-PathUnder -Candidate $HomeDir -Root $legacyRoot) -or (Test-PathUnder -Candidate $legacyRoot -Root $HomeDir)) {
        Stop-WithError "HomeDir must not overlap the legacy data at $legacyRoot"
    }
    if (Test-Path -LiteralPath $migrationDir) {
        Stop-WithError "legacy data at $legacyRoot cannot be moved because $migrationDir already exists; merge it manually and rerun"
    }
    $migrationParent = Split-Path -Parent $migrationDir
    New-Item -ItemType Directory -Path $migrationParent -Force | Out-Null
    Write-Host "Moving legacy data: $legacyRoot -> $migrationDir"
    try {
        Move-Item -LiteralPath $legacyRoot -Destination $migrationDir -ErrorAction Stop
    } catch {
        Stop-WithError "could not move legacy data from $legacyRoot to $migrationDir; service configuration was not changed: $($_.Exception.Message)"
    }
    if (Test-Path -LiteralPath $legacyRoot) {
        Stop-WithError "legacy path $legacyRoot still exists after the move; stop and inspect it"
    }
} elseif ($KeepLegacyData -and (Test-Path -LiteralPath $legacyRoot)) {
    Write-Host "Keeping legacy data at $legacyRoot (-KeepLegacyData)."
}

$legacyDb = Join-Path $migrationDir "corvus.db"
if ($KeepLegacyData) {
    $legacyDb = Join-Path $legacyRoot "corvus.db"
}
$newDb = Join-Path $dataDir "corvus.db"
if (-not (Test-Path -LiteralPath $newDb -PathType Leaf) -and (Test-Path -LiteralPath $legacyDb -PathType Leaf)) {
    Write-Host "Migrating legacy database: $legacyDb -> $newDb"
    Copy-Item -LiteralPath $legacyDb -Destination $newDb -Force
    Write-Host "  migrated corvus.db"
    foreach ($suffix in @("-wal", "-shm")) {
        $legacySidecar = $legacyDb + $suffix
        if (Test-Path -LiteralPath $legacySidecar -PathType Leaf) {
            Copy-Item -LiteralPath $legacySidecar -Destination ($newDb + $suffix) -Force
            Write-Host "  migrated corvus.db$suffix"
        }
    }
} elseif (Test-Path -LiteralPath $newDb -PathType Leaf) {
    Write-Host "Keeping existing database: $newDb"
} else {
    Write-Host "No legacy database at $legacyDb; a new database will be created."
}

Write-Host "Copying binaries to $binDir ..."
Copy-Item -LiteralPath $corvusdSource -Destination (Join-Path $binDir "corvusd.exe") -Force
Copy-Item -LiteralPath $corvusSource -Destination (Join-Path $binDir "corvus.exe") -Force

Write-Host "Copying extension to $extensionDir ..."
if (Test-Path -LiteralPath $extensionDir) {
    Remove-Item -LiteralPath $extensionDir -Recurse -Force
}
New-Item -ItemType Directory -Path $extensionDir -Force | Out-Null
Copy-Item -Path (Join-Path $extensionSource "*") -Destination $extensionDir -Recurse -Force
if (-not (Test-Path -LiteralPath (Join-Path $extensionDir "manifest.json") -PathType Leaf)) {
    Stop-WithError "the extension copy did not produce $extensionDir\manifest.json"
}

$appParameters = [string]::Format('serve --home "{0}" --ws-port {1} --http-port {2}', $HomeDir, $WsPort, $HttpPort)

if ($serviceExisted -and -not $usesHomeNssm) {
    Write-Host "Reinstalling service $ServiceName with $expectedNssm ..."
    & $script:NssmCommand remove $ServiceName confirm
    if (-not (Wait-ServiceGone -Name $ServiceName -TimeoutSeconds 30)) {
        & sc.exe delete $ServiceName | Out-Null
        if (-not (Wait-ServiceGone -Name $ServiceName -TimeoutSeconds 30)) {
            Stop-WithError "could not remove service $ServiceName before reinstalling it"
        }
    }
}

if (-not $serviceExisted -or -not $usesHomeNssm) {
    Write-Host "Installing service $ServiceName ..."
    Invoke-Nssm @("install", $ServiceName, (Join-Path $binDir "corvusd.exe"))
}

Invoke-Nssm @("set", $ServiceName, "Application", (Join-Path $binDir "corvusd.exe"))
Invoke-Nssm @("set", $ServiceName, "AppDirectory", $HomeDir)
Invoke-Nssm @("set", $ServiceName, "AppParameters", $appParameters)
Invoke-Nssm @("set", $ServiceName, "Start", "SERVICE_AUTO_START")
Invoke-Nssm @("set", $ServiceName, "DisplayName", $ServiceName)
Invoke-Nssm @("set", $ServiceName, "Description", "Corvus browser hub daemon")
Invoke-Nssm @("set", $ServiceName, "AppStdout", (Join-Path $logsDir "corvusd.out.log"))
Invoke-Nssm @("set", $ServiceName, "AppStderr", (Join-Path $logsDir "corvusd.err.log"))
Invoke-Nssm @("set", $ServiceName, "AppRotateFiles", "1")
Invoke-Nssm @("set", $ServiceName, "AppRotateOnline", "1")
Invoke-Nssm @("set", $ServiceName, "AppRotateBytes", "10485760")

Write-Host "Starting service $ServiceName ..."
& $script:NssmCommand start $ServiceName
if ($LASTEXITCODE -ne 0) {
    Write-Host "nssm start returned exit code $LASTEXITCODE; waiting for the service status ..."
}
if (-not (Wait-ServiceStatus -Name $ServiceName -Status "Running" -TimeoutSeconds 30)) {
    Write-Host "service $ServiceName did not reach Running; recent log lines:"
    Show-LogTail (Join-Path $logsDir "corvusd.err.log")
    Show-LogTail (Join-Path $logsDir "corvusd.out.log")
    Stop-WithError "service $ServiceName did not reach Running within 30 seconds"
}
Start-Sleep -Seconds 1
$service = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($null -eq $service -or $service.Status.ToString() -ne "Running") {
    Stop-WithError "service $ServiceName started but is no longer running"
}

$corvusCli = Join-Path $binDir "corvus.exe"
Write-Host ""
Write-Host "Corvus is installed and the service is running."
Write-Host "  service:   $ServiceName"
Write-Host "  home:      $HomeDir"
Write-Host "  binaries:  $binDir"
Write-Host "  data:      $dataDir"
Write-Host "  database:  $newDb"
Write-Host "  extension: $extensionDir"
Write-Host "  logs:      $logsDir"
Write-Host "  websocket: ws://127.0.0.1:$WsPort"
Write-Host "  MCP:       http://127.0.0.1:$HttpPort/mcp"
Write-Host ""
Write-Host "Next steps:"
Write-Host "  1. Open chrome://extensions (or edge://extensions), enable Developer mode,"
Write-Host "     choose Load unpacked and select: $extensionDir"
Write-Host ('  2. Verify the daemon: & "' + $corvusCli + '" doctor')
Write-Host ('  3. HTTP client configuration: & "' + $corvusCli + '" config --transport http')
Write-Host "     Paste the mcpServers entry into the client; keep the service running."
Write-Host "     For a stdio-only client, stop the service first, then run:"
Write-Host ('       & "' + $corvusCli + '" serve --stdio')
