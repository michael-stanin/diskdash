## Why

Duplicate files (photo imports, repeated downloads, copied project folders, backups of backups) are a common, low-risk source of reclaimable space. The user wants duplicate groups with the wasted space shown, plus a rule for choosing which copy to keep. The app only suggests; the user removes copies by hand.

## What Changes

- A staged duplicate detection job: **size** (from the snapshot) → **partial hash** (first and last 64 KiB) → **full hash** (BLAKE3 via the `blake3` crate; xxh3-128 via `xxhash-rust` is the documented alternative).
- Exclusions: cloud placeholder files (never opened), zero-byte files, reparse points, files below a minimum size (default 1 MiB), and hard-linked siblings. Two names with the same volume serial and file id are the *same* file, not duplicates. Where the scan recorded hard links (`hardlink_of`; by default only on the system volume, per-drive toggle, and always exact under the phase-11 MFT scan), they are collapsed before hashing. On volumes scanned without hard-link detection, the file id is read from the open handle during hashing, and same-id entries are collapsed then.
- **I/O scheduling**: files on the same HDD spindle are read sequentially, in on-disk/path order. SSD volumes can be hashed in parallel with a bounded worker count. The job can be cancelled.
- A **hash cache** keyed by (volume serial, file id, size, mtime), so rescans only hash new or changed files.
- Duplicate groups with **wasted space** = (n − 1) × allocated size, filterable by drive, folder and category.
- **Keep-rule selection** (newest, oldest, shortest path, preferred drive) produces *suggestions* (keep this copy, remove the others by hand). Suggestions can be added to a cleanup plan (`add-cleanup-plans`), with manual instructions such as "Open in Explorer and delete to the Recycle Bin". **The app never modifies, deletes or hard-links files.**
- Optional **near-duplicate folders**: Jaccard similarity over (relative name, size, hash) sets, at or above a threshold (default 0.9).
- The design records how reading files interacts with last-access times (see design.md).

## Capabilities

### New Capabilities
- `duplicate-finder`: Staged hashing duplicate detection with safe exclusions, an HDD-aware scheduler, a hash cache, duplicate groups with wasted space, keep-rule suggestions, and optional near-duplicate folders.

### Modified Capabilities
None

## Impact

- `crates/diskdash-analysis` (`duplicates` module: funnel, hasher on job-runner worker threads, scheduler), `crates/diskdash-store` (catalog tables `hash_cache`, `dup_runs`, `dup_groups`, `dup_members`, `dup_suggestions`, because groups span drives and snapshots), and `crates/diskdash` routes with ts-rs types. New crates: `blake3`, plus `xxhash-rust` as an optional feature.
- Frontend: a Duplicates tab in the drive view and a cross-drive Duplicates page, built from shared components with designed loading, empty and error states.
- Safety rules touched: **3** (this is the first capability that *reads file contents*: files are opened GENERIC_READ only with full sharing, never for write; cloud placeholders are skipped before open; last-access side effects are covered in design.md), **4** (protected and excluded paths are never suggested), **7** (hashes and paths are logged, never contents).

## Depends on

`add-project-skeleton`, `add-drive-inventory` (media type for scheduling), `add-filesystem-scan`, `add-stale-data-analysis` (time-basis ordering).
