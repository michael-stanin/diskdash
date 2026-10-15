# stale-data-analysis Specification

## Purpose
TBD - created by archiving change add-stale-data-analysis. Update Purpose after archive.
## Requirements
### Requirement: Last-access reliability detection
The system SHALL determine, for each scanned volume, whether last-access timestamps are reliable by running `fsutil behavior query disablelastaccess`. It SHALL interpret values 0 and 3 (user-managed enabled, system-managed enabled) as reliable, and values 1 and 2 (user-managed disabled, system-managed disabled) as unreliable. For system-managed enabled, reliability SHALL be assumed only for volumes of 128 GB or less. The decision SHALL be stored with each snapshot as its time basis (`atime` or `mtime`).

#### Scenario: System-managed default on a large volume
- **WHEN** fsutil reports `0x80000002` or reports system-managed with the volume larger than 128 GB
- **THEN** the snapshot's time basis is `mtime` and the stale view shows a warning that last-access times are not updated on this volume

#### Scenario: fsutil unavailable or output unparseable
- **WHEN** the fsutil call fails or its output cannot be parsed
- **THEN** the time basis falls back to `mtime`, the reason is logged, and the UI states "last-access reliability unknown"

### Requirement: Stale data query with age presets
The system SHALL list files and folders whose time basis (last-access when reliable, otherwise last-modified) is older than a chosen age, offering presets of 6, 12, 24 and 36 months and a custom value. The default SHALL be 12 months. Results SHALL be ranked by allocated size. A folder SHALL be stale only if its newest descendant is older than the cutoff. Nested stale folders SHALL be collapsed under their highest stale ancestor.

#### Scenario: Folder with one recent file
- **WHEN** a folder contains 10 GiB of files untouched for 3 years and one file modified last week
- **THEN** the folder is not listed as stale, but its stale subfolders and files are

#### Scenario: Switch preset
- **WHEN** the user changes the preset from 12 to 24 months
- **THEN** the list is re-queried from snapshot data without rescanning the disk

### Requirement: Old large files scoring
The system SHALL compute a size × age score (allocated size × age in days) for files above a configurable size floor (default 100 MiB) and SHALL provide an "old large files" list sorted by that score.

#### Scenario: Score ordering
- **WHEN** file A is 1 GiB and 400 days old, and file B is 4 GiB and 30 days old
- **THEN** A is ranked above B (score about 400 vs. about 120 GiB·days)

### Requirement: Read-only analysis
Stale-data analysis SHALL be computed solely from snapshot data and the fsutil query. It MUST NOT open, read, or change timestamps of any user file. The system MUST NOT change the `disablelastaccess` setting. It SHALL show the manual command (`fsutil behavior set disablelastaccess 0`, run by the user in an elevated terminal) as copyable text when last-access times are unreliable.

#### Scenario: No filesystem access during analysis
- **WHEN** the stale-data query runs on a snapshot
- **THEN** no user file is opened (verified in tests by asserting the analysis code path performs no file-open calls)

### Requirement: Snapshot without time data
The system SHALL handle snapshots whose entries lack a usable timestamp (for example zero or pre-1980 times on FAT volumes) by excluding them from staleness results and showing the excluded count.

#### Scenario: FAT32/exFAT internal volume with invalid timestamps
- **WHEN** some entries on an internal FAT32 or exFAT volume have no valid last-modified time
- **THEN** they are excluded from the stale list and the view shows "N items without valid timestamps excluded"

### Requirement: Stale view design and states
The Stale tab SHALL use the shared design-system components and the shared category colour map. It SHALL show the active time basis (last-access or last-modified) prominently, a skeleton while loading, a designed empty state when nothing is older than the cutoff, and an error state with retry.

#### Scenario: Nothing stale
- **WHEN** no file or folder on the drive is older than the selected preset
- **THEN** the tab shows "Nothing older than 12 months on this drive" with a control to pick a shorter preset

#### Scenario: Snapshot missing
- **WHEN** the drive has no completed snapshot
- **THEN** the tab shows an empty state with "Scan now"

