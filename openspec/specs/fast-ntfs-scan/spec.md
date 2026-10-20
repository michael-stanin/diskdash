# fast-ntfs-scan Specification

## Purpose
TBD - created by archiving change add-fast-ntfs-scan. Update Purpose after archive.
## Requirements
### Requirement: MFT-based full scan when elevated
When the read-only elevated helper is running and the target is a whole local fixed internal NTFS volume, the system SHALL scan it by reading the Master File Table with the `diskdash-core` `ntfs` module, executed inside the helper, which streams entry batches to the server. It SHALL produce snapshot entries with name, parent, logical size, allocated size, timestamps, attributes and category in the same model as the directory walk. The volume handle MUST be opened with read access only (GENERIC_READ, sharing read and write), and the system MUST NOT write to the volume.

#### Scenario: Elevated scan of an NTFS drive
- **WHEN** the helper is running and the user scans `C:`
- **THEN** the scan runs in MFT mode, the dialog shows "fast scan (MFT)", and a snapshot is stored like any other scan

#### Scenario: Corrupt record
- **WHEN** an MFT record fails fixup or bounds validation
- **THEN** the record is skipped and logged as an error, and the scan continues

### Requirement: Automatic fallback to directory walk
The system SHALL fall back to the directory-walk scanner, showing the reason, when the helper is not running, the volume is not NTFS (for example internal FAT32, exFAT or ReFS), the target is a folder rather than a whole volume, the volume handle cannot be opened, or the record error rate exceeds 1%.

#### Scenario: Non-admin user without helper
- **WHEN** the helper has not been started (reduced mode)
- **THEN** the directory walk is used and the dialog shows "fast scan requires administrator rights"

#### Scenario: Internal exFAT volume
- **WHEN** a scan targets an internal exFAT volume while the helper is running
- **THEN** the directory walk is used with the reason "MFT scan supports NTFS only"

### Requirement: Same counting rules as the directory walk
MFT scans SHALL apply the same rules as filesystem-scan: hard links counted once (always detected exactly from MFT link data, regardless of the walk's per-drive hard-link setting), reparse points not followed, logical and allocated sizes computed the same way (including compressed and sparse files), and 8.3 short names ignored. NTFS metadata files SHALL be reported under a separate synthetic node.

#### Scenario: Hard links in MFT
- **WHEN** a file has two `$FILE_NAME` attributes in different directories
- **THEN** its size is counted once in the volume total

### Requirement: Equivalence with directory walk
On the same fixture volume, totals from an MFT scan (logical size, allocated size, file count and directory count, excluding the NTFS metadata node) SHALL equal those of a directory walk. This SHALL be verified by an admin-only, opt-in test on a VHDX that the test creates and removes itself.

#### Scenario: Fixture VHDX equivalence
- **WHEN** `cargo test -- --ignored` is run elevated with `DISKDASH_ADMIN_TESTS=1`
- **THEN** both scanners run on the test-created VHDX and their totals are equal

#### Scenario: Admin tests not requested
- **WHEN** the normal test suite runs without elevation
- **THEN** the equivalence test is skipped (ignored) with a clear reason, and the unit tests on captured record bytes still run

### Requirement: Manual USN quick rescan
After an MFT scan, the system SHALL store the volume's USN journal id and next USN. When the user clicks "Quick rescan", the system SHALL read the change journal from that USN, re-read only the changed records, and produce a new snapshot. If the journal id changed, the journal wrapped past the stored USN, the journal is inactive, or changes exceed 20% of records, the system SHALL perform a full MFT scan instead. Quick rescans SHALL run only on user request. The system MUST NOT create, resize or delete the USN journal.

#### Scenario: Quick rescan
- **WHEN** 500 files changed since the last MFT scan and the user clicks Quick rescan
- **THEN** only the journal and those 500 records are read, and the new snapshot's totals match a full scan

#### Scenario: Journal wrapped
- **WHEN** the stored USN is older than the journal's first USN
- **THEN** the system logs "USN journal wrapped" and performs a full MFT scan

#### Scenario: No prior MFT scan
- **WHEN** the volume has no stored USN state
- **THEN** the Quick rescan button performs a full scan and says so

### Requirement: Strictly read-only elevated helper
Privileged operations SHALL be performed by the separate `diskdash-helper` executable that the user explicitly starts with elevation. The web server MUST NOT run elevated. The helper SHALL communicate only over a named pipe restricted to the current user's SID, and SHALL authenticate clients with a per-launch secret. It SHALL expose only a closed set of read-only operations (volume info, MFT scan, USN read, SMART reliability counters, Prefetch last-run times) that take volume GUIDs or disk numbers from its own enumeration of fixed internal drives. It MUST NOT write any file (including snapshots, which the server writes), and MUST NOT expose path-based file access, process launching, or command execution.

#### Scenario: Unauthenticated pipe client
- **WHEN** a process connects to the helper pipe without the correct secret
- **THEN** the helper closes the connection and logs the attempt

#### Scenario: Unknown operation
- **WHEN** an authenticated client sends an operation outside the allowed set
- **THEN** the helper rejects it with an error and performs nothing

#### Scenario: UAC declined
- **WHEN** the user declines the UAC prompt for the helper
- **THEN** the app stays in reduced mode and scans use the directory walk

### Requirement: Bounded memory on large volumes
The MFT scanner SHALL stream records to storage in batches and keep only a compact FRN-to-parent map in memory. Peak memory for a 5M-record volume SHALL stay at or below 500 MB.

#### Scenario: Large volume
- **WHEN** a volume with 5M MFT records is scanned
- **THEN** the scan completes with peak memory at or below 500 MB and the UI stays responsive

#### Scenario: Hard links on a non-system volume
- **WHEN** D: (with hard-link detection off for the walk) is scanned via MFT and contains two names of one file
- **THEN** the file is counted once and the second name is recorded with `hardlink_of`

### Requirement: Scan-mode UI states
The scan dialog and drive view SHALL show the scan mode ("fast scan (MFT)" or "directory walk") and, when falling back, the reason, using the shared design-system components. The "Enable elevated features" and "Quick rescan" controls SHALL show progress while running, and SHALL show an error state with the reason when they fail.

#### Scenario: Helper launch fails
- **WHEN** the helper exits immediately after launch
- **THEN** the UI shows an error banner with the reason from the log and stays in reduced mode

#### Scenario: Quick rescan in progress
- **WHEN** the user clicks Quick rescan
- **THEN** the button shows progress over SSE and the drive view keeps showing the previous snapshot until the new one completes

