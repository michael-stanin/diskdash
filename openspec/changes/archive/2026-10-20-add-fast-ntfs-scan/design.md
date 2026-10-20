## Context

Opening a volume handle (`\\.\C:`) and issuing FSCTLs requires administrator rights. By design, `diskdash.exe` (the web server) never runs elevated. The scanner core lives in `crates/diskdash-core`, and the walk scanner (phase 3) already defines the entry model, hard-link rules and snapshot writer. Snapshots must match walk snapshots in their totals, so that diffs, rules and verification work in either scan mode. A 2 TB NTFS volume can hold over 5M MFT records (about 5 GB of `$MFT`).

## Goals / Non-Goals

**Goals:**
- a full scan of 1M files in well under 30 s on SSD
- totals identical to the walk
- bounded memory
- a strictly read-only elevated component with a minimal attack surface
- a manual quick rescan

**Non-Goals:**
- ReFS/FAT/exFAT fast paths
- writing to or managing the USN journal
- recovering deleted files
- running the server elevated
- continuous monitoring (no watchdog)
- non-internal volumes

## Decisions

### D1. Raw $MFT parsing over FSCTL_ENUM_USN_DATA
`FSCTL_ENUM_USN_DATA` returns FRN, parent FRN, name and attributes, but **no sizes**. A per-file open would be needed, which is barely faster than the walk. Raw parsing instead:
- reads the layout with `FSCTL_GET_NTFS_VOLUME_DATA`
- gets the `$MFT` runlist from record 0, falling back to `FSCTL_GET_RETRIEVAL_POINTERS`
- reads the runs sequentially in 4–16 MiB chunks
- applies update-sequence fixups and parses the attributes

`$ATTRIBUTE_LIST` extension records are resolved in a second pass.
- *Alternative: ENUM_USN_DATA + per-file stat.* Too slow.

### D2. Safe, bounds-checked parsing in diskdash-core
The `ntfs` module parses with explicit bounds checks on every on-disk offset and length: no `unsafe` in the parser, and `unsafe` only in the thin `windows`-crate FFI layer. A corrupt record yields `Err`, not a panic. A `cargo fuzz` target covers `parse_record`. Output is struct-of-arrays batches (`EntryBatch`), the same type the walk produces, so the snapshot writer is shared.
- *Alternative: a C helper.* It lost on memory safety when parsing data that may be corrupt.

### D3. Sizes and link semantics mirror the walk
- Logical size = the unnamed `$DATA` real size. Allocated size = the allocated size, or the total allocated size for compressed and sparse files.
- Alternate data streams follow the walk's rule exactly.
- Multiple `$FILE_NAME` attributes give **exact hard-link sets**. The canonical name follows the walk's rule, and other names get `hardlink_of`, the same as the walk sets on the system volume. On MFT-scanned volumes this is always on, independent of the walk's per-drive hard-link toggle.
- 8.3 names are ignored. Reparse points are recorded and not followed.
- Metafiles go under a synthetic `<NTFS metadata>` node, which the equivalence test excludes.

### D4. Memory bound
Batches are streamed to the snapshot writer. In memory the scanner keeps only a compact FRN → (parent FRN, entry id) map, about 24 B per record, or about 120 MB for 5M records. Directory aggregation runs bottom-up in SQL, as for the walk. Peak target: ≤ 500 MB in the server, and ≤ 64 MB of buffers in the helper, which streams and does not accumulate.

### D5. Manual USN quick rescan
After every MFT scan, catalog `volume_usn_state(volume_guid, journal_id, next_usn, snapshot_id)` is stored. When the user clicks Quick rescan:
1. The helper reads `FSCTL_READ_USN_JOURNAL` from `next_usn` and returns the changed FRNs and their records (`FSCTL_GET_NTFS_FILE_RECORD`).
2. The server creates the new snapshot file as a copy of the previous one (SQLite online backup API), applies the changes, and re-aggregates the affected ancestor chains.

A full MFT scan runs instead when the journal id differs, `next_usn < FirstUsn` (wrapped), the journal is inactive, there is no prior state, or more than 20% of records changed.

### D6. Elevated read-only helper (owner requirement)
`diskdash-helper.exe` is launched via `ShellExecuteExW` with the `runas` verb only when the user clicks "Enable elevated features". If the user declines UAC, the app stays in reduced mode.
- **Pipe:** `\\.\pipe\diskdash-<SID>-<random>`, with a security descriptor granting only the launching user's SID, `PIPE_REJECT_REMOTE_CLIENTS`, and a single instance.
- **Authentication:** a 256-bit secret passed on the command line, compared in constant time.
- **Protocol:** length-prefixed `postcard`/`bincode` frames. A shared `helper-proto` module in the helper crate is re-used by the server, so both ends use one serde enum.
- **Closed operation set:**
  - `VolumeInfo`
  - `MftScan { volume_guid }` (streams `EntryBatch` frames)
  - `UsnRead { volume_guid, from_usn }`
  - `SmartCounters { disk_number }`
  - `PrefetchLastRun`, which returns `(exe name, last-run times)` only

  Volume GUIDs are checked against the helper's own enumeration of fixed internal volumes. There are no path parameters, no write calls, no process spawning and no handle duplication. The helper exits when its client disconnects or after 10 min idle.
- **CI guard:** a `disallowed-methods`/deny-list check runs against the helper crate too, and fails on any write-capable Win32 API.
- *Alternative: run the whole server elevated.* Rejected by the owner and by safety rule 1's intent.
- *Alternative: a Windows service.* It lost because it is a persistent system change.

### D7. Tests
- Parser fixtures: captured record bytes covering resident/non-resident data, attribute lists, bad fixups, and compressed/sparse files, run with `cargo test`. A fuzz target is also kept.
- Helper protocol tests run in-process: unauthenticated clients are rejected, unknown frames are rejected, and the volume-GUID allow-list is enforced.
- Admin equivalence test (`#[ignore]` plus a `DISKDASH_ADMIN_TESTS=1` gate; run in the opt-in CI job):
  1. `test-fixtures` creates a VHDX under the test's temp dir and formats it NTFS.
  2. It populates the fixture tree: hard links, junctions, sparse and compressed files, long paths.
  3. Both scanners run and their totals must be equal.
  4. The test changes some files, runs a quick rescan, and asserts that the result equals a full scan.
  5. The test detaches and deletes its own VHDX.

## Risks / Trade-offs

- Corrupt or unusual MFT → the record is skipped and logged. If more than 1% of records fail, the scan aborts and falls back to the walk.
- BitLocker-locked volume → the open fails → fallback with a reason.
- Helper attack surface → closed operation set, pipe ACL, remote clients rejected, secret, GUID allow-list, fuzzed parser, and CI API deny-list.

## Open Questions

None.
