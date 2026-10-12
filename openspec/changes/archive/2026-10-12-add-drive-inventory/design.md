## Context

The target machine as probed on 2026-10-09:
- C: on a Lexar 512 GB NVMe
- G: on a Samsung 850 EVO 250 GB SATA SSD
- D: on a WD 1 TB SATA HDD
- one file-backed virtual disk without a letter (out of scope)
- one optical drive E: (out of scope)

In a non-admin session, `Get-PhysicalDisk` and `Get-Partition` work, `Get-StorageReliabilityCounter` fails with "Access to a CIM resource was not available", and `fsutil behavior query` works. So the reduced/elevated split is real and has to be designed in.

## Goals / Non-Goals

**Goals:**
- Accurate volume → disk → media-class mapping without admin rights.
- Health data that is honest about what it doesn't know.
- No hard dependency on smartctl.

**Non-Goals:**
- Running SMART self-tests, or changing any disk or OS setting (TRIM, write cache).
- USB/removable, optical, network and virtual drives. The owner limited scope to fixed internal drives.
- RAID controller pass-through. smartctl `-d` device types beyond auto-detect are out of scope.
- Predicting disk failure beyond simple thresholds.

## Decisions

### D1. PowerShell + CIM via subprocess, JSON output
A single `powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command <script>` call runs a fixed script embedded in the binary (not built from user input). The script gathers `Get-Volume`, `Get-Partition`, `Get-PhysicalDisk`, and `Get-Disk` (as raw `MSFT_*` CIM classes, so enum values are numeric and locale-independent) and emits one JSON document. Network and optical letters are counted natively with `GetDriveTypeW`, which reads only local mapping state. The reliability-counter query runs in a separate call with its own timeout, so a slow or failing counter query can't block the basic inventory.
- *Alternative: WMI/CIM directly from Rust (`wmi` crate over COM).* It avoids ~0.5–1 s of PowerShell startup, but the MSFT_* storage classes are awkward through COM (method calls, embedded instances). PowerShell startup cost is fine at a 60 s refresh rate. If profiling shows a problem, the volume/disk queries can move to the `wmi` crate later without changing the API.
- *Alternative: raw `DeviceIoControl` (IOCTL_STORAGE_QUERY_PROPERTY) via the `windows` crate.* It is the fastest and needs no admin for the seek-penalty and bus type. We keep it as a fallback for classification if PowerShell is unavailable (restricted environments), but it is not the primary path.

### D2. Scope filter, then classification
The scope filter runs first, on BusType. USB, File Backed Virtual, Virtual, SD and MMC are excluded, and so is anything whose volume DriveType is not Fixed. USB enclosures can report BusType SCSI/SATA through some bridges, so `Get-Disk.IsSystem`/`IsBoot` and the PnP `Removable` capability are also checked. BusType is checked before MediaType because enclosures and virtual disks often lie about media type. For in-scope disks, NVMe bus → NVMe SSD, then MediaType, then `SpindleSpeed` (0 = SSD) as the tie-breaker.

### D3. Volume identity
The key is the volume GUID path (`\\?\Volume{…}\`), not the drive letter, because letters can be reassigned. Snapshots, samples and history all reference `volume_id`. Each volume also records its serial number, to detect a reformatted volume that kept its GUID.

### D4. Health aggregation as "worst of", with reasons
Every input contributes a status and a reason string. The UI shows the reasons, so a "warning" is always explainable. Thresholds (60 °C HDD, 70 °C SSD, 90% wear) are constants in one module and covered by unit tests.

### D5. smartctl adapter with an argument allow-list; elevated sources via a channel
`smartctl --scan-open -j` discovers devices; per device we run `smartctl -i -H -A -j <dev>`. The adapter builds argv from a frozen allow-list and never through a shell. Matching smartctl devices to Windows disk numbers uses the serial number.

smartctl and the reliability counters need admin. The web server never elevates itself, so these sources sit behind a `HealthSource` trait with two implementations:
- direct, when the user started the app elevated;
- the read-only helper's `SmartCounters` operation, added in phase 11, which wraps the same allow-listed calls.

### D6. Refresh cadence and sampling
Inventory is cheap (~1–2 s), so it refreshes every 60 s while the UI has an SSE subscriber, and on demand otherwise. Free-space samples are coalesced into 15-minute buckets, which gives about 35k rows per volume per year — negligible.

### D7. Network drives
These are only counted, via `GetDriveTypeW` (DRIVE_REMOTE), which reads local mapping state. WMI (`Win32_LogicalDisk`) is deliberately not used, because its provider may query the share. The share is never queried. This keeps safety rule 2: SMB traffic counts as network traffic.

### D8. Low-space rule: percentage OR floor
The owner chose "flag below 10% **or** below 20 GiB". The reason (`percent` / `floor`) is returned, so the user can tune whichever condition is noisy. On the 931 GiB HDD the percentage condition dominates at 93 GiB. If that turns out to be too strict, the percentage can be lowered in Settings.

## Risks / Trade-offs

- [PowerShell may be blocked by policy, or start slowly] → `-ExecutionPolicy Bypass` applies only to our inline command. The native IOCTL fallback (D1) still provides letters, sizes and bus type.
- [The serial number reported by `Get-PhysicalDisk` can differ in format from smartctl's (padding, byte order on some SATA drivers)] → Normalise by stripping whitespace and comparing case-insensitively, then fall back to model + size matching.
- [Some NVMe drives don't expose temperature through reliability counters] → The field shows as Unavailable, with "not reported by this drive".
- [A USB bridge that reports a SATA bus slips past the filter] → Secondary checks (PnP removable capability, Get-Disk `BusType` vs `Get-PhysicalDisk`) apply. Worst case, an external drive is shown, which is harmless because the app is read-only.

## Open Questions

None. Resolved: fixed internal drives only; low space = below 10% OR below 20 GiB; elevated health data via the read-only helper.
