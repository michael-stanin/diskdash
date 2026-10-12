# drive-health Specification

## Purpose
TBD - created by archiving change add-drive-inventory. Update Purpose after archive.
## Requirements
### Requirement: Health data tiers
The system SHALL collect disk health from up to three sources and SHALL report, for every health field, both its value (or null) and its `source` (`storage_cmdlets`, `reliability_counters`, `smartctl`, or `unavailable`). Elevated sources (reliability counters, smartctl) SHALL be read only through an elevated, read-only channel: the `SmartCounters` operation of the read-only helper (`add-fast-ntfs-scan`), or directly when the user has started the app elevated. The web server MUST NOT elevate itself. Missing data MUST be shown as "Unavailable", with the reason (for example "requires administrator" or "not reported by this drive"), and never as zero.

#### Scenario: Reduced mode
- **WHEN** the app runs without elevation and smartctl is not installed
- **THEN** each disk reports `health_status` and `operational_status` from the storage cmdlets
- **AND** temperature, power-on hours and wear are null with reason "requires administrator"

#### Scenario: Elevated data available
- **WHEN** the read-only helper is running (phase 11), or the app was started elevated
- **THEN** temperature, power-on hours, wear (SSD) and read/write error totals are populated from reliability counters where the drive reports them

#### Scenario: smartctl present
- **WHEN** `smartctl.exe` is found on PATH or in `C:\Program Files\smartmontools\bin`, and elevated data is available (helper running or app elevated)
- **THEN** reallocated sector count (HDD), data units written / TBW (SSD), and the SMART overall assessment are populated with source `smartctl`

### Requirement: Overall health status
The system SHALL report, for each physical disk, an overall status of `healthy`, `warning`, `unhealthy` or `unknown`, computed as the worst of these:
- the storage cmdlet HealthStatus (Healthy / Warning / Unhealthy);
- the SMART overall assessment when available (PASSED → healthy, FAILED → unhealthy);
- a reallocated sector count > 0 (warning);
- wear ≥ 90% (warning);
- temperature above 60 °C for HDD or 70 °C for SSD (warning).

#### Scenario: Reallocated sectors present
- **WHEN** smartctl reports 8 reallocated sectors on an HDD whose cmdlet status is Healthy
- **THEN** the overall status is `warning`, and the reason lists "8 reallocated sectors"

#### Scenario: Nothing known
- **WHEN** the disk reports no HealthStatus and no other source is available
- **THEN** the overall status is `unknown`

### Requirement: TRIM state
The system SHALL report whether TRIM (delete notifications) is enabled for NTFS and ReFS, by parsing `fsutil behavior query DisableDeleteNotify`. `DisableDeleteNotify = 0` SHALL be reported as TRIM **enabled**. For SSD classes, the UI SHALL show a warning when TRIM is disabled.

#### Scenario: TRIM enabled
- **WHEN** fsutil reports `NTFS DisableDeleteNotify = 0`
- **THEN** `trim_enabled.ntfs` is `true`, and SSD cards show no TRIM warning

#### Scenario: TRIM disabled on an SSD system
- **WHEN** fsutil reports `NTFS DisableDeleteNotify = 1` and at least one SSD is present
- **THEN** SSD detail panels show "TRIM is disabled", with the command to enable it shown as text only

#### Scenario: fsutil fails
- **WHEN** the fsutil call fails or its output cannot be parsed
- **THEN** `trim_enabled` is null with the reason, and no warning is shown

### Requirement: smartctl usage is optional and read-only
The system SHALL use smartctl only if it is already installed. It SHALL call smartctl only with read-only query flags (`--scan-open`, `-i`, `-H`, `-A`, `-j`), and MUST NOT pass any option that runs self-tests, changes settings, or writes to the device. A missing or failing smartctl MUST NOT stop the rest of the inventory.

#### Scenario: smartctl absent
- **WHEN** smartctl is not installed
- **THEN** the inventory completes, the settings page says "smartctl not found (optional)", and the app attempts no download

#### Scenario: Command allow-list enforced
- **WHEN** code attempts to call the smartctl adapter with an argument outside the allow-list
- **THEN** the adapter raises an error before starting a process, and a unit test covers this

### Requirement: Health detail view
The drive detail panel SHALL show, for each physical disk under the volume: model, serial, bus type, media class, size, overall status with reasons, temperature, power-on hours, wear % (SSD), data written (SSD, when available), reallocated sectors (HDD, when available), TRIM state (SSD), and each field's source. Unavailable fields SHALL show their reason.

#### Scenario: Viewing an NVMe disk in reduced mode
- **WHEN** the user opens the detail panel for `C:` on an NVMe disk without elevation
- **THEN** model, serial, bus type `NVMe` and overall status are shown, and temperature reads "Unavailable — requires administrator"

