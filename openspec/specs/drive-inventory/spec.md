# drive-inventory Specification

## Purpose
TBD - created by archiving change add-drive-inventory. Update Purpose after archive.
## Requirements
### Requirement: Volume enumeration
The system SHALL enumerate all mounted volumes that have a drive letter or mount path and that reside on in-scope physical disks (see "Scope limited to fixed internal drives"). For each, it SHALL report: a stable volume id (the volume GUID path), drive letter(s), label, filesystem (NTFS, ReFS, exFAT, FAT32, other), cluster size, total capacity in bytes, free bytes, used bytes, and used percentage.

#### Scenario: Fixed NTFS volume
- **WHEN** the inventory is refreshed on a machine with an NTFS volume `C:` on an internal NVMe disk
- **THEN** `GET /api/v1/drives` includes an entry for `C:` with filesystem `NTFS`, a capacity > 0, `free + used == capacity`, and a cluster size

#### Scenario: Volume without a drive letter
- **WHEN** an in-scope volume is mounted only on a folder path
- **THEN** it is listed with that mount path and an empty letter list

### Requirement: Scope limited to fixed internal drives
The system SHALL include only volumes whose physical disks are fixed internal drives, meaning bus type SATA, SAS, NVMe, ATA, SCSI or RAID, and drive type fixed. It MUST exclude:
- USB and other removable drives;
- optical drives;
- network (mapped or UNC) drives;
- virtual disks (file-backed VHD/VHDX), whose space is already counted inside their host file.

Excluded volumes MUST NOT be measured, beyond what is needed to identify them. Network drives MUST NOT be contacted at all. The response SHALL include only a count of excluded volumes per reason, so the UI can say "2 drives not shown (USB, optical)".

#### Scenario: USB stick attached
- **WHEN** a USB flash drive is mounted as `F:`
- **THEN** `F:` is not in the drive list, and `excluded.usb` is 1

#### Scenario: Mapped network drive
- **WHEN** `Z:` is mapped to `\\nas\media`
- **THEN** `Z:` is not listed, no call is made that reaches the share, and `excluded.network` is 1

#### Scenario: Optical drive
- **WHEN** the machine has an optical drive `E:`
- **THEN** `E:` is not listed, and `excluded.optical` is 1

### Requirement: Volume-to-physical-disk mapping
The system SHALL map each in-scope volume to the physical disk(s) it resides on. For each physical disk it SHALL report: disk number, model (friendly name), serial number, bus type, media class, spindle speed when known, and size. A volume spanning several disks (a storage space or dynamic volume) SHALL list all of them.

#### Scenario: Single-disk volume
- **WHEN** volume `D:` is a partition on disk 0
- **THEN** the drive entry for `D:` references physical disk 0 with its model and serial

#### Scenario: Mapping unavailable
- **WHEN** the partition-to-disk query fails or returns nothing for a fixed volume
- **THEN** the volume is still listed, with `physical_disks: []` and media class `UNKNOWN`, and the failure is logged

### Requirement: Media classification
The system SHALL classify each in-scope physical disk as exactly one of `HDD`, `SATA_SSD`, `NVME_SSD` or `UNKNOWN`, using these rules in order:
1. BusType `NVMe` → `NVME_SSD`.
2. MediaType `SSD`, or SpindleSpeed 0 → `SATA_SSD`.
3. MediaType `HDD`, or SpindleSpeed > 0 → `HDD`.
4. Otherwise → `UNKNOWN`.

The UI SHALL show the class as a badge. Downstream features SHALL treat `UNKNOWN` as `HDD` for scheduling.

#### Scenario: NVMe disk
- **WHEN** a physical disk reports BusType `NVMe` and MediaType `SSD`
- **THEN** it is classified `NVME_SSD`

#### Scenario: SATA HDD with unknown spindle speed
- **WHEN** a disk reports BusType `SATA`, MediaType `HDD`, and SpindleSpeed unknown
- **THEN** it is classified `HDD`

#### Scenario: Unspecified media type
- **WHEN** an internal SATA disk reports MediaType `Unspecified` and no spindle speed
- **THEN** it is classified `UNKNOWN`, and the UI shows "media type unknown"

### Requirement: Low free-space warning
The system SHALL flag a volume as `low_space` when its free space is below the percentage threshold (setting, default 10% of capacity) **or** below the absolute floor (setting, default 20 GiB). The response SHALL state which condition triggered it. The fill bar SHALL use the warning colour when `low_space` is true, the caution colour when used space is 75% or more, and the normal colour otherwise. The Home page SHALL show a warning banner listing every flagged volume.

#### Scenario: Below percentage threshold
- **WHEN** volume `D:` (931 GiB) has 80 GiB free (8.6%), with defaults
- **THEN** it is flagged `low_space` with reason `percent`

#### Scenario: Below absolute floor only
- **WHEN** volume `G:` (232 GiB) has 18 GiB free (7.8%) with the percentage threshold set to 5%
- **THEN** it is flagged `low_space` with reason `floor`

#### Scenario: Thresholds changed
- **WHEN** the user lowers the percentage to 5% and the floor to 10 GiB, and `C:` has 30 GiB free of 476 GiB (6.3%)
- **THEN** on the next inventory response `C:` is no longer flagged

### Requirement: Inventory refresh and caching
The system SHALL cache inventory results and serve `GET /api/v1/drives` from the cache. It SHALL refresh the cache on `POST /api/v1/drives/refresh`, on startup, and at most every 60 seconds while at least one UI client is connected to the event stream. Refreshes SHALL run off the request path, with each PowerShell call limited by a 15-second timeout.

#### Scenario: PowerShell call hangs
- **WHEN** the physical-disk query has not returned after 15 seconds
- **THEN** the call is terminated, the previous cached values are kept and marked `stale: true`, and the error is logged

### Requirement: Free-space sampling
On every successful inventory refresh, the system SHALL store one sample per in-scope volume (`volume_id`, timestamp, capacity, free bytes) in `volume_samples`. It SHALL store at most one sample per volume per 15 minutes, keeping the latest sample in each window.

#### Scenario: Frequent refreshes are coalesced
- **WHEN** the inventory refreshes 10 times within 5 minutes
- **THEN** `volume_samples` gains at most one row per volume for that 15-minute window

### Requirement: Home overview
The Home page SHALL open with a summary strip of stat cards: total capacity, total free, number of drives with low space, and (from phase 5 onward) potential savings. Below the strip, a responsive grid SHALL show one card per in-scope volume, built from the shared `ui-foundation` components. Each card SHALL show:
- the drive letter and label;
- the media badge;
- the filesystem;
- a large free-space figure;
- the fill bar;
- the health pill;
- a "Scan" / "View" action, depending on whether a snapshot exists.

The page SHALL show skeleton cards while the first inventory loads, and an error state with retry if the inventory fails.

#### Scenario: First load
- **WHEN** Home opens before the first inventory response
- **THEN** skeleton cards are shown in the grid layout, then replaced by real cards without layout shift

#### Scenario: Drive with no scan yet
- **WHEN** a volume has no snapshot
- **THEN** its card's primary action is "Scan", and the card notes "Not scanned yet"

#### Scenario: Inventory failure
- **WHEN** the inventory request fails and no cache exists
- **THEN** Home shows the error state with "Retry" instead of empty cards

### Requirement: Drives API
The system SHALL expose:
- `GET /api/v1/drives`: in-scope volumes, each with its physical disks, health summary, `low_space` flag and reason, and `stale` flag, plus the excluded counts;
- `GET /api/v1/drives/{volume_id}`: full detail, including all health fields;
- `POST /api/v1/drives/refresh`: triggers a refresh and returns the job id.

Sizes SHALL be integer bytes.

#### Scenario: Unknown volume id
- **WHEN** `GET /api/v1/drives/{id}` is requested for an id not in the inventory
- **THEN** the server responds 404

