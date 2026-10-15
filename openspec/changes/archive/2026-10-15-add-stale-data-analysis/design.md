## Context

The NTFS last-access time is often frozen. `disablelastaccess` can be 1 (user-managed, disabled), 0 (user-managed, enabled), `0x80000002` (system-managed, disabled — reported as 2) or `0x80000003` (system-managed, enabled — reported as 3). On Windows 10 1803+ the system-managed default enables updates only on volumes of 128 GB or less, decided at mount time. Even when updates are enabled, NTFS updates atime lazily (about a 1 h granularity). Antivirus, indexers and our own duplicate hashing can also bump atime. "Last used" for apps has no single authoritative source.

## Goals / Non-Goals

**Goals:** never present atime-based staleness as reliable when it isn't, cheap queries over snapshot data, and a best-effort app last-used value with its source shown.

**Non-Goals:** changing the `disablelastaccess` setting (a system setting; the app only shows the command as text), uninstalling apps, and tracking usage continuously (a background watchdog is not planned).

## Decisions

### D1. Per-volume time basis
At scan start, `filesystem-scan` (or this capability, run against the latest inventory) runs `fsutil behavior query disablelastaccess` (a read-only query, no admin needed) via `std::process::Command`. It parses both the numeric value and the "System Managed" text. Values 1 or 2 are treated as **unreliable**. Values 0 or 3 are treated as **reliable**. For value 3 the reliability is additionally assumed only for volumes of 128 GB or less, because Windows applies the system-managed policy per volume size. Each snapshot records the decision as `time_basis = atime|mtime`.
- *Alternative: probe by reading a file and checking whether atime changed.* It lost because it violates read-only-by-default, it is slow, and it has hour-granularity lag.
- *Alternative: always use mtime.* It lost because it throws away useful data on small volumes where atime works.

### D2. Folder staleness = newest descendant time
A folder is as stale as its most recently touched descendant. `max(mtime)` (and `max(atime)`) are aggregated per directory by `scan-snapshots`. Queries then filter `dir_agg.max_time < cutoff` inside the snapshot DB. Memory is bounded: the query streams rows, and results are paged 20 at a time. To avoid listing nested stale folders separately, results show the *highest* stale ancestor that is not the drive root and not a protected system root.

### D3. Score = allocated_size × age_days^α
The score is allocated size times age in days raised to α, with α = 1 by default. It is computed in SQL. The size floor (default 100 MiB for "old large files") keeps the list short.
- *Alternative: a percentile-based composite.* It lost because it is harder to explain in the UI.

### D4. Installed-apps sources and precedence
Size: `EstimatedSize` (KiB) if present. Otherwise, if `InstallLocation` resolves to a snapshot directory, use its allocated aggregate. Otherwise "unknown". Last used: Prefetch last-run times, when the phase-11 read-only helper is running. The helper reads `C:\Windows\Prefetch`, decompresses the Win10 MAM format (`RtlDecompressBufferEx`, LZXPRESS Huffman) and returns only `(exe name, last run times)` through its closed operation `prefetch_last_run`. Matching uses the exe name from `DisplayIcon` or exe files under `InstallLocation`. Otherwise UserAssist is used (HKCU, ROT13-decoded value names, FILETIME at offset 60 for Win7+ format), else "unknown". The UI always shows which source supplied the value.
- *Alternative: WMI `Win32_Product`.* It lost because it is slow and triggers MSI self-repair (a side effect).
- *Alternative: run the server elevated to read Prefetch.* It lost because the server never runs elevated (owner decision).

## Risks / Trade-offs

- Our own duplicate hashing (`duplicate-finder`) can bump atime → stale analysis records the snapshot's times at scan time and never re-reads them after hashing. See the duplicate-finder design.
- UserAssist is per-user and misses apps launched in other ways → it is labelled best effort.
- `fsutil` output is localised → parse the hex value with a regex, not the text, and fall back to "unknown → mtime".

## Open Questions

None. The 12-month default and the 100 MiB floor for "old large files" are accepted. Instructions to enable last-access tracking are shown as copyable text only.
