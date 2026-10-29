# Reads, and with -Set chooses, the Windows spatial sound format of one of our
# endpoints through the public WinRT API Windows.Media.Audio.
# SpatialAudioDeviceConfiguration (Windows 10 2004 and later), the one the
# Settings app's "Spatial audio" list stands on. Windows PowerShell 5.1 only:
# PowerShell 7 has no WinRT projection.
#
#   powershell.exe -NoProfile -ExecutionPolicy Bypass -File tools/ci/spatial-format.ps1 -Endpoint 0
#   powershell.exe ... -Endpoint 0 -Set DolbyAtmosForHomeTheater
#
# Prints one JSON object: whether the endpoint takes spatial sound, which
# formats it supports, the active and default formats, and the status of the
# -Set request. A step of the probe that cannot run is reported in `error`
# instead of failing, so the MAT probe job shows how far a runner gets.
param(
    [ValidateRange(0, 2)] [int] $Endpoint = 0,
    [string] $Set = '',
    # Another render endpoint's MMDevice id instead of ours (trying the script
    # on a PC without the driver).
    [string] $MmDeviceId = ''
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object Text.UTF8Encoding($false)

# MMDevice ids map to WinRT device interface ids under the audio render class.
$RenderInterfaceClass = '{e6327cad-dcec-4949-ae8a-991e976a79d2}'
$Formats = 'WindowsSonic', 'DolbyAtmosForHeadphones', 'DolbyAtmosForHomeTheater', 'DolbyAtmosForSpeakers',
    'DTSHeadphoneX', 'DTSXUltra', 'DTSXForHomeTheater'

$result = [ordered]@{ endpoint = $Endpoint; mmdevice_id = $null; device_id = $null; spatial_supported = $null
    formats = [ordered]@{}; active = $null; default = $null; set = $null; error = $null }

function Resolve-WinRtType([string] $name) {
    foreach ($assembly in 'Windows.Media.Audio', 'Windows.Media', 'Windows.Foundation.UniversalApiContract') {
        try {
            $type = [type]"$name, $assembly, ContentType=WindowsRuntime"
            if ($type) { return $type }
        } catch { }
    }
    throw "the WinRT type $name is not available on this Windows"
}

function Wait-Operation($operation, [type] $resultType) {
    $asTask = [System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
        $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and
        $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1'
    } | Select-Object -First 1
    $task = $asTask.MakeGenericMethod($resultType).Invoke($null, @($operation))
    if (-not $task.Wait(30000)) { throw 'the WinRT operation did not finish in 30 s' }
    $task.Result
}

try {
    if ($MmDeviceId) {
        $result.mmdevice_id = $MmDeviceId
    } else {
        # The helper's objects, not its JSON: Windows PowerShell's
        # ConvertFrom-Json hands an array over as one object, and a filter on
        # endpoint 0 then reads as false.
        . (Join-Path $PSScriptRoot 'endpoints.ps1')
        $ours = @(Get-SpatialEndpoints) | Where-Object { $_.endpoint -eq $Endpoint } | Select-Object -First 1
        if (-not $ours) { throw "endpoint $Endpoint of our driver is not present" }
        $result.mmdevice_id = $ours.id
    }
    $result.device_id = "\\?\SWD#MMDEVAPI#$($result.mmdevice_id)#$RenderInterfaceClass"

    Add-Type -AssemblyName System.Runtime.WindowsRuntime
    $configType = Resolve-WinRtType 'Windows.Media.Audio.SpatialAudioDeviceConfiguration'
    $subtypeType = Resolve-WinRtType 'Windows.Media.Audio.SpatialAudioFormatSubtype'
    $config = $configType::GetForDeviceId($result.device_id)
    $result.spatial_supported = [bool]$config.IsSpatialAudioSupported

    $names = @{}
    foreach ($format in $Formats) {
        try {
            $subtype = [string]$subtypeType::$format
            if (-not $subtype) { continue }
            $names[$subtype.ToLowerInvariant()] = $format
            $result.formats[$format] = [bool]$config.IsSpatialAudioFormatSupported($subtype)
        } catch {
            $result.formats[$format] = "unknown: $($_.Exception.Message)"
        }
    }
    $named = { param($subtype) if (-not $subtype) { 'none' } elseif ($names[$subtype.ToLowerInvariant()]) { $names[$subtype.ToLowerInvariant()] } else { $subtype } }
    $result.active = & $named ([string]$config.ActiveSpatialAudioFormat)
    $result.default = & $named ([string]$config.DefaultSpatialAudioFormat)

    if ($Set) {
        $subtype = [string]$subtypeType::$Set
        if (-not $subtype) { throw "unknown spatial format $Set" }
        $resultType = Resolve-WinRtType 'Windows.Media.Audio.SetDefaultSpatialAudioFormatResult'
        $outcome = Wait-Operation ($config.SetDefaultSpatialAudioFormatAsync($subtype)) $resultType
        $result.set = [ordered]@{ format = $Set; status = [string]$outcome.Status }
        $result.active = & $named ([string]$config.ActiveSpatialAudioFormat)
        $result.default = & $named ([string]$config.DefaultSpatialAudioFormat)
    }
} catch {
    $result.error = $_.Exception.Message
}
ConvertTo-Json -InputObject $result -Depth 4 -Compress
