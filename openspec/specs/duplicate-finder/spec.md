# duplicate-finder Specification

## Purpose
TBD - created by archiving change add-duplicate-finder. Update Purpose after archive.
## Requirements
### Requirement: Staged duplicate detection
The system SHALL detect duplicate files in a chosen scope (snapshot, drives or folders) by grouping candidates by size, then by a partial hash of the first and last 64 KiB, then by a full-content BLAKE3 hash. Only files whose full hashes match SHALL be reported as duplicates. The hash algorithm SHALL be recorded with each result.

#### Scenario: Same size, different content
- **WHEN** two 10 MiB files have identical first and last 64 KiB but differ in the middle
- **THEN** they are not reported as duplicates

#### Scenario: True duplicates
- **WHEN** three files on C: and D: have identical content of 50 MiB
- **THEN** they form one group with wasted space equal to 2 × the allocated size

### Requirement: Safe candidate exclusions
The system SHALL exclude from hashing: files with cloud-placeholder attributes (RECALL_ON_DATA_ACCESS, RECALL_ON_OPEN, OFFLINE), reparse points, zero-byte files, and files smaller than a configurable minimum (default 1 MiB). Entries that the scan recorded as hard links of one another (`hardlink_of`), or that share the same volume serial number and file id, SHALL be treated as one file and MUST NOT be reported as duplicates of each other. On volumes scanned without hard-link detection, the system SHALL read the file id from the handle opened for hashing and collapse same-id entries before comparing hashes. Cloud placeholders MUST NOT be opened.

#### Scenario: Hard-linked siblings on the system volume
- **WHEN** `C:\a.bin` and `C:\b.bin` are recorded by the scan with `hardlink_of` pointing to the same file
- **THEN** they are not reported as a duplicate group and no wasted space is attributed

#### Scenario: Hard links on a volume without link detection
- **WHEN** `D:\x.bin` and `D:\y.bin` are hard links, but D: was scanned with hard-link detection off
- **THEN** the hasher sees the same file id on both handles and collapses them, and no duplicate group is reported

#### Scenario: OneDrive online-only file
- **WHEN** a candidate file has the RECALL_ON_DATA_ACCESS attribute
- **THEN** it is skipped without being opened and counted under "skipped: cloud placeholder"

### Requirement: Read-only file access
The hasher SHALL open files with read access only, sharing read/write/delete, and MUST NOT request write or write-attributes access or modify any file or timestamp explicitly. Before hashing, the system SHALL verify that size and modification time match the snapshot, and SHALL skip changed files. Files that cannot be opened (access denied, locked, sharing violation) SHALL be recorded per path and skipped without failing the job.

#### Scenario: Locked file
- **WHEN** a candidate file is opened by another process without read sharing
- **THEN** it is skipped with error "sharing violation", and the rest of the job continues

#### Scenario: File changed since scan
- **WHEN** a candidate's current size or mtime differs from the snapshot
- **THEN** it is skipped as "changed since scan" and not hashed

### Requirement: Drive-aware I/O scheduling
The system SHALL hash files on the same HDD physical disk sequentially (one reader per spindle), ordered by snapshot entry order. It MAY hash files on SSD/NVMe disks with a bounded number of parallel readers. The job SHALL report progress as bytes hashed vs. bytes planned over SSE and SHALL be cancellable.

#### Scenario: Two HDD volumes on one disk
- **WHEN** candidates span D: and E:, which are partitions of the same HDD
- **THEN** at most one file on that disk is read at a time

#### Scenario: Cancel
- **WHEN** the user cancels a running duplicate job
- **THEN** reading stops within one chunk, completed hashes remain in the cache, and the job status is "cancelled"

### Requirement: Hash cache
The system SHALL cache partial and full hashes keyed by (volume serial number, file id, size, last-modified time, algorithm), and SHALL reuse a cached hash only if all key fields match.

#### Scenario: Rescan after no changes
- **WHEN** a duplicate job is rerun on an unchanged scope
- **THEN** no file contents are read and the groups are reproduced from the cache

### Requirement: Duplicate groups and wasted space
The system SHALL present duplicate groups with member paths, sizes, modification times and drive. Each group's wasted space SHALL be computed as (members − 1) × allocated size. Groups SHALL be sortable by wasted space and filterable by drive, folder and category.

#### Scenario: Sort by wasted space
- **WHEN** the user opens the duplicates view
- **THEN** groups are listed with the largest wasted space first, along with a total across all groups

### Requirement: Keep-rule suggestions without action
The system SHALL let the user select a keep rule per group or globally: newest, oldest, shortest path, or preferred drive/folder. Ties SHALL be broken deterministically. Applying a rule SHALL store a suggested keep/remove set that the user can add to a cleanup plan. The system MUST NOT delete, move, link or otherwise modify any file.

#### Scenario: Preferred drive
- **WHEN** the user applies "prefer drive D:" to a group with copies on C: and D:
- **THEN** the D: copy is marked keep, the C: copy is marked suggested-remove with manual instructions, and no file is changed

### Requirement: Near-duplicate folders (optional)
When enabled, the system SHALL report pairs of folders whose sets of (relative path, size, full hash) have a Jaccard similarity at or above a configurable threshold (default 0.9), with the shared and differing bytes. Candidate pairs SHALL be capped, and truncation SHALL be reported.

#### Scenario: Copied project folder
- **WHEN** `D:\proj` and `E:\backup\proj` share 95% of their (path, size, hash) entries
- **THEN** they are reported as near-duplicate folders with similarity 0.95

### Requirement: Duplicates view design and states
The Duplicates tab and the cross-drive Duplicates page SHALL use the shared design-system components and the shared category colour map. They SHALL show 20 groups by default with "Show more", the job's progress (bytes hashed of planned) with a cancel control while running, a skeleton while loading results, a designed empty state, and an error state with retry.

#### Scenario: No duplicates found
- **WHEN** a completed run finds no duplicate groups
- **THEN** the view shows "No duplicates over 1 MiB found" with the scope and the date of the run

#### Scenario: Never run
- **WHEN** no duplicate analysis has been run for the drive
- **THEN** the view shows an empty state that explains what will be read and offers "Find duplicates"

