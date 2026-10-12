## 1. Backend read API (crates/diskdash-store, crates/diskdash)

- [x] 1.1 Snapshot-schema indexes for top-N by logical/allocated; `cargo test` asserting the query plan uses them
- [x] 1.2 Tree query with depth, limit (5–20, default 20 from settings), size_mode and the "Other" bucket; `cargo test` on a test-fixtures snapshot (sums match, 404)
- [x] 1.3 Paged top-folders/top-files (20 per page) and category/extension breakdown routes with serde types exported via ts-rs; integration tests
- [x] 1.4 `POST /shell/reveal` (snapshot+entry lookup, metadata-only existence check, `explorer.exe` via `raw_arg`, CSRF); tests with the launcher behind a trait mock (204/410/403)

## 2. Frontend foundations (on the skeleton's design system)

- [x] 2.1 Size formatter (binary units, tabular numerals, exact-bytes tooltip) and the size-mode context (persisted); Vitest
- [x] 2.2 `chartTheme(tokens, mode)` and the shared `categoryColor` map with an AA-contrast check in both themes; Vitest

## 3. Drive view

- [x] 3.1 Treemap and sunburst with lazy levels, breadcrumb (keyboard), "incomplete" marker, ≤200 ms transitions with reduced-motion off; states: chart skeleton, "no scan"/"empty folder" empty state, error with retry; component tests
- [x] 3.2 "Other" paged table with re-root; component test
- [x] 3.3 Top folders/files virtualised tables, 20 rows + Show more, sort/filter/drill-down; states: row skeletons, "no files match filter" empty state, error; component tests
- [x] 3.4 Category/extension breakdown wired to the table filter; states: skeleton, empty, error; component test
- [x] 3.5 Row actions: copy path, Open in Explorer (410 → inline in-app error message); component test

## 4. Home page

- [x] 4.1 Drive cards: media badge, health (with the reduced-mode label), fill bar, low-space flag (% OR 20 GiB), last snapshot, savings slot; states: card skeletons, "no internal drives found" empty state, inventory error; component tests
- [x] 4.2 Playwright visual smoke in light and dark at 1280×720 (home and drive view on a fixture snapshot), with screenshots attached to the PR

## Notes

- Chart behaviour is covered by unit tests of the data mapping (`chartOption.test.ts`) plus the Playwright flow that scans a real folder and drills into it in both themes; ECharts itself is not rendered in jsdom.
- Folders are coloured by their dominant category, computed by the scanner during its post-order pass (`top_category`); snapshots from before this change show folders in the neutral colour until rescanned.
- The per-level cap is 5–20 (owner decision), enforced in Settings and on the server.
