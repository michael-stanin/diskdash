## Context

Tiering is pure analysis over data the app already has: media type per volume (drive-inventory), snapshot sizes and times, and the per-volume time basis (stale-data-analysis). The non-obvious parts are choosing a target when several drives qualify, judging "hotness" without access telemetry, keeping projections consistent when suggestions compete for the same free space, and giving move instructions that don't break applications.

## Goals / Non-Goals

**Goals:** explainable suggestions, projections that never take a target below its low-space limit, safe manual instructions, and no file access beyond reading library manifests.

**Non-Goals:** performing any move, copy, junction or symlink (the app is strictly read-only); continuous access monitoring (a background watchdog is not planned); USB, removable or network drives as sources or targets.

## Decisions

### D1. Unit of suggestion = highest qualifying folder
As in stale analysis, the outermost folder whose newest descendant is older than the cutoff and whose size is at least the minimum is suggested, not its children. Single large files outside such folders are suggested on their own.

### D2. Hotness heuristic
A folder on an HDD is "hot" if at least X% (default 20%) of its bytes, or at least K files (default 100), were modified within the last N days (default 30). A detected Steam/Epic library or game install also counts as hot when its manifest's `LastPlayed`/`LastUpdated` falls within N days. The UI states the reason ("37% modified in last 30 days", "Steam game last played 3 days ago").
- *Alternative: atime-based hotness.* It lost because atime is unreliable on large volumes (see the stale-data design).

### D3. Greedy target assignment with shared budget
Suggestions are sorted by bytes descending. For each one, the target is the internal drive of the opposite tier with the most projected free space after the move, such that `free_after` stays above the low-space limit (≥ 10% **and** ≥ 20 GiB) plus headroom (default 10% of capacity or 20 GiB, whichever is larger). Projected free space is decremented as suggestions are marked planned. A suggestion with no valid target is listed with reason `no_target_with_space`. Volumes on the same physical disk as the source are never targets.
- *Alternative: optimal bin-packing.* It lost because it is overkill. Greedy is explainable and near-optimal for a handful of drives.

### D4. Instruction templates by kind
- **App library** (Steam/Epic): the launcher's own move feature. A raw copy breaks the launcher's registration.
- **Registered app install folder**: "Reinstall to the target drive via Apps & features." The app recommends against manual moves here, and it does **not** suggest junctions.
- **Plain data folder**: a robocopy line with the source and destination filled in, followed by "check the destination opens correctly, then delete the source via Explorer (Recycle Bin)".

All instructions are rendered as copyable text. Nothing is executed.

## Risks / Trade-offs

- A user moves an app folder by hand and the app breaks → the instruction text warns about this, and app folders get the launcher/reinstall route.
- Stale snapshot data → the snapshot age is shown, and the verification in `cleanup-plans` confirms the result after a rescan.

## Open Questions

None. The defaults are accepted: 12 months, 1 GiB, headroom 10% or 20 GiB, internal drives only.
