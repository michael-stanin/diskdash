## Why

Knowing that a drive is full doesn't help until you know *what* fills it. Every later analysis (visualisation, cleanup rules, stale data, duplicates, tiering, history) reads from one source: a correct, complete snapshot of the file tree. If that snapshot double-counts junctions or hard links, or dies on the first access-denied folder, every number built on it is wrong. So this phase covers correctness as much as speed.

## What Changes

- Add a directory-walk scanner (no admin needed) in the `diskdash-core` library crate. It runs as a manually started background job over one or more roots: whole volumes, several volumes, or a single folder, on fixed internal drives only.
- Apply the correctness rules:
  - never descend into junctions or symlinks (record them as zero-size link entries);
  - descend into cloud-file and dedup reparse points without reading them;
  - count hard-linked files once per snapshot, on the system volume by default (per-drive toggle);
  - use extended-length paths so paths over 260 characters work;
  - skip and record access-denied and other per-path errors;
  - open nothing for writing and never read file contents.
- Record, per entry: name, parent, type, logical size, allocated size ("size on disk", accounting for compression, sparse files, cluster slack and cloud placeholders), timestamps (created/modified/accessed), attributes, reparse tag, link count, file id, extension, and file-type category.
- Aggregate recursive directory totals (logical, allocated, file count, directory count) in a single post-order pass, with bounded memory.
- Schedule scans per physical disk: one walker per HDD spindle, and configurable parallelism on SSDs (uses `add-drive-inventory`).
- Report progress (entries, bytes, current path, ETA), allow cancel, and allow resume of interrupted volume scans at top-level-subtree granularity.
- Store snapshots in SQLite, written in batches, with per-snapshot statistics and an error log.
- Add query APIs for snapshots, children of a directory, top-N largest folders and files, and breakdowns by category and extension. `add-usage-visualization` builds the UI on these.
- UI (minimal in this phase): a "Scan" dialog (pick drives and folders), a scan progress panel with cancel, a snapshot list, and a plain top-N table to verify the data. Charts come in phase 4.

Safety rules touched: **3**. A scan opens no file for writing, reads no file contents, and does not open cloud placeholders. **6**: tests use generated fixture trees only.

## Capabilities

### New Capabilities
- `filesystem-scan`: The scanning process: root selection, traversal rules (reparse points, hard links, long paths, errors), size-on-disk computation, file-type categorisation, scheduling per physical disk, progress, cancel and resume.
- `scan-snapshots`: Persisted scan results and the query surface over them: snapshot lifecycle, entries, directory aggregates, top-N, category/extension breakdowns, errors, and snapshot deletion.

### Modified Capabilities
None.

## Impact

- `crates/diskdash-core/src/scan/`: Win32 enumeration via the `windows` crate, reparse table, size-on-disk, categoriser, aggregator, rusqlite writer.
- `crates/diskdash-store`: snapshot queries.
- `crates/diskdash`: scan job, scheduler, resume, and routes under `/api/v1/scans` and `/api/v1/snapshots`.
- Migration `0003_snapshots.sql`.
- Test fixtures: a generator that builds trees with junctions, symlinks (when the test process has the privilege or Developer Mode is on, otherwise skipped with a reason), hard links, deep paths over 260 characters, ACL-denied folders, sparse and NTFS-compressed files, all under a temp dir created by the `test-fixtures` crate.
- DB size: about 120–200 bytes per entry, so roughly 150–250 MB per snapshot for 1 M entries. Retention arrives in `add-history-and-trends`; until then, snapshots can be deleted manually.
- New Rust dependencies: `crossbeam-channel` and `tempfile` (dev). `windows` and `rusqlite` are already present.

## Depends on

`add-project-skeleton` (jobs, locks, persistence) and `add-drive-inventory` (volume ids, media class, cluster size).
