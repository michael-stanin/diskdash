## Context

Duplicate detection must read file contents, which is the first time the app does so. Reading has side effects. When last-access updates are enabled on a volume (see `stale-data-analysis`), reading a file updates its atime, which skews stale-data analysis. Reading a cloud placeholder triggers a download. A home directory can contain 1M+ files and hundreds of GB, and HDDs thrash under parallel random reads.

## Goals / Non-Goals

**Goals:** correct grouping (no false positives), minimal bytes read, bounded memory, HDD-friendly I/O, a reusable hash cache, and zero writes to user files.

**Non-Goals:** fuzzy or perceptual duplicates (similar photos), deleting or hard-linking duplicates (the app is strictly read-only; removal is manual via `cleanup-plans`), and scanning anything other than local fixed internal drives.

## Decisions

### D1. Three-stage funnel
1. Group by `size` across the selected snapshot DBs (attached read-only; `GROUP BY size HAVING count>1`, with size ≥ min_size). Within each size group, collapse entries that the scan marked `hardlink_of` the same file, or that share (volume serial, file id) where the snapshot recorded it.
2. Partial hash: BLAKE3 over the first 64 KiB plus the last 64 KiB (the whole file if it is ≤ 128 KiB).
3. Full hash: streamed BLAKE3 in 1 MiB chunks, only for files whose partial hashes collide.

**Hard links on volumes without link detection.** The scan checks hard links only on the system volume by default (per-drive toggle; the MFT scan always has exact link counts). On other volumes two names of one file look like two files. The hasher therefore reads `(volume serial, file id)` from `GetFileInformationByHandle` on the handle it already opened for stage 2. Before any partial hash is compared, entries with the same id are collapsed into one member that has several names. No extra open is needed, and the result is never a false duplicate.

Byte-for-byte compare is not done. BLAKE3-256 collisions are negligible.
- *Alternative: xxh3-128 (`xxhash-rust`).* It is faster per byte (CPU), but it is non-cryptographic and the disk is the bottleneck anyway. It stays as a config option (`hash_algo: xxh3`) if profiling shows BLAKE3 is the limit on NVMe.
- *Alternative: full hash for everything.* It lost because it reads orders of magnitude more bytes.

### D2. How files are opened
Via the `windows` crate: `CreateFileW(GENERIC_READ, FILE_SHARE_READ|FILE_SHARE_WRITE|FILE_SHARE_DELETE, OPEN_EXISTING, FILE_FLAG_SEQUENTIAL_SCAN | FILE_FLAG_OPEN_REPARSE_POINT)`. Before opening, attributes are re-checked from the snapshot *and* via `GetFileAttributesExW`. Any `RECALL_ON_DATA_ACCESS`, `RECALL_ON_OPEN`, `OFFLINE` or reparse attribute → skip. After open, `GetFileInformationByHandle` verifies that size and mtime still match the snapshot. If they do not, the file is marked "changed since scan" and skipped.

### D3. Last-access side effect (decision + open question)
Reading updates atime when last-access tracking is enabled. The options are:
- (a) Open with `FILE_WRITE_ATTRIBUTES` and call `SetFileTime(h, NULL, &{0xFFFFFFFF,0xFFFFFFFF}, NULL)` to suppress the atime update for this handle. **Rejected**: it requires write-attribute access to user files, which conflicts with safety rule 3 ("never open for write / change timestamps"), and it fails on read-only ACLs.
- (b) Accept the atime update, and make stale analysis independent of it. **Chosen**: stale analysis uses the snapshot's times captured *at scan time*, before any hashing, and a duplicate run never updates the snapshot's times. The UI notes that hashing may update last-access times on volumes where tracking is enabled. On most volumes over 128 GB it is disabled by default, so nothing changes.
- (c) Skip hashing on volumes with reliable atime. Rejected because it makes the feature useless on small SSDs.

Option (b) is the decision. The duplicates view shows a notice before hashing starts on atime-reliable volumes.

### D4. Scheduling
Work items are grouped by physical disk (from drive-inventory). HDD: one reader thread per spindle, items sorted by snapshot entry id (≈ directory order, an approximation of on-disk locality). SSD/NVMe: a pool of `min(4, cpu)` reader threads per disk. All of this runs on job-runner worker threads, never on the async request path. A panic in a reader marks only that item failed; the job continues. Progress goes over SSE as bytes hashed / bytes planned, and cancellation is checked between 1 MiB chunks.

### D5. Hash cache
`hash_cache(volume_serial, file_id, size, mtime, partial, full, algo)` lives in the catalog DB, because it outlives snapshots. A cache hit requires all four key fields and the algorithm to match. Entries not seen in the last K runs are pruned. Memory bound: one size bucket is processed at a time, and the largest bucket is streamed from SQLite; per-file state is about 100 bytes.

### D6. Keep rules → suggestions only
The keep rules are newest mtime, oldest mtime, shortest path, and preferred drive/folder prefix, with ties broken by shortest path then lowest entry id. Applying a rule writes `dup_suggestions(group_id, keep_member, remove_members)`. They are suggestions that `cleanup-plans` can turn into checklist items with manual instructions. No component of the app executes them.

### D7. Near-duplicate folders (optional, off by default)
For directories with ≥ 10 hashed files, build a set of (relative path, size, full hash). The candidate pairs are directories sharing at least one hash, found through an inverted index. The Jaccard index is computed and pairs at or above the threshold (default 0.9) are reported. Bound: candidate pairs are capped (default 10k), and the rest are reported as truncated.

## Risks / Trade-offs

- A long run on HDDs → it can be cancelled, it resumes via the cache, and it shows planned bytes up front so the user can narrow the scope.
- File changed between scan and hash → it is detected by the size/mtime re-check and skipped.
- Antivirus scanning on read slows things down → documented; nothing to change.

## Open Questions

None. The defaults are accepted: 1 MiB minimum size, near-duplicate folders off by default, and atime option (b).
