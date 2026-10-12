## Why

Before deciding what could be deleted or moved, the user needs to see every internal drive at once: how full it is, what kind of hardware it sits on (HDD / SATA SSD / NVMe), and whether that hardware is healthy. The media type also feeds later phases. Scans are scheduled per spindle in `add-filesystem-scan`, and tiering suggestions need to know which drive is the SSD.

## What Changes

- Add an inventory service that collects volumes and maps each one to its partition and physical disk, using PowerShell storage cmdlets that output JSON. Results are cached and refreshed on demand or every 60 s while the UI is open.
- Limit scope to **fixed internal drives**. USB/removable, optical, network and virtual drives are excluded and only counted ("2 drives not shown"). Network shares are never contacted.
- Classify in-scope media as `HDD`, `SATA_SSD`, `NVME_SSD` or `UNKNOWN`, using BusType, MediaType and SpindleSpeed.
- Add health collection.
  - Without admin: overall health status, TRIM state, and operational status.
  - With admin: temperature, power-on hours, wear %, and read/write error counters via `Get-StorageReliabilityCounter`.
  - With `smartctl` already installed: reallocated sectors, TBW/data units written, and the SMART overall assessment.
- Add a low-free-space evaluation: flagged when free space is below 10% **or** below 20 GiB (both configurable). The reason is reported.
- Record each volume's free space in a `volume_samples` table on every refresh, so `add-history-and-trends` has a time series from day one.
- API: `GET /api/v1/drives` and `GET /api/v1/drives/{volume_id}`; `POST /api/v1/drives/refresh`.
- UI: the Home page shows one card per volume (letter, label, FS, type badge, fill bar coloured by threshold, used/free in bytes and %, health pill, warning banner). A drive detail panel shows the physical disk and all available health fields, with "not available in reduced mode" where applicable.

Safety rules touched: **2** (network drives are never contacted) and **3** (read-only; no file is opened, and TRIM/settings are shown as text only).

## Capabilities

### New Capabilities
- `drive-inventory`: Enumerating in-scope volumes and physical disks, the fixed-internal scope filter, mapping volumes to disks, media classification, capacity and free space, low-space warnings, and volume free-space sampling.
- `drive-health`: SMART and reliability information per physical disk, TRIM state, and how availability varies between reduced mode, elevated mode and smartctl-enhanced mode.

### Modified Capabilities
None.

## Impact

- Backend: new crate `crates/diskdash-inventory` (PowerShell runner, serde parsers, scope filter, classifier, health aggregation, smartctl adapter), new migration `0002_inventory.sql` (`volumes`, `physical_disks`, `volume_samples`), and the routes above.
- Frontend: Home page drive cards, the drive detail panel, and a settings field for the threshold (already in settings, now wired up).
- No heavy new dependencies (serde_json, `windows` for the IOCTL fallback). PowerShell 5.1 is built in. `smartctl` is optional and detected on PATH and in the default install folder, never installed by us.

## Depends on

`add-project-skeleton`.
