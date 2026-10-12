## ADDED Requirements

### Requirement: Scan targets
The system SHALL let the user start a scan of one or more roots, where each root is either a whole local volume or a folder path. A scan request SHALL be rejected with a validation error when a root does not exist, is not a directory, or does not lie on an in-scope fixed internal drive from the inventory. Scans SHALL only start on explicit user request; the system SHALL NOT start scans on a schedule or in the background. Overlapping roots in one request (one root inside another) SHALL be merged into the outer root.

#### Scenario: Scan two volumes
- **WHEN** the user starts a scan of `C:\` and `D:\`
- **THEN** one scan job is created that produces one snapshot per root

#### Scenario: Nested roots merged
- **WHEN** the user requests `D:\` and `D:\Media` in the same scan
- **THEN** only `D:\` is scanned, and the response notes that `D:\Media` was merged

#### Scenario: Out-of-scope root rejected
- **WHEN** the user requests `Z:\` (a mapped network drive) or `F:\` (a USB drive)
- **THEN** the request is rejected with "only fixed internal drives can be scanned", and nothing on that drive is accessed

### Requirement: Read-only traversal
The scanner MUST NOT create, modify, rename, move or delete any file or directory. It MUST NOT open any file for writing or for reading its contents. It MUST NOT change any timestamp or attribute. (The one effect it cannot avoid: NTFS may update a directory's last-access time when the directory is listed, as it does for any program; suppressing that would need write access. File access times are never affected, because file contents are never read.) It SHALL obtain metadata only through directory enumeration and metadata queries (`FindFirstFileExW`, `GetFileInformationByHandleEx` on a handle opened with `FILE_READ_ATTRIBUTES` only, `GetCompressedFileSizeW`). The only files the scanner writes are its own snapshot database files in the app data directory.

#### Scenario: Fixture unchanged after scan
- **WHEN** a fixture tree is scanned
- **THEN** a before/after comparison shows no change to any file's size, timestamps (including last access) or attributes, and no change to any directory other than its OS-maintained last-access time

#### Scenario: Cloud placeholder not hydrated
- **WHEN** the scan encounters a file with `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS` or `FILE_ATTRIBUTE_OFFLINE`
- **THEN** the file is recorded with its logical size and an allocated size of 0, flagged `cloud_placeholder`, and no handle with data-read access is opened on it

### Requirement: Reparse point handling
The scanner MUST NOT descend into directories that are junctions (`IO_REPARSE_TAG_MOUNT_POINT`) or symbolic links (`IO_REPARSE_TAG_SYMLINK`). These SHALL be recorded as link entries with size 0, their reparse tag, and their target path where readable. Symbolic-link files SHALL likewise be recorded with size 0 and not followed. Directories with other reparse tags that represent real local content (cloud files `IO_REPARSE_TAG_CLOUD_*`, deduplication `IO_REPARSE_TAG_DEDUP`, WCI/AppExecLink as files) SHALL be traversed or recorded normally, according to a documented tag table. Unknown reparse tags on directories SHALL NOT be traversed, and SHALL be logged once per tag.

#### Scenario: Junction not double counted
- **WHEN** a fixture contains `A\big.bin` (100 MiB) and a junction `B` pointing to `A`
- **THEN** the total for the fixture root is 100 MiB, and `B` appears as a link entry of size 0 with target `A`

#### Scenario: Junction loop
- **WHEN** a fixture contains a junction pointing to its own parent
- **THEN** the scan finishes, and the junction is recorded as a link entry without recursion

#### Scenario: OneDrive folder traversed
- **WHEN** a directory carries a cloud-files reparse tag
- **THEN** its children are enumerated and counted

### Requirement: Hard links counted once
The scanner SHALL identify files by (volume serial, 128-bit file id) whenever the link count is greater than 1. It SHALL count a hard-linked file's size toward directory totals and snapshot totals only at the first path where it is encountered. Each later path SHALL be recorded with `hardlink_of` pointing to the first entry, and SHALL contribute 0 to aggregates. Memory used for hard-link tracking SHALL be proportional to the number of multi-link files, not to the total number of files.

Hard-link detection SHALL be a per-volume setting. By default it SHALL be enabled on the system volume (the volume containing `%WINDIR%`) and disabled on all other volumes, because detection costs one metadata open per file. On volumes where it is disabled, every path SHALL be counted at full size. The snapshot SHALL be flagged `hardlinks_not_deduplicated`, and the UI SHALL show that flag as a small note on the drive's views.

#### Scenario: Default per-volume behaviour
- **WHEN** the user scans `C:\` (system volume) and `D:\` with default settings
- **THEN** the `C:` snapshot deduplicates hard links, and the `D:` snapshot is flagged `hardlinks_not_deduplicated`

#### Scenario: User enables detection on a data drive
- **WHEN** the user turns on hard-link detection for `D:` in Settings and rescans
- **THEN** the new `D:` snapshot deduplicates hard links and carries no flag

#### Scenario: Two links to one file
- **WHEN** a fixture has `x\data.bin` (50 MiB) hard-linked as `y\data.bin`
- **THEN** the root total is 50 MiB, one entry carries the size, and the other has `hardlink_of` set and contributes 0

#### Scenario: Link count unavailable
- **WHEN** metadata for a file cannot be read because of access denied, but enumeration succeeded
- **THEN** the file is counted with the size from enumeration, flagged `link_count_unknown`, and the snapshot statistics report how many such files exist

### Requirement: Long path support
The scanner SHALL handle paths of any length up to the NTFS limit (~32,767 characters). It SHALL do this by using `\\?\` extended-length paths for all filesystem calls, regardless of the system `LongPathsEnabled` setting. Stored and displayed paths SHALL NOT include the `\\?\` prefix.

#### Scenario: Deep path
- **WHEN** a fixture contains a file whose full path is 600 characters long
- **THEN** it is scanned, its size is included in the totals, and the API returns its full path without the prefix

### Requirement: Error tolerance
The scanner SHALL NOT fail the scan because of an error on an individual path. It SHALL record each such error (path, operation, Windows error code, message) in the snapshot's error log, and continue. Access-denied directories SHALL be recorded as entries flagged `access_denied`, with unknown contents. The snapshot statistics SHALL report the error count and the number of inaccessible directories. The scan SHALL fail only if the root itself is unreadable or the database becomes unwritable.

#### Scenario: Protected folder
- **WHEN** a non-elevated scan of `C:\` reaches `C:\System Volume Information`
- **THEN** the folder is recorded with `access_denied`, an error-log row is written, and the scan continues

#### Scenario: File deleted during scan
- **WHEN** a file listed by enumeration disappears before its metadata is read
- **THEN** an error-log row with `ERROR_FILE_NOT_FOUND` is written, the file is omitted, and the scan continues

#### Scenario: Root unreadable
- **WHEN** the scan root itself returns access denied
- **THEN** the job fails with a clear message, and no snapshot is marked complete

### Requirement: Logical and allocated size
For every file, the scanner SHALL record the logical size (end of file) and the allocated size, defined as follows:
- For compressed or sparse files, the value from `GetCompressedFileSizeW`, rounded up to a whole cluster.
- For cloud placeholders, 0.
- For all other files, the logical size rounded up to a whole cluster of the volume's cluster size.

Directory aggregates SHALL sum both values separately. The snapshot SHALL record the volume's cluster size. Allocated size SHALL be documented as an estimate: MFT-resident small files and alternate data streams are not modelled in directory-walk mode.

#### Scenario: Compressed file
- **WHEN** a 10 MiB fixture file of zeros is NTFS-compressed
- **THEN** its logical size is 10 MiB and its allocated size is strictly smaller

#### Scenario: Cluster slack
- **WHEN** a 1-byte file sits on a volume with 4 KiB clusters
- **THEN** its logical size is 1 and its allocated size is 4096

#### Scenario: Sparse file
- **WHEN** a fixture file is marked sparse with a 1 GiB logical size and 64 KiB of written data
- **THEN** its allocated size is approximately 64 KiB, not 1 GiB

### Requirement: File-type categorisation
The scanner SHALL assign each file a lowercase extension (the text after the last dot, or empty) and a category from a fixed table maintained in code: `video`, `images`, `audio`, `archives`, `installers` (ISOs and installers), `documents`, `code` (code/dev artifacts), `disk-images` (VM/disk images), `system`, or `other`. Files under the Windows directory or the System Volume Information folder, and the system files (pagefile, hiberfil, swapfile), SHALL be categorised `system` regardless of extension.

#### Scenario: Known extensions
- **WHEN** files `a.MKV`, `b.vhdx`, `c.msi` and `d.py` are scanned
- **THEN** their categories are `video`, `disk-images`, `installers` and `code`, and their extensions are `mkv`, `vhdx`, `msi` and `py`

#### Scenario: Unknown extension
- **WHEN** a file `e.xyz123` is scanned
- **THEN** its category is `other`

### Requirement: Scheduling by physical disk
The scanner SHALL group roots by the physical disk they reside on, using the drive inventory. It SHALL run at most `hdd_concurrency` traversal workers per HDD (default 1) and at most `ssd_concurrency` per SSD (default 4). Different physical disks SHALL be scanned concurrently. Roots whose disk is unknown SHALL be treated as HDD.

#### Scenario: Two folders on the same HDD
- **WHEN** the user scans `D:\Media` and `D:\Backup`, both on the HDD
- **THEN** they are traversed one after the other, not in parallel

#### Scenario: SSD and HDD together
- **WHEN** the user scans `C:\` (NVMe) and `D:\` (HDD)
- **THEN** both scans progress at the same time

### Requirement: Progress and ETA
A running scan SHALL report, at most 4 times per second: entries scanned, bytes counted (logical), directories pending, current directory, errors so far, and elapsed time. For whole-volume roots it SHALL report an ETA based on bytes counted versus the volume's used bytes from the inventory. For folder roots it SHALL use the previous snapshot's total for the same root if one exists, or report no ETA.

#### Scenario: Whole-volume ETA
- **WHEN** a scan of `D:\` has counted 300 GiB of a volume with 600 GiB used, after 2 minutes
- **THEN** progress reports approximately 50%, with an ETA of about 2 minutes

#### Scenario: Folder with no history
- **WHEN** a folder is scanned for the first time
- **THEN** progress shows counts and no ETA

### Requirement: Cancel and resume
The user SHALL be able to cancel a scan. A cancelled or interrupted scan SHALL keep its completed work units: the files directly in the root form one unit, and each subdirectory of the root forms another. Resuming SHALL re-scan only the subtrees that did not complete, and then recompute the root aggregates. A snapshot SHALL be marked `complete` only after all subtrees finish. A resumed snapshot SHALL record that it was resumed, along with the time span it covers.

#### Scenario: Cancel
- **WHEN** the user cancels a running scan
- **THEN** the job stops within 2 seconds, and the snapshot is marked `partial` with its completed subtrees listed

#### Scenario: Resume after interruption
- **WHEN** the app restarts during a scan of `C:\` in which `C:\Users` completed and `C:\Windows` did not
- **THEN** resuming re-scans `C:\Windows` and the subtrees after it, without re-scanning `C:\Users`, and the final snapshot is `complete`

#### Scenario: Resume not possible
- **WHEN** the volume serial number differs from the one recorded at scan start, because the volume was reformatted or replaced
- **THEN** resume is refused, and the user is offered a fresh scan

### Requirement: Bounded memory and responsive UI
The scanner SHALL use memory bounded by tree depth plus batch size plus hard-link tracking, not by total entry count. It SHALL write entries to the database in batched transactions of between 5,000 and 50,000 rows. API requests made during a scan SHALL continue to respond.

#### Scenario: Large synthetic tree
- **WHEN** a generated tree of 1,000,000 empty files is scanned (marked slow test, opt-in)
- **THEN** the scan worker's peak working set stays under 500 MiB, and `GET /api/v1/health` responds in under 200 ms throughout
