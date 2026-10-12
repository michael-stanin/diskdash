## 1. MFT parser (crates/diskdash-core::ntfs)

- [ ] 1.1 Read-only volume open, `FSCTL_GET_NTFS_VOLUME_DATA`, `$MFT` runlist (record 0, retrieval-pointers fallback); `cargo test` on captured bytes
- [ ] 1.2 FILE record parser (fixups, STANDARD_INFORMATION, FILE_NAME all links, DATA sizes resident/non-resident/sparse/compressed, ATTRIBUTE_LIST second pass) with bounds-checked parsing and no `unsafe`; `cargo test` on record fixtures including corrupt ones; `cargo fuzz` target
- [ ] 1.3 Tree builder → `EntryBatch` (exact hard-link sets with `hardlink_of`, reparse, metadata node, category), compact FRN map; tests asserting the same batch shape as the walk

## 2. Elevated read-only helper (crates/diskdash-helper)

- [ ] 2.1 Shared protocol enum (serde + postcard/bincode frames), named pipe with current-user DACL, remote clients rejected, per-launch secret, closed operation set, GUID/disk allow-list, idle exit; in-process tests for auth rejection, unknown frames and the allow-list
- [ ] 2.2 Operations: VolumeInfo, MftScan (streamed batches, ≤ 64 MB of buffers), UsnRead, SmartCounters, PrefetchLastRun; unit tests with the Win32 layer behind traits
- [ ] 2.3 CI: build the helper, run the write-API deny-list check on the helper crate, `cargo test`

## 3. Server integration (crates/diskdash)

- [ ] 3.1 Helper client and the "Enable elevated features" UAC launch (`ShellExecuteExW` runas), with declined-UAC and early-exit handling; tests with a fake helper
- [ ] 3.2 Scan-mode selection and fallback reasons (helper not running, non-NTFS, folder target, open failure, error rate); tests
- [ ] 3.3 Scan dialog: mode badge, fallback reason, Enable button with progress and error banner; component tests

## 4. Manual quick rescan (USN)

- [ ] 4.1 Catalog `volume_usn_state`; full-rescan triggers (id change, wrap, inactive, no prior state, 20% threshold); tests with fake journal buffers
- [ ] 4.2 Copy-on-write snapshot (SQLite backup API) plus applying changes and ancestor re-aggregation; Quick rescan route and button (progress, error state); tests

## 5. Admin-only equivalence (opt-in)

- [ ] 5.1 `test-fixtures` VHDX builder (create/format/populate/detach/delete under the test temp dir) and the equivalence test (walk vs MFT, then quick rescan vs full) gated by `DISKDASH_ADMIN_TESTS=1`; record throughput and peak memory against the 500 MB bound
