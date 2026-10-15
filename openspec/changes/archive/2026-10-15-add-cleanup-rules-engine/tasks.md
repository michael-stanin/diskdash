## 1. Rule model and loader (crates/diskdash-analysis::rules)

- [x] 1.1 Serde rule types (`deny_unknown_fields`; risk enum; kind manual-cleanup/system-setting; conditions; instructions required) plus semantic validation; `cargo test` for valid and invalid rules
- [x] 1.2 Pick and wrap a maintained serde YAML crate behind `parse_rules`; loader for embedded built-ins and the user directory with override/disable and per-file errors with line/column; `cargo test` with a broken YAML fixture
- [x] 1.3 Env-var and known-folder (`SHGetKnownFolderPath`) expansion; `cargo test` with an injected environment

## 2. Evaluator

- [x] 2.1 Snapshot-schema migration for `rule_matches` and the `name_lower` index; catalog savings cache
- [x] 2.2 Path/glob matcher (`globset`; anchored subtree aggregate plus unanchored streaming pass); `cargo test` on a test-fixtures snapshot
- [x] 2.3 Conditions: sibling_exists, child_exists, min_size, min_age_days, extracted_sibling; tests for each, including the node_modules/package.json and pyvenv.cfg cases
- [x] 2.4 Virtual candidates: Recycle Bin via `SHQueryRecycleBinW`, hiberfil/pagefile/swapfile metadata, shadow storage "not measured" without the helper; tests with the Win32 layer behind a trait mock, including access denied
- [x] 2.5 Overlap resolution and savings per (drive, risk); test for no double counting
- [x] 2.6 Job wiring on worker threads (post-scan and on rule change) with SSE progress; integration test on the fixture tree

## 3. Built-in rules

- [x] 3.1 Write `rules/*.yaml` for every category in the spec; test that all embedded built-ins validate and every rule has non-empty instructions
- [x] 3.2 Golden test: a test-fixtures tree with planted temp, dev, browser-cache, Downloads and log items produces the expected match set

## 4. API and UI

- [x] 4.1 Routes: candidates (filter, 20-row paging), savings per drive, rule list with errors; ts-rs types; integration tests
- [x] 4.2 Recommendations page and drive-view cleanup tab (ranking, filters, risk badges, instructions with copy buttons, Open in Explorer). States: row skeletons; empty "Scan a drive to see suggestions" and "Nothing to suggest for this drive"; error with retry. Component tests
- [x] 4.3 Rules page (rule list, overridden/disabled markers, validation errors) and savings on home cards. States: skeleton, "no user rules" empty state, error. Component tests
