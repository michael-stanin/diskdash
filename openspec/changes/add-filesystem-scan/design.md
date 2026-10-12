## Context

This scanner is the data source for every later phase. It must be correct first: no double counting through junctions or hard links, no crash on denied folders, and no touching of file contents, including cloud placeholders that would download if opened. It must also be usable on more than 1 M files without admin rights. The fast MFT path comes later (`add-fast-ntfs-scan`). This design must produce the same results that path will, so the two can be cross-checked.

## Goals / Non-Goals

**Goals:**
- Exact totals on fixture trees, covering junctions, symlinks, hard links, long paths, denied ACLs, sparse and compressed files.
- Memory bounded by depth, not by entry count. The UI stays responsive.
- Snapshots that are fast to query (children, top-N, subtree breakdowns) and cheap to delete.
- Sequential I/O per HDD spindle.

**Non-Goals:**
- MFT/USN reading (phase 11).
- Alternate data streams. ADS sizes are not counted in walk mode, and the omission is documented.
- Incremental rescans in this phase (see D10).
- Any UI beyond a scan dialog, progress panel and a verification table.

## Decisions

### D1. Scanner as a library crate, driven by the job runner
The walker, the size/category logic and the snapshot writer live in `crates/diskdash-core`, which has no HTTP dependency. Everything is one binary, so nothing crosses a language or process boundary.
- The `scan` job (in `crates/diskdash`) calls `diskdash_core::scan(roots, options, snapshot_db_path) -> ScanHandle`.
- The job polls `handle.progress()` (atomics) at 4 Hz for job events, and maps job cancellation to `handle.cancel()`.
- The writer thread inserts rows directly into the per-snapshot SQLite file with rusqlite.
- `FindFirstFileExW` (`FindExInfoBasic`, `FIND_FIRST_EX_LARGE_FETCH`) returns size, timestamps, attributes and reparse tag per entry. Only hard-link detection needs a per-file metadata open (D3).
- Phase 11 (MFT/USN) reuses the same crate, the same tables and the same category and reparse logic, which makes the "walk and MFT give identical totals" test meaningful.
- *Alternative: running the scan in a separate process.* That gives isolation, but panics are already caught at the job boundary, and a process adds IPC for nothing. That model is reserved for the elevated helper in phase 11, which needs a different token.
- *Alternative: shelling out to `dir /s` or Everything SDK.* Fragile parsing, missing fields, or a third-party service.

### D2. Traversal: iterative DFS with post-order aggregation
An explicit stack of frames (directory entry id, accumulators, open `FindFirstFileExW` handle) replaces recursion, which avoids recursion limits on deep trees. When a directory's iterator is exhausted, its totals are final. The directory row is emitted with totals, and its accumulators are added to the parent frame. Memory is O(depth × frame) plus the write batch plus the hard-link map. Each frame also carries the max mtime/atime seen below it, which phase 6 needs. File rows are emitted as they are seen. Directory rows are emitted when they close.

### D3. Hard links: file-id map only for multi-link files
For each regular file, the scanner opens a metadata-only handle (`FILE_READ_ATTRIBUTES`, full sharing, `FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT`) and calls `GetFileInformationByHandleEx(FileStandardInfo / FileIdInfo)` to get the link count and 128-bit file id. Only files with `NumberOfLinks > 1` enter the `(volume_serial, file_id) → first_entry_id` map. These are rare outside `C:\Windows\WinSxS` and package stores such as pnpm, so the map stays small.
- *Trade-off:* one extra open per file is the main cost of walk mode. On HDD it can cause MFT-record seeks.
- **Owner decision: detection per volume, on by default only for the system volume** (the one holding `%WINDIR%`, where WinSxS lives). There, hard links are common and the drive is the fast NVMe. On the data drives (G: SSD, D: HDD), hard links are practically absent, so detection is off by default. Those snapshots are flagged `hardlinks_not_deduplicated`, and a per-drive toggle in Settings turns detection on. Phase 11's MFT scan gets exact link counts at no extra cost.
- *Alternative: `FindFirstFileNameW` per file.* Same cost, with more calls.
- *Alternative: `NtQueryDirectoryFile(FileIdExtdDirectoryInformation)`.* Gives file ids in bulk, but no link count, so we would still need the per-file call.

### D4. Reparse tag table
A small, unit-tested table decides **traverse / record-as-link / record-as-file**:

| Tag | Directory | File |
|---|---|---|
| MOUNT_POINT (junction) | link, don't traverse | n/a |
| SYMLINK | link, don't traverse | link, size 0 |
| CLOUD, CLOUD_1..F (OneDrive) | traverse | file; placeholder flags decide allocated size |
| DEDUP | traverse | file (allocated = `GetCompressedFileSizeW`) |
| APPEXECLINK, WCI, LX_SYMLINK, AF_UNIX | link | link, size 0 |
| unknown | link, log once | file, log once |

### D5. Snapshot storage: one SQLite file per snapshot, plus a catalog in the main DB
The main DB holds `snapshots` (metadata, stats, status, resume state). Each snapshot's `entries`, `errors` and breakdown tables live in `%LOCALAPPDATA%\diskdash\snapshots\<id>.db`, which is ATTACHed on demand.
- Deleting a snapshot is a file delete: instant, and space is returned without VACUUMing a 1 GB main DB.
- Cross-snapshot diffs (phase 10) ATTACH two files.
- Writes during a scan never contend with the main DB.
- *Alternative: one big `entries` table keyed by snapshot id.* `DELETE` of 1 M rows is slow, the file never shrinks without VACUUM, and indexes grow across all snapshots.
- *Risk:* the catalog and the files can drift. **Mitigation:** reconcile at startup. Orphan files are deleted, and catalog rows without files are marked `failed`.

### D6. Entry ids as DFS pre-order sequence → subtree = id range
Entry ids are assigned in DFS pre-order, and each directory stores `last_desc_id`. Every subtree is then the contiguous range `[dir.id, dir.last_desc_id]`, so subtree top-N and breakdowns are `WHERE id BETWEEN ? AND ?` against an index. No path LIKE scans and no recursive CTEs.
- For parallel SSD scans, each top-level subtree gets an id block: `id = (block << 40) | local_seq`. Within a block, DFS order holds. The root covers all blocks.
- Indexes: `(parent_id, alloc DESC)` for children; `(kind, alloc DESC)` for top files; `(kind, alloc DESC) WHERE kind='dir'` for top dirs; `(category)` and `(ext)` for breakdowns, which are precomputed at the root at scan end.
- `name_lower` is stored and indexed for case-insensitive rule matching (phase 5).
- Columns are fixed-width integers where possible: times as int64 FILETIME seconds, attributes as int. Names are TEXT. Full paths are stored for directories only.
- Estimated ~120–200 B per entry including indexes.

### D7. Allocated size computation
- Compressed or sparse files (attribute bits): `GetCompressedFileSizeW` rounded up to a cluster.
- Cloud placeholders (`RECALL_ON_DATA_ACCESS`, `RECALL_ON_OPEN` or `OFFLINE`): 0.
- Everything else: `ceil(size / cluster) × cluster`.
- Known inaccuracy: MFT-resident files (≲ 700 B) actually use 0 clusters. We over-report them by up to one cluster each. This is documented and acceptable. The MFT path gets it exact.
- Calling `GetCompressedFileSizeW` on every file would add another per-file call for no benefit, so we don't.

### D8. Concurrency model
- One scan job occupies one job-runner slot and spawns its own traversal threads.
- There is one traversal thread per HDD root (sequential, with HDD roots on the same disk queued), and up to `ssd_concurrency` threads per SSD that pull top-level subtrees from a work queue.
- A single writer thread owns the snapshot DB connection and receives batches through a bounded channel. The bounded channel provides backpressure, so the walkers can't outrun the disk writes.
- If two threads meet the same multi-link file, the first one to claim it in the hard-link map (under a lock) owns its size. Totals are deterministic. Which path is the owner is not, and that is documented.

### D9. Cancel and resume
- Resume state is stored in `snapshots.resume_state`: an ordered list of top-level work units (children of the root, and large depth-1 children expanded one level), each with status `pending | done` and its id block.
- On resume, the rows of incomplete units are deleted by id-block range, those units are re-run, and the root aggregates are recomputed from the unit summaries.
- Resume is refused if the volume serial changed.
- Cancel sets the job's cancel flag. Walkers check it at every directory boundary and at least every 1,000 entries.

### D10. Manual full rescans only; no incremental rescans here
Without admin rights, detecting "what changed" still requires a full enumeration. Directory mtimes change when entries are added or removed, but not when a file inside grows, so an mtime-only shortcut would miss growth. Scans are always started by the user. There is no schedule and no watchdog (a watchdog toggle is a possible future item, not planned). A manual USN-based "quick rescan" comes with `add-fast-ntfs-scan`. **Owner approved.**

### D11. Test fixtures
Pure logic (tag table, allocated size, categories, id blocks) has unit tests. End-to-end behaviour is tested in `cargo test` integration tests. The `test-fixtures` crate builds trees under a `tempfile::TempDir` from a declarative spec and returns the expected totals:
- Junctions: `mklink /J` works without admin.
- Hard links: `std::fs::hard_link`.
- Symlinks: need Developer Mode or admin, otherwise the test is skipped with that reason.
- Long paths: `\\?\`.
- Denied ACL: `icacls /deny <me>:(RX)`, always restored in teardown.
- Sparse: `FSCTL_SET_SPARSE` on our own file via the `windows` crate, no admin.
- Compressed: `compact /C`.

A before/after manifest (size, mtime, ctime, attrs, sha256) proves the scan didn't modify anything. Cloud placeholders cannot be built locally without OneDrive. That path is unit-tested with mocked attributes.

## Risks / Trade-offs

- [Per-file metadata open makes scans slow] → Only on the system volume by default (owner decision), with a per-drive toggle and phase 11 MFT.
- [Data drives over-count if they do contain hard links (e.g. a pnpm store or backup tool on D:)] → The flag is visible on those snapshots, and the toggle fixes it on the next scan.
- [Antivirus scanning on each file open slows the scan, though with metadata-only access most AV filters skip content scans] → Measure in the benchmark. Document excluding the scanner if needed.
- [Snapshot DB files take a lot of space (≈200 MB per 1 M entries)] → Show the storage used by snapshots in Settings. Retention comes in phase 10, and manual delete is available now.
- [Scanning the volume that holds `%LOCALAPPDATA%\diskdash` counts our own growing snapshot file] → Exclude our data dir from the scan, and say so in the snapshot stats.
- [Files changing during the scan cause inconsistent totals] → Accepted. A snapshot is "as observed between start and end", and the UI shows the scan's time span.
- [The `\\?\` prefix disables path normalisation, so `..` and `/` are not handled] → Normalise roots once (`std::path::absolute`) before prefixing. Never prefix user-supplied relative paths.

## Open Questions

1. Should `C:\` scans include the `C:\Windows` subtree by default? It is useful for "system" totals, but it is ~200 k entries that can't be cleaned anyway. The proposed default is to include it, because category breakdowns need it.
Resolved: hard-link detection on the system volume only, by default (D3).
