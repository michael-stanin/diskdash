## 1. Time basis (crates/diskdash-analysis::stale)

- [x] 1.1 fsutil `disablelastaccess` parser (hex value regex, values 0–3, localised output) plus the 128 GB rule; `cargo test` over captured outputs, including failure
- [x] 1.2 Store `time_basis` in snapshot metadata and expose it in the snapshot API (ts-rs); integration test

## 2. Stale analysis

- [x] 2.1 Per-directory `max_mtime`/`max_atime` aggregation (provided by scan-snapshots; verify only); `cargo test` on a test-fixtures tree with planted times
- [x] 2.2 Stale query with presets, custom value, highest-stale-ancestor collapse, protection/exclusion filter, invalid-timestamp exclusion count, 20-row paging; tests
- [x] 2.3 Size × age scoring with a size floor; test for ordering
- [x] 2.4 Test that the analysis performs no file opens (the filesystem layer is behind a trait; the test uses a panicking implementation)

## 3. Installed apps (crates/diskdash-inventory::apps)

- [x] 3.1 Uninstall-key reader (HKLM 64/32, HKCU), de-dup and SystemComponent filter; tests with a fake registry trait, including access denied
- [x] 3.2 Size resolution (EstimatedSize → InstallLocation snapshot match → unknown); tests
- [x] 3.3 Last-used: UserAssist ROT13 decode and FILETIME parse; Prefetch via the helper client when available ("helper not running" path otherwise); tests with sample blobs and the no-helper case

## 4. API and UI

- [x] 4.1 Routes for stale lists, old large files and installed apps; integration tests
- [x] 4.2 Stale tab: preset selector, reliability warning banner with copyable fsutil command, tables using the shared category colours. States: table skeletons; empty "No data older than N months"; error with retry. Component tests
- [x] 4.3 Installed apps page with size/last-used source badges and the "Open Apps & features" link. States: skeleton; "No applications found" empty state; registry error. Component tests
