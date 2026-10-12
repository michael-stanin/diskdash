## 1. Fixtures first (`crates/test-fixtures`)

- [x] 1.1 Write the fixture tree builder (declarative spec → tree under `tempfile::TempDir`, returns expected per-directory logical/allocated totals, counts and max mtime)
- [x] 1.2 Add fixture features: junction (`mklink /J`), junction loop, hard-link pair, symlink (skip if no privilege), 600-char path, denied-ACL dir with guaranteed teardown (Drop guard), sparse file (FSCTL_SET_SPARSE), compressed file (`compact /C`), 1-byte file
- [x] 1.3 Add an integrity manifest helper (size, mtime, ctime, atime, attrs, blake3 before/after) and `assert_tree_unchanged`

## 2. Scanner core (`crates/diskdash-core`)

- [x] 2.1 Add Win32 wrappers via the `windows` crate: enumeration (`FindFirstFileExW` with `FindExInfoBasic` + `FIND_FIRST_EX_LARGE_FETCH`, RAII handle), a metadata-only handle (`FILE_READ_ATTRIBUTES`, `FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT`), `GetFileInformationByHandleEx` (FileStandardInfo, FileIdInfo), `GetCompressedFileSizeW`, `GetVolumeInformationW`, `GetDiskFreeSpaceW`, and the reparse target read; integration tests on fixtures
- [x] 2.2 Write extended-length path helpers (prefix/unprefix, root normalisation); test that a 600-char path round-trips
- [x] 2.3 Write the reparse tag table and classifier as pure functions; table-driven tests, including unknown tags
- [x] 2.4 Implement allocated-size computation (compressed/sparse/cloud/normal) as a pure function plus its Win32 call site; unit tests with mocked attributes, and integration tests on the compressed, sparse and 1-byte fixtures
- [x] 2.5 Write the extension and category table (`video`, `images`, `audio`, `archives`, `installers`, `documents`, `code`, `disk-images`, `system`, `other`) with the system-path override; table-driven tests; expose the table via the API for the UI legend
- [x] 2.6 Implement the iterative DFS walker: post-order aggregation (sizes, counts, max mtime/atime), the hard-link map, per-path error capture, the cancel flag and progress counters (atomics); integration tests for exact totals on the full fixture, junction/loop/hard-link scenarios, the denied dir, and `assert_tree_unchanged`
- [x] 2.7 Implement per-volume hard-link detection (default: on only for the volume containing `%WINDIR%`; settings override per volume) and the `hardlinks_not_deduplicated` flag; tests for both modes on the fixture
- [x] 2.8 Implement the `ScanHandle` API (`progress()`, `cancel()`, `join()`); test that cancel returns in under 2 s
- [x] 2.9 **Performance check**: generate 1 M files (ignored-by-default test), scan on the NVMe (detection on) and on the HDD (detection off), record entries/s and peak memory in `docs/benchmarks.md`; target under 3 min on the NVMe; if missed, raise it before phase 4

## 3. Snapshot storage

- [x] 3.1 Write the catalog migration `0003_snapshots.sql` (`snapshots` with stats, flags, status and resume_state) and the per-snapshot DB schema (entries incl. `name_lower` and per-dir `max_mtime`/`max_atime`, errors, category_totals, ext_totals, indexes per design D6)
- [x] 3.2 Write the batched writer (rusqlite, one thread, bounded crossbeam channel, 5k–50k rows per transaction); test that backpressure blocks the walkers, not memory
- [x] 3.3 Implement the id-block allocator (`block << 40 | seq`) and `last_desc_id` for directories; test that subtree ranges contain exactly the descendants
- [x] 3.4 Precompute category and extension totals at scan end (excluding hard-link repeats); test that the sums equal the root totals
- [x] 3.5 Add catalog/file reconciliation at startup (delete orphan snapshot files via `own_files`, mark missing as failed); test both cases
- [x] 3.6 Exclude our own data directory from scans; test with the data dir placed inside the fixture root

## 4. Scheduling, progress, cancel and resume (`crates/diskdash`)

- [x] 4.1 Add root validation and merge (nonexistent, non-dir, not on an in-scope fixed internal drive, nested merge); unit tests
- [x] 4.2 Implement the per-physical-disk scheduler (HDD concurrency 1, SSD up to N threads over top-level work units, unknown disk treated as HDD); test with a fake inventory that two HDD roots run sequentially
- [x] 4.3 Add progress reporting (entries, bytes, pending dirs, current dir, errors) polled at 4 Hz into job events, with the ETA from the volume's used bytes or the previous snapshot; unit test the ETA math
- [x] 4.4 Implement cancel → `partial` with completed work units recorded; test that it stops in under 2 s
- [x] 4.5 Implement resume (delete incomplete id blocks, re-run, recompute root, refuse on serial mismatch); test with an interrupt injected after the first work unit
- [x] 4.6 Take path locks for the scan roots via the job runner (prevents overlapping scans and, later, duplicate hashing during a scan)

## 5. Query API

- [x] 5.1 Add the scan routes: `POST /api/v1/scans` (roots → job), `POST /api/v1/scans/{job}/resume`
- [x] 5.2 Add the snapshot routes: list, get, delete (409 when running; deletes only our own snapshot file via `own_files`), and latest-complete per root
- [x] 5.3 Add `children` (sort by measure, cursor pagination, 404 for unknown path), `top-files`, `top-dirs` (`under`, `max_depth`; default 20 rows), `breakdown/categories`, `breakdown/extensions`, and `errors` (pagination, code filter, summary); integration tests on a fixture snapshot; every response includes the snapshot id and its end time
- [x] 5.4 Performance check: top-files and children respond in under 1 s on the 1 M-entry benchmark snapshot

## 6. Scan UI (uses `ui-foundation` components)

- [x] 6.1 Build the scan dialog (shadcn Dialog): pick drives as selectable cards with media badges, plus an optional folder path, with inline validation; states: inventory loading skeleton, "no drives" empty state; scans are always started manually
- [x] 6.2 Build the scan activity panel: per-root progress (entries, bytes, current dir truncated in the middle, ETA or indeterminate bar), cancel, resume for partial snapshots; the header scan indicator shows the overall progress; states: idle, running, cancelled, failed with error details
- [x] 6.3 Build the snapshot list (status pill, root, time span, totals, error count, hard-link flag note, delete with confirmation dialog); states: skeleton rows, "No scans yet" empty state with a Scan action, error with retry
- [x] 6.4 Build a drive page placeholder showing the top 20 folders and files for the latest snapshot in the shared data table (logical/allocated toggle, Open in Explorer, Copy path); full visualisation is deferred to phase 4
- [x] 6.5 Manual check: scan `G:\` (SATA SSD) and `D:\` (HDD) at the same time; the UI stays responsive, and totals are within 1% of the Explorer "Properties" size on disk for a sample folder (differences explained by hard links and junctions)
