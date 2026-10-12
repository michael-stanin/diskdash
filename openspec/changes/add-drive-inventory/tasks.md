## 1. Data model

- [ ] 1.1 Write migration `0002_inventory.sql`: `physical_disks`, `volumes` (keyed by volume GUID, with serial), `volume_disks` link, `volume_samples` (volume_id, ts, capacity, free); test that the migration applies cleanly
- [ ] 1.2 Add serde/ts-rs types for Volume, PhysicalDisk, HealthField (value, source, reason), DriveSummary and ExcludedCounts

## 2. Collection

- [ ] 2.1 Add a PowerShell runner (fixed embedded scripts, `-NoProfile -NonInteractive`, 15 s timeout, JSON parse, stderr captured to the log); unit test the timeout handling with a fake runner
- [ ] 2.2 Write the basic inventory script (volumes, partitions, disks, `Win32_LogicalDisk` for counting network/optical); parser tests using recorded JSON fixtures from this machine (NVMe, SATA SSD, HDD, virtual, optical)
- [ ] 2.3 Implement the scope filter (exclude USB, removable, optical, network, virtual, with excluded counts per reason) and the media classifier; table-driven tests, including a USB bridge reporting SATA
- [ ] 2.4 Unit test that no measuring call is ever made for network or excluded drives
- [ ] 2.5 Add the native fallback via the `windows` crate (`GetLogicalDriveStringsW`, `GetDiskFreeSpaceExW`, `GetVolumeInformationW`, `IOCTL_STORAGE_QUERY_PROPERTY` bus type), used when PowerShell fails; test it with PowerShell mocked to fail

## 3. Health

- [ ] 3.1 Add the reliability-counter script behind a `HealthSource` trait (direct when the app is elevated now; the helper's `SmartCounters` operation plugs in with phase 11); parser tests on recorded fixtures; in reduced mode, verify the fields are `unavailable` with "requires administrator"
- [ ] 3.2 Add the fsutil TRIM parser (NTFS/ReFS lines, `0` = enabled); tests for enabled, disabled and unparseable output
- [ ] 3.3 Add the smartctl adapter: discovery (PATH + default folder), argv allow-list, JSON parse for reallocated sectors / data units written / overall assessment, serial matching; tests include allow-list rejection and serial normalisation
- [ ] 3.4 Implement overall-status aggregation (worst-of, with reasons); table-driven tests for each threshold

## 4. Service and API

- [ ] 4.1 Add the inventory service with cache, `stale` marking on failure, a 60 s auto-refresh while SSE subscribers exist, and refresh as a background job
- [ ] 4.2 Add free-space sampling with 15-minute coalescing; test that 10 refreshes produce 1 row per window
- [ ] 4.3 Add the routes `GET /api/v1/drives`, `GET /api/v1/drives/{id}` (404 on unknown) and `POST /api/v1/drives/refresh`; API tests with the service mocked
- [ ] 4.4 Compute low-space evaluation (percentage OR floor, with reason) from settings; test both reasons and the threshold change scenario

## 5. UI

- [ ] 5.1 Build the drive card component (letter, label, FS, media badge, fill bar with three colour states, low-space reason, used/free bytes and %, health pill); Vitest for colour thresholds
- [ ] 5.2 Build the Home page: summary stat strip, drive card grid, low-space alert banner, "N drives not shown" note, refresh tied to the job/SSE. States: skeleton cards on first load, "Not scanned yet" card variant, error with retry
- [ ] 5.3 Build the drive detail panel listing physical disks and health fields with source and "Unavailable — reason". States: skeleton, error with retry
- [ ] 5.4 Manual check on this machine: C: shows NVMe, G: shows SATA SSD, D: shows HDD, E: and the virtual disk are not listed (the "not shown" count is correct), and reduced mode shows the admin-only fields as unavailable
