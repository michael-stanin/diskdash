## 1. Free-space samples and forecast (crates/diskdash-analysis::history, crates/diskdash-store)

- [ ] 1.1 Catalog migration: `volume_samples` (keyed by volume GUID), `snapshot_summaries`, snapshot `pinned`/`detail_level`
- [ ] 1.2 Record samples on inventory refresh and scan completion with de-dup; `cargo test`, including the unavailable-volume case
- [ ] 1.3 OLS forecast with the min-points/span gate, R², not-filling, the % OR 20 GiB limit, and the large-jump reset; tests on synthetic series
- [ ] 1.4 Daily downsampling of samples older than 90 days; test

## 2. Snapshot diff

- [ ] 2.1 Diff over two snapshot files (read-only ATTACH, `path_key` join: growing/shrinking/appeared/disappeared, 20-row paging); tests on two test-fixtures snapshots
- [ ] 2.2 New-large-files and per-category deltas, the different-root rejection, and the summarised-side path; tests
- [ ] 2.3 Perf check: diff of two synthetic 1M-entry snapshots in < 10 s with bounded memory (release-mode `cargo test --ignored` benchmark)

## 3. Retention

- [ ] 3.1 Retention job: summarise into the catalog, mark, delete the app-owned snapshot file (path confined to the snapshots dir), crash recovery, pin exemption; tests including interruption at each step

## 4. API and UI

- [ ] 4.1 Routes: diff, samples, forecast, pin/unpin, retention settings; ts-rs types; integration tests
- [ ] 4.2 History page: snapshot picker and diff tables using shared category colours. States: skeleton; "Need two scans of this root to compare" empty state; error with retry. Component tests
- [ ] 4.3 Free-space chart (token-themed ECharts time series, forecast line with caveat) and forecast badges on home cards. States: chart skeleton; "Not enough history yet" empty state; error. Component tests
