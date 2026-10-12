# diskdash reliability counters (read-only; needs administrator).
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$ns = 'root\Microsoft\Windows\Storage'
@(Get-CimInstance -Namespace $ns -ClassName MSFT_PhysicalDisk | ForEach-Object {
    $disk = $_
    try {
        $r = Get-CimAssociatedInstance -InputObject $disk -ResultClassName MSFT_StorageReliabilityCounter -ErrorAction Stop
        [pscustomobject]@{
            DeviceId         = [string]$disk.DeviceId
            Temperature      = if ($null -ne $r.Temperature) { [uint32]$r.Temperature } else { $null }
            Wear             = if ($null -ne $r.Wear) { [uint32]$r.Wear } else { $null }
            PowerOnHours     = if ($null -ne $r.PowerOnHours) { [uint64]$r.PowerOnHours } else { $null }
            ReadErrorsTotal  = if ($null -ne $r.ReadErrorsTotal) { [uint64]$r.ReadErrorsTotal } else { $null }
            WriteErrorsTotal = if ($null -ne $r.WriteErrorsTotal) { [uint64]$r.WriteErrorsTotal } else { $null }
        }
    } catch { }
}) | ConvertTo-Json -Depth 3 -Compress
