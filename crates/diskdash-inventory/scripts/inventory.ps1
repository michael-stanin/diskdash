# diskdash inventory query (read-only). Emits one JSON document.
# Uses raw CIM classes so enum values are numeric and stable across locales.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$ns = 'root\Microsoft\Windows\Storage'

function Safe([scriptblock]$Block) {
    try { @(& $Block) } catch { @() }
}

$volumes = Safe { Get-CimInstance -Namespace $ns -ClassName MSFT_Volume | ForEach-Object {
    [pscustomobject]@{
        Path               = $_.Path
        DriveLetter        = if ($_.DriveLetter) { [string]$_.DriveLetter } else { $null }
        FileSystemLabel    = $_.FileSystemLabel
        FileSystem         = $_.FileSystem
        DriveType          = [int]$_.DriveType
        Size               = [uint64]$_.Size
        SizeRemaining      = [uint64]$_.SizeRemaining
        AllocationUnitSize = [uint64]$_.AllocationUnitSize
        HealthStatus       = [int]$_.HealthStatus
    } } }

$partitions = Safe { Get-CimInstance -Namespace $ns -ClassName MSFT_Partition | ForEach-Object {
    [pscustomobject]@{
        DiskNumber  = [int]$_.DiskNumber
        DriveLetter = if ($_.DriveLetter -and [int][char]$_.DriveLetter -ne 0) { [string]$_.DriveLetter } else { $null }
        AccessPaths = @($_.AccessPaths)
        IsSystem    = [bool]$_.IsSystem
        IsBoot      = [bool]$_.IsBoot
    } } }

$physical = Safe { Get-CimInstance -Namespace $ns -ClassName MSFT_PhysicalDisk | ForEach-Object {
    [pscustomobject]@{
        DeviceId          = [string]$_.DeviceId
        FriendlyName      = $_.FriendlyName
        SerialNumber      = $_.SerialNumber
        MediaType         = [int]$_.MediaType
        BusType           = [int]$_.BusType
        SpindleSpeed      = [uint32]$_.SpindleSpeed
        Size              = [uint64]$_.Size
        HealthStatus      = [int]$_.HealthStatus
        OperationalStatus = @($_.OperationalStatus | ForEach-Object { [int]$_ })
    } } }

$disks = Safe { Get-CimInstance -Namespace $ns -ClassName MSFT_Disk | ForEach-Object {
    [pscustomobject]@{
        Number       = [int]$_.Number
        BusType      = [int]$_.BusType
        IsBoot       = [bool]$_.IsBoot
        IsSystem     = [bool]$_.IsSystem
        SerialNumber = $_.SerialNumber
        FriendlyName = $_.FriendlyName
    } } }

# Network/optical letters are counted natively (GetDriveTypeW), never via WMI.

[pscustomobject]@{
    volumes    = $volumes
    partitions = $partitions
    physical   = $physical
    disks      = $disks
} | ConvertTo-Json -Depth 5 -Compress
