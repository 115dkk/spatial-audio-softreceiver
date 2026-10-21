#Requires -Version 7.0
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Releases,
    [Parameter(Mandatory)][string]$Setup,
    [Parameter(Mandatory)][string]$OldVersion,
    [Parameter(Mandatory)][string]$NewVersion,
    [Parameter(Mandatory)][string]$Summary
)

# Disposable, unelevated Windows test account only. Does not install a driver.
# CLI: https://docs.velopack.io/reference/cli/content/setup-windows
#      https://docs.velopack.io/reference/cli/content/update-windows
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$Summary = [IO.Path]::GetFullPath($Summary)
$artifactDir = [IO.Path]::GetDirectoryName($Summary)
[IO.Directory]::CreateDirectory($artifactDir) | Out-Null
$runId = [Guid]::NewGuid().ToString('N')
$resultPath = Join-Path $artifactDir "update-once-$runId.json"
$report = [ordered]@{
    ok = $false; old_version = $OldVersion; new_version = $NewVersion
    releases = $Releases; setup = $Setup; result = $resultPath
    started = [DateTime]::UtcNow.ToString('o'); stages = [Collections.Generic.List[object]]::new()
    cleanup = $null; error = $null
}
$ownedInstall = $false
$installRoot = $null
$shortcut = $null
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\SpatialSoftReceiver'
$appLog = $null
$logOffset = 0L
$stageNames = @('preflight', 'install', 'installed-files-and-old-version', 'shortcut-aumid',
    'uninstall-registration', 'update-once', 'updated-version', 'uninstall', 'removed', 'uninstall-hook')

function Save-Summary {
    $temp = "$Summary.$runId.tmp"
    [IO.File]::WriteAllText($temp, (($report | ConvertTo-Json -Depth 12) + "`n"), [Text.UTF8Encoding]::new($false))
    [IO.File]::Move($temp, $Summary, $true)
}

function Invoke-Stage([string]$Name, [scriptblock]$Action) {
    $timer = [Diagnostics.Stopwatch]::StartNew()
    $record = [ordered]@{ name = $Name; status = 'running'; seconds = 0; detail = $null }
    $report.stages.Add($record)
    Save-Summary
    try {
        $record.detail = & $Action
        $record.status = 'passed'
    } catch {
        $record.status = 'failed'
        $record.detail = $_.Exception.Message
        throw
    } finally {
        $record.seconds = [Math]::Round($timer.Elapsed.TotalSeconds, 3)
        Save-Summary
    }
}

function Invoke-Child([string]$File, [string[]]$Arguments, [int]$Timeout = 120, [string]$Source = '') {
    # ArgumentList quotes each argument correctly; the source override is scoped
    # to this child rather than changing the runner's process environment.
    $info = [Diagnostics.ProcessStartInfo]::new($File)
    $info.UseShellExecute = $false
    foreach ($argument in $Arguments) { $info.ArgumentList.Add($argument) }
    if ($Source) { $info.Environment['SSR_UPDATE_SOURCE'] = $Source }
    else { $info.Environment.Remove('SSR_UPDATE_SOURCE') | Out-Null }
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $info
    try {
        if (-not $process.Start()) { throw "Failed to start $File" }
        if (-not $process.WaitForExit($Timeout * 1000)) {
            $process.Kill($true)
            $process.WaitForExit(5000) | Out-Null
            throw "Child timed out after ${Timeout}s: $File"
        }
        if ($process.ExitCode -ne 0) { throw "Child exited $($process.ExitCode): $File" }
        return @{ file = $File; arguments = $Arguments; exit_code = $process.ExitCode }
    } finally { $process.Dispose() }
}

function Wait-Condition([scriptblock]$Condition, [string]$Description, [int]$Timeout = 120) {
    $timer = [Diagnostics.Stopwatch]::StartNew()
    $lastError = ''
    do {
        try { if (& $Condition) { return } } catch { $lastError = $_.Exception.Message }
        Start-Sleep -Milliseconds 500
    } while ($timer.Elapsed.TotalSeconds -lt $Timeout)
    throw "Timed out after ${Timeout}s: $Description. $lastError"
}

function Read-InstalledVersion {
    # Velopack locator.rs reads current/sq.version as a NuGet XML manifest.
    $manifest = Join-Path $installRoot 'current\sq.version'
    [xml]$xml = [IO.File]::ReadAllText($manifest)
    $id = $xml.SelectSingleNode("//*[local-name()='metadata']/*[local-name()='id']")
    $version = $xml.SelectSingleNode("//*[local-name()='metadata']/*[local-name()='version']")
    if ($null -eq $id -or $id.InnerText -cne 'SpatialSoftReceiver' -or $null -eq $version) {
        throw "Invalid installed manifest: $manifest"
    }
    return $version.InnerText
}

function Stop-InstalledProcesses {
    # Only our installation, never kill an unrelated development/test process.
    $prefix = $installRoot.TrimEnd('\') + '\'
    # Stop the guard first so it cannot respawn the child between enumerations.
    foreach ($name in @('ssr-guard', 'spatial-softreceiver')) {
        foreach ($process in @(Get-Process -Name $name -ErrorAction SilentlyContinue)) {
            try {
                if ($process.HasExited) { continue }
                $path = $process.Path
                if ($path -and $path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
                    $process.Kill()
                    if (-not $process.WaitForExit(10000)) { throw "Cannot stop $path" }
                }
            } finally { $process.Dispose() }
        }
    }
}

function Read-AppLogSuffix {
    if (-not (Test-Path -LiteralPath $appLog)) { return '' }
    $stream = [IO.File]::Open($appLog, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
    try {
        if ($stream.Length -lt $logOffset) { throw 'app.log shrank during the cycle' }
        $stream.Position = $logOffset
        $reader = [IO.StreamReader]::new($stream)
        try { return $reader.ReadToEnd() } finally { $reader.Dispose() }
    } finally { $stream.Dispose() }
}

try {
    Invoke-Stage 'preflight' {
        if (-not $IsWindows) { throw 'installer_cycle.ps1 requires Windows' }
        $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
        try {
            $principal = [Security.Principal.WindowsPrincipal]::new($identity)
            if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
                throw 'Use an unelevated disposable account for this per-user cycle'
            }
        } finally { $identity.Dispose() }
        # Start-Process -Credential hands this process the caller's environment,
        # so USERPROFILE, TEMP and LOCALAPPDATA still name the runner account's
        # folders; the known-folder API expands %USERPROFILE% from that block
        # too, and Velopack's Setup is refused there. Read this account's
        # profile from ProfileList by its SID, which -LoadUserProfile created,
        # and rebuild the variables before anything, Setup included, reads them.
        $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
        $profileKey = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList\$sid"
        $userProfile = [Environment]::ExpandEnvironmentVariables((Get-ItemProperty -LiteralPath $profileKey).ProfileImagePath)
        if (-not $userProfile -or -not (Test-Path -LiteralPath $userProfile -PathType Container)) {
            throw "No loaded profile for $sid"
        }
        $own = [ordered]@{
            USERPROFILE = $userProfile
            APPDATA = Join-Path $userProfile 'AppData\Roaming'
            LOCALAPPDATA = Join-Path $userProfile 'AppData\Local'
        }
        $own.TEMP = Join-Path $own.LOCALAPPDATA 'Temp'
        $own.TMP = $own.TEMP
        $own.HOMEDRIVE = Split-Path -Qualifier $userProfile
        $own.HOMEPATH = $userProfile.Substring($own.HOMEDRIVE.Length)
        $own.USERNAME = [Environment]::UserName
        foreach ($folder in $own.APPDATA, $own.LOCALAPPDATA, $own.TEMP) {
            New-Item -ItemType Directory -Force -Path $folder | Out-Null
        }
        foreach ($name in @($own.Keys)) { [Environment]::SetEnvironmentVariable($name, $own[$name], 'Process') }
        if (-not $env:LOCALAPPDATA) { throw 'LOCALAPPDATA is missing' }
        $script:installRoot = Join-Path $env:LOCALAPPDATA 'SpatialSoftReceiver'
        $script:appLog = Join-Path $env:LOCALAPPDATA 'Spatial SoftReceiver\app.log'
        # The per-user Start Menu Programs folder, where Velopack puts the
        # StartMenuRoot shortcut; a fresh profile may not have it yet.
        $programs = Join-Path $own.APPDATA 'Microsoft\Windows\Start Menu\Programs'
        $script:shortcut = Join-Path $programs 'Spatial SoftReceiver.lnk'
        foreach ($path in @($installRoot, $shortcut, $uninstallKey,
            (Join-Path $env:LOCALAPPDATA 'Spatial SoftReceiver\restore-point.json'))) {
            if (Test-Path -LiteralPath $path) { throw "Pre-existing installation or restore point: $path" }
        }
        if ($OldVersion -eq $NewVersion -or $OldVersion -notmatch '^\d+\.\d+\.\d+([+-][0-9A-Za-z.-]+)?$' -or
            $NewVersion -notmatch '^\d+\.\d+\.\d+([+-][0-9A-Za-z.-]+)?$') { throw 'Supply distinct semantic versions' }
        $script:Releases = (Resolve-Path -LiteralPath $Releases).Path
        $script:Setup = (Resolve-Path -LiteralPath $Setup).Path
        if (-not (Test-Path -LiteralPath $Releases -PathType Container) -or
            -not (Test-Path -LiteralPath $Setup -PathType Leaf)) { throw 'Releases directory or Setup executable is missing' }
        $feed = Get-Content -LiteralPath (Join-Path $Releases 'releases.win.json') -Raw | ConvertFrom-Json
        $packages = @()
        foreach ($version in @($OldVersion, $NewVersion)) {
            $assets = @($feed.Assets | Where-Object { $_.Version -ceq $version -and $_.Type -eq 'Full' })
            if ($assets.Count -ne 1) { throw "Expected one full package for $version" }
            $package = [IO.Path]::GetFullPath((Join-Path $Releases $assets[0].FileName))
            if (-not $package.StartsWith($Releases.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) {
                throw 'Package path escapes Releases'
            }
            $zip = [IO.Compression.ZipFile]::OpenRead($package)
            try {
                $entries = @($zip.Entries | Where-Object { $_.FullName -like '*.nuspec' })
                if ($entries.Count -ne 1) { throw "Expected one nuspec in $package" }
                $reader = [IO.StreamReader]::new($entries[0].Open())
                try { [xml]$xml = $reader.ReadToEnd() } finally { $reader.Dispose() }
                $actual = $xml.SelectSingleNode("//*[local-name()='metadata']/*[local-name()='version']").InnerText
                $id = $xml.SelectSingleNode("//*[local-name()='metadata']/*[local-name()='id']").InnerText
                if ($actual -cne $version -or $id -cne 'SpatialSoftReceiver') { throw "Package identity mismatch: $package" }
                $packages += @{ path = $package; version = $actual; id = $id }
            } finally { $zip.Dispose() }
        }
        @{ packages = $packages; setup_sha256 = (Get-FileHash -LiteralPath $Setup -Algorithm SHA256).Hash }
    }
    Invoke-Stage 'install' {
        $script:ownedInstall = $true
        Invoke-Child $Setup @('--silent')
    }
    Invoke-Stage 'installed-files-and-old-version' {
        Wait-Condition { (Read-InstalledVersion) -ceq $OldVersion } "installed version $OldVersion"
        $files = @('spatial-softreceiver.exe', 'ssr-guard.exe', 'ssr-setup.exe', 'ssr-plughost.exe')
        foreach ($file in $files) {
            if (-not (Test-Path -LiteralPath (Join-Path $installRoot "current\$file") -PathType Leaf)) { throw "Missing $file" }
        }
        Stop-InstalledProcesses
        @{ version = (Read-InstalledVersion); files = $files }
    }
    Invoke-Stage 'shortcut-aumid' {
        if (-not (Test-Path -LiteralPath $shortcut -PathType Leaf)) { throw "Missing shortcut: $shortcut" }
        $shell = New-Object -ComObject Shell.Application
        try {
            $folder = $shell.NameSpace([IO.Path]::GetDirectoryName($shortcut))
            $item = $folder.ParseName([IO.Path]::GetFileName($shortcut))
            $aumid = $item.ExtendedProperty('System.AppUserModel.ID')
            if ($aumid -cne 'io.github.115dkk.spatial-softreceiver') { throw "Shortcut AUMID: '$aumid'" }
            @{ path = $shortcut; aumid = $aumid }
        } finally {
            if ($null -ne $shell) { [Runtime.InteropServices.Marshal]::FinalReleaseComObject($shell) | Out-Null }
        }
    }
    Invoke-Stage 'uninstall-registration' {
        $entry = Get-ItemProperty -LiteralPath $uninstallKey
        if ($entry.DisplayVersion -cne $OldVersion -or $entry.UninstallString -notlike '*Update.exe*uninstall*') {
            throw 'Unexpected HKCU uninstall registration'
        }
        @{ key = $uninstallKey; version = $entry.DisplayVersion; command = $entry.UninstallString }
    }
    Invoke-Stage 'update-once' {
        $child = Invoke-Child (Join-Path $installRoot 'current\spatial-softreceiver.exe') @('--update-once', '--result', $resultPath) 120 $Releases
        $lines = @(Get-Content -LiteralPath $resultPath)
        if ($lines.Count -ne 1) { throw 'Expected exactly one update result line' }
        $result = $lines[0] | ConvertFrom-Json
        if ($result.checked -cne $true -or $result.found -cne $NewVersion -or $result.applied -cne $true) {
            throw "Update was not requested: $($lines[0])"
        }
        @{ child = $child; result = $result }
    }
    Invoke-Stage 'updated-version' {
        Wait-Condition { (Read-InstalledVersion) -ceq $NewVersion } "updated version $NewVersion" 180
        # A manifest can be replaced before the updater finishes registration.
        Wait-Condition {
            $busy = $false
            foreach ($process in @(Get-Process -Name Update -ErrorAction SilentlyContinue)) {
                try {
                    if ($process.Path -and $process.Path.Equals((Join-Path $installRoot 'Update.exe'), [StringComparison]::OrdinalIgnoreCase)) {
                        $busy = $true
                    }
                } finally { $process.Dispose() }
            }
            -not $busy
        } 'updater process completion' 60
        $entry = Get-ItemProperty -LiteralPath $uninstallKey
        if ($entry.DisplayVersion -cne $NewVersion) { throw 'Updated registry version did not match manifest' }
        # apply_after_exit preserves its normal restart semantics. The restarted
        # app may create a guard; uninstall itself must cope with those processes.
        @{ version = (Read-InstalledVersion); manifest = (Join-Path $installRoot 'current\sq.version') }
    }
    Invoke-Stage 'uninstall' {
        if (Test-Path -LiteralPath $appLog) { $script:logOffset = (Get-Item -LiteralPath $appLog).Length }
        Invoke-Child (Join-Path $installRoot 'Update.exe') @('--silent', 'uninstall')
    }
    Invoke-Stage 'removed' {
        Wait-Condition {
            -not (Test-Path -LiteralPath $installRoot) -and -not (Test-Path -LiteralPath $shortcut) -and
                -not (Test-Path -LiteralPath $uninstallKey)
        } 'install folder, Start Menu shortcut and HKCU registration removal'
        @{ install = $installRoot; shortcut = $shortcut; registry = $uninstallKey }
    }
    Invoke-Stage 'uninstall-hook' {
        Wait-Condition { (Read-AppLogSuffix) -match '(?m)^\d+\.\d+ update: before-uninstall hook\r?$' } 'fresh uninstall hook log' 15
        $text = Read-AppLogSuffix
        if ($text -notmatch 'shell: last resort: there is no restore point; the roles are not ours') {
            throw 'Uninstall hook did not reach the expected no-restore-point path'
        }
        $logArtifact = Join-Path $artifactDir "uninstall-hook-$runId.log"
        [IO.File]::WriteAllText($logArtifact, $text)
        @{ source = $appLog; offset = $logOffset; artifact = $logArtifact }
    }
    $report.ok = $true
} catch {
    $report.error = $_.Exception.Message
} finally {
    # Never remove files/registry entries by hand: doing so would hide defects.
    # Retry the documented uninstaller only for an installation this run owns.
    if ($ownedInstall -and -not $report.ok) {
        try {
            $updater = Join-Path $installRoot 'Update.exe'
            if (Test-Path -LiteralPath $updater) {
                Stop-InstalledProcesses
                $report.cleanup = Invoke-Child $updater @('--silent', 'uninstall')
            } else { $report.cleanup = @{ status = 'updater-absent' } }
        } catch { $report.cleanup = @{ error = $_.Exception.Message } }
    }
    foreach ($name in $stageNames) {
        if ($name -notin @($report.stages | ForEach-Object { $_.name })) {
            $report.stages.Add(@{ name = $name; status = 'skipped'; seconds = 0; detail = 'Earlier stage failed' })
        }
    }
    $report.finished = [DateTime]::UtcNow.ToString('o')
    Save-Summary
}
if (-not $report.ok) { Write-Error -Message $report.error -ErrorAction Continue; exit 1 }
exit 0
