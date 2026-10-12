## Context

Each scan writes its own SQLite snapshot file (owned by `diskdash-store`). The catalog DB indexes them. A snapshot of a 1M-file drive is roughly 150–300 MB, so keeping one per scan quickly dominates disk use, which is ironic for a disk-space tool. Diffs must be fast and must work across snapshots whose entry ids are not stable (ids are per-snapshot file). Forecasts on few, noisy points can mislead, and with manual rescans only, points can be sparse.

## Goals / Non-Goals

**Goals:** a cheap diff on 1M-entry snapshots, bounded disk use for the app's own data, honest forecasts.

**Non-Goals:**
- file-level history beyond the retained full snapshots
- rename/move detection
- scheduled scans or a background watchdog (not planned; rescans are manual)

## Decisions

### D1. Diff by directory path matching across attached snapshot files
The diff job opens the newer snapshot file read-only and `ATTACH`es the older one read-only (`?mode=ro`). Both have a `dir_agg(path_key, logical, allocated, file_count)` table, where `path_key` is the case-folded full path, indexed. The diff is a full outer join (emulated as `LEFT JOIN` ∪ anti-join) on `path_key`, ranked by Δallocated, and returns the top 20 per list with "Show more". Only directories are diffed at full depth. Files are diffed only for "new large files" (size ≥ 100 MiB in the new snapshot, absent by path in the old one). Memory stays bounded because SQLite does the join and only paged rows are returned. The job runs on a worker thread.
- *Alternative: match by file id (FRN).* It lost because file ids are unavailable on FAT32/exFAT and unstable across copy/restore.
- *Alternative: a Merkle-style hash per directory.* It lost because it adds complexity without benefit when only sizes are compared.

### D2. Retention: summarise, then remove the app's own snapshot file
After each successful scan, for each root, the newest N full snapshot files are kept (default 5). For each older unpinned snapshot:
1. Copy its directory aggregates up to depth K (default 4), plus category totals, into catalog `snapshot_summaries(snapshot_id, path_key, depth, logical, allocated, file_count)` in one transaction.
2. Mark the snapshot `detail_level = summary`.
3. Delete its snapshot file. The file is created and owned by the app under `%LOCALAPPDATA%\diskdash\snapshots\`, and the deletion is checked to stay within that directory.

If the process crashes between steps, the next run sees `detail_level = summary` with the file still present and simply deletes it. Summarised snapshots stay diffable at depth ≤ K, with the diff reading the catalog table instead of an attached file. Pinned snapshots are exempt.
- *Alternative: keep everything.* It lost on unbounded growth.
- *Alternative: delete old snapshots entirely.* It lost because long-range trends would disappear.
- *Alternative: delete rows inside the file and VACUUM.* It lost because removing a whole file is simpler and immediate once each snapshot has its own file.

### D3. Free-space samples
Samples go into catalog `volume_samples(volume_guid, ts, total, free, source[inventory|scan])`. A sample is skipped if the previous one for that volume is under 10 minutes old and free space differs by less than 0.1%. Samples older than 90 days are downsampled to one per day (the daily minimum).

### D4. Forecast
Ordinary least squares of free bytes against time, over the last 90 days. The window starts after the last positive jump of more than 5% of capacity, so a cleanup doesn't flatten the slope. A forecast is produced only with ≥ 3 points spanning ≥ 7 days. It reports days until zero and until the low-space limit (10% **or** 20 GiB, whichever is hit first), with R². Below R² 0.5 it is labelled "low confidence". A slope ≥ 0 means "not filling".
- *Alternative: Holt-Winters / ARIMA.* It lost because it is overkill and hard to explain for a few points.

## Risks / Trade-offs

- A rename shows up as "disappeared + appeared" → accepted, and labelled that way rather than as "deleted".
- Summaries lose file-level detail → documented; pinning is available.
- Volume letters change → samples are keyed by volume GUID.
- Sparse samples (manual rescans only) → the forecast gate shows "not enough history" and the UI suggests refreshing occasionally.

## Open Questions

- Whether a total size cap for the app's snapshot directory is needed in addition to N = 5 / K = 4. Decide after measuring real snapshot sizes.
